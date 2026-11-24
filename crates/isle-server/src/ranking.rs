//! Ranked games in the account database: ratings, match history and the
//! leaderboard. Ratings are computed by [`crate::rating`].

use isle_proto::ranking::{
    Leaderboard, LeaderboardEntry, MatchSummary, PlayerProfile, RatingChange, SeatRating,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::account_store::{AccountStore, AccountStoreError, check_canonical, now_secs};
use crate::accounts;
use crate::rating::{self, Skill};

/// One signed-in player's result in a finished game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchEntry {
    pub account_id: i64,
    pub seat: u8,
    /// 1-based place among all seats; lower is better, equal is a tie.
    pub place: u8,
    pub points: u8,
}

/// A player's rating row (defaults for players without ranked games).
#[derive(Clone, Copy, Debug, Default)]
struct Standing {
    skill: Skill,
    games: u32,
    wins: u32,
}

fn standing(conn: &Connection, account_id: i64) -> Result<Standing, AccountStoreError> {
    let row = conn
        .query_row(
            "SELECT mu, sigma, games, wins FROM ratings WHERE account_id = ?1",
            params![account_id],
            |r| {
                Ok(Standing {
                    skill: Skill {
                        mu: r.get(0)?,
                        sigma: r.get(1)?,
                    },
                    games: r.get(2)?,
                    wins: r.get(3)?,
                })
            },
        )
        .optional()?;
    Ok(row.unwrap_or_default())
}

fn seat_rating(s: Standing, placement_games: u32) -> SeatRating {
    SeatRating {
        rating: s.skill.display(),
        tier: (s.games >= placement_games).then(|| s.skill.tier()),
    }
}

fn check_entries(seats: u8, entries: &[MatchEntry]) -> Result<(), AccountStoreError> {
    let bad = |m: &str| Err(AccountStoreError::InvalidMatch(m.into()));
    if !(2..=6).contains(&seats) {
        return bad("seats must be 2..=6");
    }
    if entries.len() < 2 || entries.len() > seats as usize {
        return bad("needs 2 or more signed-in players, at most one per seat");
    }
    for (i, e) in entries.iter().enumerate() {
        if e.seat >= seats || !(1..=seats).contains(&e.place) {
            return bad("seat or place out of range");
        }
        if entries[..i]
            .iter()
            .any(|f| f.account_id == e.account_id || f.seat == e.seat)
        {
            return bad("duplicate player or seat");
        }
    }
    Ok(())
}

