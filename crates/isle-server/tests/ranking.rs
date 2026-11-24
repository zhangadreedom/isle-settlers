//! Ranked rooms: rating at game end, the leaderboard and player profiles.

mod common;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use common::*;
use isle_proto::net::{ClientMsg, SeatView};
use isle_proto::ranking::{Leaderboard, PlayerProfile, RatingChange, SeatRating};
use isle_server::ServerConfig;
use serde_json::{Value, json};
use tokio::time::timeout;

const PW: &str = "correct horse battery";

fn config(dir: PathBuf, placement_games: u32) -> ServerConfig {
    ServerConfig {
        data_dir: Some(dir),
        auth_burst: 0,
        placement_games,
        ..fast_config()
    }
}

async fn register(addr: SocketAddr, username: &str) -> String {
    let body = json!({ "username": username, "password": PW }).to_string();
    let (status, _, body) = http(addr, "POST", "/api/accounts", &[], Some(&body)).await;
    assert_eq!(status, 201, "{body}");
    serde_json::from_str::<Value>(&body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn get<T: serde::de::DeserializeOwned>(addr: SocketAddr, path: &str) -> (u16, Option<T>) {
    let (status, _, body) = http(addr, "GET", path, &[], None).await;
    (status, serde_json::from_str(&body).ok())
}

fn seat_rating(c: &Client, seat: usize) -> Option<SeatRating> {
    match &c.room.as_ref().unwrap().seats[seat] {
        SeatView::Human { rating, .. } => *rating,
        _ => None,
    }
}

/// Two signed-in players and a bot play a 3-seat room to the end. Returns
/// both clients once the room has reported its rating changes (or the game
/// is over, for rooms that are not rated).
async fn play_room(addr: SocketAddr, a: &str, b: Option<&str>, ranked: bool) -> (Client, Client) {
    let host = Client::create_room(addr, 3, "A", Some(a), ranked).await;
    let room = host.room_id.clone();
    let guest = Client::join_as(addr, &room, "B", None, b).await;
    let mut host = host;
    host.until(|c| matches!(c.room.as_ref().unwrap().seats[1], SeatView::Human { .. }))
        .await;
    host.send(&ClientMsg::Start).await;
    let (host, guest) = tokio::join!(
        timeout(Duration::from_secs(90), play_to_end(host)),
        timeout(Duration::from_secs(90), play_to_end(guest))
    );
    (host.expect("host finished"), guest.expect("guest finished"))
}

async fn wait_rated(c: &mut Client) -> Vec<RatingChange> {
    timeout(
        Duration::from_secs(10),
        c.until(|c| !c.room.as_ref().unwrap().rating.is_empty()),
    )
    .await
    .expect("rating changes");
    c.room.as_ref().unwrap().rating.clone()
}

#[tokio::test]
async fn ranked_games_rate_signed_in_players() {
    let dir = temp_dir("ranking-rated");
    let server = Server::start(config(dir.clone(), 5)).await;
    let addr = server.addr;
    let ann = register(addr, "ann").await;
    let bob = register(addr, "bob").await;

    let (mut host, _guest) = play_room(addr, &ann, Some(&bob), true).await;
    assert!(host.room.as_ref().unwrap().ranked);
    let changes = wait_rated(&mut host).await;
    let room = host.room.clone().unwrap();
    let replay = room.replay.clone().expect("replay id");
    let view = host.view.clone().unwrap();
    let points = view.final_points.clone().unwrap();
    let winner = view.winner.unwrap() as usize;
    assert_eq!(changes.len(), 2);
    assert_eq!((changes[0].seat, changes[0].username.as_str()), (0, "ann"));
    assert_eq!((changes[1].seat, changes[1].username.as_str()), (1, "bob"));
    assert!(changes.iter().all(|c| c.before == 1000 && c.after != 1000));
    // The better finisher gains more (bots and guests do not count).
    let place = |s: usize| {
        if s == winner {
            0
        } else {
            1 + (0..3)
                .filter(|&q| q != winner && points[q] > points[s])
                .count()
        }
    };
    if place(0) != place(1) {
        let (better, worse) = if place(0) < place(1) { (0, 1) } else { (1, 0) };
        assert!(changes[better].after > changes[worse].after, "{changes:?}");
    }
    // Seats show the new ratings; still placing, so no tier yet.
    for c in &changes {
        let r = seat_rating(&host, c.seat as usize).expect("seat rating");
        assert_eq!((r.rating, r.tier), (c.after, None));
    }

    // Profiles list the game; nobody is placed yet.
    let (status, profile) = get::<PlayerProfile>(addr, "/api/players/ANN").await;
    let profile = profile.unwrap();
    assert_eq!(status, 200);
    assert_eq!(
        (profile.games, profile.placement_left, profile.rank),
        (1, 4, None)
    );
    assert_eq!(profile.rating, changes[0].after);
    assert_eq!(profile.recent.len(), 1);
    let m = &profile.recent[0];
    assert_eq!(
        (m.id.as_str(), m.seats, m.points),
        (replay.as_str(), 3, points[0])
    );
    assert_eq!(m.place as usize, place(0) + 1);
    assert_eq!((m.before, m.after), (1000, changes[0].after));
    assert_eq!(profile.wins, u32::from(winner == 0));
    let (_, board) = get::<Leaderboard>(addr, "/api/leaderboard").await;
    assert_eq!(board.unwrap().total, 0);

    // A restart reads the result back instead of rating again.
    drop((host, _guest));
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.stop().await;
    let server = Server::start(config(dir.clone(), 5)).await;
    let mut back = Client::join_as(server.addr, &room.id, "x", None, Some(&ann)).await;
    assert_eq!(wait_rated(&mut back).await, changes);
    let (_, profile) = get::<PlayerProfile>(server.addr, "/api/players/ann").await;
    assert_eq!(profile.unwrap().games, 1);
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn only_ranked_rooms_with_two_accounts_count() {
    let dir = temp_dir("ranking-unrated");
    let server = Server::start(config(dir.clone(), 1)).await;
    let addr = server.addr;
    let ann = register(addr, "ann").await;
    let bob = register(addr, "bob").await;

    // A casual room.
    let (host, _) = play_room(addr, &ann, Some(&bob), false).await;
    assert!(!host.room.as_ref().unwrap().ranked);
    // A ranked room with one account and one guest.
    let (host2, _) = play_room(addr, &ann, None, true).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    for c in [&host, &host2] {
        assert!(c.room.as_ref().unwrap().rating.is_empty());
    }
    let (_, profile) = get::<PlayerProfile>(addr, "/api/players/ann").await;
    assert_eq!(profile.unwrap().games, 0);
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_host_chooses_ranked_or_casual() {
    let dir = temp_dir("ranking-toggle");
    let server = Server::start(config(dir.clone(), 5)).await;
    let addr = server.addr;
    let ann = register(addr, "ann").await;
    let mut host = Client::create_room(addr, 3, "A", Some(&ann), true).await;
    assert!(host.room.as_ref().unwrap().ranked);
    let room = host.room_id.clone();
    let mut guest = Client::join(addr, &room, "B", None).await;
    guest.send(&ClientMsg::SetRanked { ranked: false }).await;
    guest.until(|c| !c.errors.is_empty()).await;
    assert_eq!(guest.errors, ["NotHost"]);
    host.send(&ClientMsg::SetRanked { ranked: false }).await;
    guest.until(|c| !c.room.as_ref().unwrap().ranked).await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    host.send(&ClientMsg::SetRanked { ranked: true }).await;
    host.until(|c| !c.errors.is_empty()).await;
    assert_eq!(host.errors, ["AlreadyStarted"]);
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn leaderboard_and_profiles() {
    let dir = temp_dir("ranking-board");
    let server = Server::start(config(dir.clone(), 1)).await;
    let addr = server.addr;
    let ann = register(addr, "ann").await;
    let bob = register(addr, "bob").await;
    let (mut host, _) = play_room(addr, &ann, Some(&bob), true).await;
    let changes = wait_rated(&mut host).await;
    // Placed after one game here: a tier and a rank.
    let r = seat_rating(&host, 0).unwrap();
    assert!(r.tier.is_some());

    let (status, board) = get::<Leaderboard>(addr, "/api/leaderboard").await;
    let board = board.unwrap();
    assert_eq!((status, board.total), (200, 2));
    let mut expected: Vec<(&str, i32)> = changes
        .iter()
        .map(|c| (c.username.as_str(), c.after))
        .collect();
    expected.sort_by_key(|e| -e.1);
    let got: Vec<(&str, i32)> = board
        .entries
        .iter()
        .map(|e| (e.username.as_str(), e.rating))
        .collect();
    if expected[0].1 != expected[1].1 {
        assert_eq!(got, expected);
    }
    assert_eq!(board.entries[0].rank, 1);
    let (_, page) = get::<Leaderboard>(addr, "/api/leaderboard?limit=1&offset=1").await;
    let page = page.unwrap();
    assert_eq!((page.entries.len(), page.entries[0].rank), (1, 2));
    assert_eq!(page.entries[0].username, board.entries[1].username);
    for e in &board.entries {
        let (_, p) = get::<PlayerProfile>(addr, &format!("/api/players/{}", e.username)).await;
        let p = p.unwrap();
        assert_eq!(
            (p.rank, p.rating, p.tier),
            (Some(e.rank), e.rating, Some(e.tier))
        );
    }

    let (status, body) = get::<Value>(addr, "/api/players/nobody").await;
    assert_eq!(
        (status, body.unwrap()["error"].as_str()),
        (404, Some("PlayerNotFound"))
    );
    let (status, _) = get::<Value>(addr, "/api/players/not%20valid").await;
    assert_eq!(status, 404);
    let (status, _) = get::<Value>(addr, "/api/leaderboard?limit=x").await;
    assert_eq!(status, 400);
    let (status, page) = get::<Leaderboard>(addr, "/api/leaderboard?limit=100000").await;
    assert_eq!((status, page.unwrap().entries.len()), (200, 2));
    server.stop().await;

    // Without accounts there is no ranking either.
    let server = Server::start(fast_config()).await;
    let (status, _) = get::<Value>(server.addr, "/api/leaderboard").await;
    assert_eq!(status, 503);
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}