impl AccountStore {
    /// Rates a finished ranked game and records it under `id` (its replay
    /// id). Recording an id again changes nothing and returns the changes
    /// stored the first time, so callers may simply retry.
    pub fn record_match(
        &self,
        id: &str,
        seats: u8,
        entries: &[MatchEntry],
    ) -> Result<Vec<RatingChange>, AccountStoreError> {
        check_entries(seats, entries)?;
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let known: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM matches WHERE id = ?1)",
            params![id],
            |r| r.get(0),
        )?;
        if !known {
            let now = now_secs();
            let before = entries
                .iter()
                .map(|e| standing(&tx, e.account_id))
                .collect::<Result<Vec<_>, _>>()?;
            let skills: Vec<Skill> = before.iter().map(|s| s.skill).collect();
            let places: Vec<u32> = entries.iter().map(|e| u32::from(e.place)).collect();
            let after = rating::rate(&skills, &places);
            tx.execute(
                "INSERT INTO matches (id, finished_at, seats) VALUES (?1, ?2, ?3)",
                params![id, now, seats],
            )?;
            for ((e, old), new) in entries.iter().zip(&before).zip(&after) {
                tx.execute(
                    "INSERT INTO ratings (account_id, mu, sigma, games, wins, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (account_id) DO UPDATE SET mu = excluded.mu,
                        sigma = excluded.sigma, games = excluded.games,
                        wins = excluded.wins, updated_at = excluded.updated_at",
                    params![
                        e.account_id,
                        new.mu,
                        new.sigma,
                        old.games + 1,
                        old.wins + u32::from(e.place == 1),
                        now
                    ],
                )?;
                tx.execute(
                    "INSERT INTO match_players
                        (match_id, account_id, seat, place, points, before, after)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        id,
                        e.account_id,
                        e.seat,
                        e.place,
                        e.points,
                        old.skill.display(),
                        new.display()
                    ],
                )?;
            }
        }
        let changes = {
            let mut stmt = tx.prepare(
                "SELECT mp.seat, a.username, mp.before, mp.after
                 FROM match_players mp JOIN accounts a ON a.id = mp.account_id
                 WHERE mp.match_id = ?1 ORDER BY mp.seat",
            )?;
            stmt.query_map(params![id], |r| {
                Ok(RatingChange {
                    seat: r.get(0)?,
                    username: r.get(1)?,
                    before: r.get(2)?,
                    after: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };
        tx.commit()?;
        Ok(changes)
    }

    /// The rating shown next to a signed-in player's name.
    pub fn seat_rating(
        &self,
        account_id: i64,
        placement_games: u32,
    ) -> Result<SeatRating, AccountStoreError> {
        let s = standing(&self.lock(), account_id)?;
        Ok(seat_rating(s, placement_games))
    }

    /// Placed players (`games >= placement_games`), best first.
    pub fn leaderboard(
        &self,
        placement_games: u32,
        limit: u32,
        offset: u32,
    ) -> Result<Leaderboard, AccountStoreError> {
        let conn = self.lock();
        let total: u32 = conn.query_row(
            "SELECT count(*) FROM ratings WHERE games >= ?1",
            params![placement_games],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(
            "SELECT a.username, r.mu, r.sigma, r.games, r.wins
             FROM ratings r JOIN accounts a ON a.id = r.account_id
             WHERE r.games >= ?1
             ORDER BY r.mu - 3 * r.sigma DESC, r.games DESC, a.username
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt
            .query_map(params![placement_games, limit, offset], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Skill {
                        mu: r.get(1)?,
                        sigma: r.get(2)?,
                    },
                    r.get::<_, u32>(3)?,
                    r.get::<_, u32>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut entries = Vec::with_capacity(rows.len());
        for (i, (username, skill, games, wins)) in rows.into_iter().enumerate() {
            check_canonical(&username)?;
            entries.push(LeaderboardEntry {
                rank: offset + i as u32 + 1,
                username,
                rating: skill.display(),
                tier: skill.tier(),
                games,
                wins,
            });
        }
        Ok(Leaderboard { entries, total })
    }

    /// A player's rating, leaderboard rank and latest ranked games.
    pub fn profile(
        &self,
        username: &str,
        placement_games: u32,
        recent: u32,
    ) -> Result<Option<PlayerProfile>, AccountStoreError> {
        let Ok(username) = accounts::normalize_username(username) else {
            return Ok(None);
        };
        let conn = self.lock();
        let Some((id, username)) = conn
            .query_row(
                "SELECT id, username FROM accounts WHERE username = ?1",
                params![username],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        check_canonical(&username)?;
        let s = standing(&conn, id)?;
        let placed = s.games >= placement_games;
        let rank = if placed {
            // Same order as the leaderboard.
            let ordinal = s.skill.mu - 3.0 * s.skill.sigma;
            let ahead: u32 = conn.query_row(
                "SELECT count(*) FROM ratings r JOIN accounts a ON a.id = r.account_id
                 WHERE r.games >= ?1 AND (r.mu - 3 * r.sigma > ?2
                    OR (r.mu - 3 * r.sigma = ?2
                        AND (r.games > ?3 OR (r.games = ?3 AND a.username < ?4))))",
                params![placement_games, ordinal, s.games, username],
                |r| r.get(0),
            )?;
            Some(ahead + 1)
        } else {
            None
        };
        let mut stmt = conn.prepare(
            "SELECT m.id, m.finished_at, m.seats, mp.place, mp.points, mp.before, mp.after
             FROM match_players mp JOIN matches m ON m.id = mp.match_id
             WHERE mp.account_id = ?1
             ORDER BY m.finished_at DESC, m.rowid DESC LIMIT ?2",
        )?;
        let recent = stmt
            .query_map(params![id, recent], |r| {
                Ok(MatchSummary {
                    id: r.get(0)?,
                    finished_at: r.get(1)?,
                    seats: r.get(2)?,
                    place: r.get(3)?,
                    points: r.get(4)?,
                    before: r.get(5)?,
                    after: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let rating = seat_rating(s, placement_games);
        Ok(Some(PlayerProfile {
            username,
            rating: rating.rating,
            tier: rating.tier,
            rank,
            games: s.games,
            wins: s.wins,
            placement_left: placement_games.saturating_sub(s.games),
            recent,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_store::tests::TempDir;
    use isle_proto::ranking::Tier;

    const PW: &str = "correct horse battery";

    fn setup(names: &[&str]) -> (TempDir, AccountStore, Vec<i64>) {
        let tmp = TempDir::new();
        let store = AccountStore::open_for_tests(&tmp.0);
        let ids = names
            .iter()
            .map(|n| store.register(n, PW).expect("register").id)
            .collect();
        (tmp, store, ids)
    }

    fn entry(account_id: i64, seat: u8, place: u8) -> MatchEntry {
        MatchEntry {
            account_id,
            seat,
            place,
            points: 11 - place,
        }
    }

    #[test]
    fn a_match_is_rated_once() {
        let (_tmp, store, ids) = setup(&["ann", "bob", "cy"]);
        let entries = [
            entry(ids[1], 2, 2),
            entry(ids[0], 0, 1),
            entry(ids[2], 3, 4),
        ];
        let first = store.record_match("GAME1", 4, &entries).expect("record");
        let seats: Vec<u8> = first.iter().map(|c| c.seat).collect();
        assert_eq!(seats, [0, 2, 3]);
        assert_eq!(first[0].username, "ann");
        assert!(first.iter().all(|c| c.before == 1000));
        assert!(first[0].after > first[1].after && first[1].after > first[2].after);
        // Replaying the same id is a no-op that returns the same changes.
        let again = store.record_match("GAME1", 4, &entries).expect("again");
        assert_eq!(again, first);
        let ann = store.profile("ANN", 5, 10).unwrap().unwrap();
        assert_eq!((ann.games, ann.wins, ann.rating), (1, 1, first[0].after));
        assert_eq!((ann.tier, ann.rank, ann.placement_left), (None, None, 4));
        assert_eq!(ann.recent.len(), 1);
        assert_eq!(ann.recent[0].id, "GAME1");
        assert_eq!((ann.recent[0].place, ann.recent[0].seats), (1, 4));
        let bob = store.profile("bob", 5, 10).unwrap().unwrap();
        assert_eq!((bob.games, bob.wins), (1, 0));
    }

    #[test]
    fn invalid_results_are_refused() {
        let (_tmp, store, ids) = setup(&["ann", "bob"]);
        let cases: [(u8, Vec<MatchEntry>); 5] = [
            (4, vec![entry(ids[0], 0, 1)]),
            (4, vec![entry(ids[0], 0, 1), entry(ids[0], 1, 2)]),
            (4, vec![entry(ids[0], 0, 1), entry(ids[1], 0, 2)]),
            (4, vec![entry(ids[0], 0, 1), entry(ids[1], 4, 2)]),
            (7, vec![entry(ids[0], 0, 1), entry(ids[1], 1, 2)]),
        ];
        for (seats, entries) in cases {
            assert!(matches!(
                store.record_match("X", seats, &entries),
                Err(AccountStoreError::InvalidMatch(_))
            ));
        }
        // Unknown accounts violate the foreign key and leave no trace.
        let ghost = [entry(ids[0], 0, 1), entry(9999, 1, 2)];
        assert!(store.record_match("Y", 2, &ghost).is_err());
        assert_eq!(store.profile("ann", 5, 10).unwrap().unwrap().games, 0);
    }

    #[test]
    fn leaderboard_lists_placed_players_in_order() {
        let (_tmp, store, ids) = setup(&["ann", "bob", "cy", "dee"]);
        // ann always wins, dee always loses; cy plays one game fewer.
        for g in 0..5 {
            let mut entries = vec![
                entry(ids[0], 0, 1),
                entry(ids[1], 1, 2),
                entry(ids[3], 3, 4),
            ];
            if g > 0 {
                entries.push(entry(ids[2], 2, 3));
            }
            store
                .record_match(&format!("G{g}"), 4, &entries)
                .expect("record");
        }
        let board = store.leaderboard(5, 50, 0).expect("leaderboard");
        assert_eq!(board.total, 3);
        let names: Vec<&str> = board.entries.iter().map(|e| e.username.as_str()).collect();
        assert_eq!(names, ["ann", "bob", "dee"]);
        assert_eq!(
            board.entries.iter().map(|e| e.rank).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!((board.entries[0].games, board.entries[0].wins), (5, 5));
        assert!(board.entries.windows(2).all(|w| w[0].rating >= w[1].rating));
        // Paging keeps ranks.
        let page = store.leaderboard(5, 1, 1).expect("page");
        assert_eq!(page.entries[0].username, "bob");
        assert_eq!(page.entries[0].rank, 2);
        // Profiles agree with the board; cy is still placing.
        for e in &board.entries {
            let p = store.profile(&e.username, 5, 3).unwrap().unwrap();
            assert_eq!(
                (p.rank, p.rating, p.tier),
                (Some(e.rank), e.rating, Some(e.tier))
            );
            assert_eq!(p.recent.len(), 3);
            assert_eq!(p.recent[0].id, "G4");
        }
        let cy = store.profile("cy", 5, 10).unwrap().unwrap();
        assert_eq!(
            (cy.games, cy.placement_left, cy.rank, cy.tier),
            (4, 1, None, None)
        );
        assert_eq!(store.seat_rating(ids[2], 5).unwrap().tier, None);
        assert_eq!(
            store.seat_rating(ids[0], 5).unwrap().tier,
            Some(board.entries[0].tier)
        );
        // A player without games.
        let (_t2, fresh, fresh_ids) = setup(&["eve"]);
        let r = fresh.seat_rating(fresh_ids[0], 5).unwrap();
        assert_eq!((r.rating, r.tier), (1000, None));
        assert_eq!(
            fresh.seat_rating(fresh_ids[0], 0).unwrap().tier,
            Some(Tier::Bronze)
        );
        assert!(fresh.profile("nobody", 5, 10).unwrap().is_none());
        assert!(fresh.profile("not valid!", 5, 10).unwrap().is_none());
    }

    #[test]
    fn version_1_databases_are_migrated() {
        let tmp = TempDir::new();
        let path = tmp.0.join(crate::account_store::DB_FILE_NAME);
        {
            // A database as the first release wrote it.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE accounts (
                    id INTEGER PRIMARY KEY,
                    username TEXT NOT NULL UNIQUE COLLATE NOCASE
                        CHECK (length(username) BETWEEN 1 AND 32
                               AND username = lower(username) COLLATE BINARY),
                    password_hash TEXT NOT NULL CHECK (password_hash LIKE '$argon2id$%'),
                    created_at INTEGER NOT NULL
                 ) STRICT;
                 CREATE TABLE sessions (
                    token_hash BLOB NOT NULL PRIMARY KEY CHECK (length(token_hash) = 32),
                    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                    created_at INTEGER NOT NULL,
                    expires_at INTEGER NOT NULL CHECK (expires_at > created_at)
                 ) STRICT, WITHOUT ROWID;
                 PRAGMA application_id = 1230195781;
                 PRAGMA user_version = 1;",
            )
            .unwrap();
            let hash = crate::account_store::tests::cheap_hash(PW).unwrap();
            for name in ["ann", "bob"] {
                conn.execute(
                    "INSERT INTO accounts (username, password_hash, created_at) VALUES (?1, ?2, 0)",
                    params![name, hash],
                )
                .unwrap();
            }
        }
        let store = AccountStore::open_for_tests(&tmp.0);
        let version: i32 = store
            .lock()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let new = store.login("ann", PW).expect("old account still logs in");
        let ann = new.session.account.id;
        let bob = store.find_account("bob").unwrap().unwrap().id;
        let changes = store
            .record_match("M", 2, &[entry(ann, 0, 1), entry(bob, 1, 2)])
            .unwrap();
        assert_eq!(changes.len(), 2);
        drop(store);
        // Reopening a migrated database changes nothing.
        let store = AccountStore::open_for_tests(&tmp.0);
        assert_eq!(store.profile("bob", 5, 5).unwrap().unwrap().games, 1);
    }
}
