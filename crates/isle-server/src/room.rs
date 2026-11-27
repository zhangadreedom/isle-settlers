//! Room actor: owns seats, connections and the authoritative game state.
//!
//! All mutations happen inside [`Room::run`], one message at a time, so no
//! locking is needed. Every connection gets messages projected for its seat.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use isle_ai::{Bot, bot_by_name};
use isle_core::{Action, Event, GameConfig, GameState, Phase, Scenario, TradeResponse};
use isle_proto::net::{
    BotLevel, ChatLine, ClientMsg, HouseRules, LegalActions, RoomInfo, SeatView, ServerMsg,
    Snapshot, Update, clean_chat, clean_name,
};
use isle_proto::ranking::RatingChange;
use isle_proto::{ReplayPlayer, ReplayRecord, board_info, player_view};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};

use crate::auth::Accounts;
use crate::ranking::MatchEntry;
use crate::registry::Registry;
use crate::store::{ReplayArchive, RoomRecord, SeatAccount, SeatRecord, Store};
use crate::{ServerConfig, ids};

pub type ConnId = u64;

/// What the creator chose for a new room.
#[derive(Clone, Copy, Debug)]
pub struct RoomOptions {
    pub players: u8,
    /// Level of the bots filling free seats.
    pub bots: BotLevel,
    pub ranked: bool,
    pub scenario: Scenario,
    pub rules: HouseRules,
}

fn build_game_config(scenario: Scenario, players: u8, rules: HouseRules) -> GameConfig {
    let mut config = GameConfig::scenario(scenario, players);
    if let Some(vp) = rules.victory_points {
        config.victory_points = vp;
    }
    config.friendly_robber = rules.friendly_robber;
    config.paired_turns = rules.paired_turns;
    config
}

/// Frames queued for one connection's writer task.
#[derive(Debug)]
pub enum Out {
    Text(Arc<str>),
    Ping,
}

/// Bounded so a stalled client cannot grow server memory; when full, the
/// connection is dropped and the client resynchronises on reconnect.
pub type Outbox = mpsc::Sender<Out>;

pub enum RoomCmd {
    Attach {
        name: String,
        token: Option<String>,
        /// Verified account of the connection, if signed in.
        account: Option<SeatAccount>,
        out: Outbox,
        reply: oneshot::Sender<Result<Attached, String>>,
    },
    Detach {
        conn: ConnId,
    },
    Msg {
        conn: ConnId,
        msg: ClientMsg,
    },
    /// Persist and stop (server shutdown).
    Shutdown {
        done: oneshot::Sender<()>,
    },
}

/// A connection accepted by a room (which has already sent `Joined`).
#[derive(Debug)]
pub struct Attached {
    pub conn: ConnId,
}

/// Events kept for the log shown to (re)connecting players.
const LOG_KEEP: usize = 120;
const CHAT_INTERVAL: Duration = Duration::from_millis(600);
/// Saves are coalesced: at most one write per this interval.
const SAVE_DELAY: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
enum SeatKind {
    Open,
    Human {
        name: String,
        token: String,
        account: Option<SeatAccount>,
    },
    Bot(BotLevel),
}

/// Display name: the username for accounts, else the cleaned-up nickname.
fn display_name(name: &str, account: Option<&SeatAccount>, fallback: &str) -> String {
    match account {
        Some(a) => a.username.clone(),
        None => clean_name(name).unwrap_or_else(|| fallback.into()),
    }
}

#[derive(Debug)]
struct Seat {
    kind: SeatKind,
    connections: u32,
    disconnected_at: Option<Instant>,
}

struct Conn {
    seat: Option<u8>,
    out: Outbox,
    last_chat: Option<Instant>,
}

struct Game {
    state: GameState,
    /// One per seat: plays bot seats, and humans' seats on autopilot.
    /// Taken out while a bot thinks on the blocking thread pool.
    bots: Vec<Option<Box<dyn Bot + Send>>>,
    actions: Vec<(u8, Action)>,
    log: VecDeque<Event>,
}

impl Game {
    fn new(state: GameState, seats: &[Seat], seed: u64, config: &ServerConfig) -> Game {
        let bots = seats
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let name = match s.kind {
                    SeatKind::Bot(BotLevel::Hard) => config.hard_bot.as_str(),
                    SeatKind::Bot(level) => level.bot_name(),
                    // Autopilot for disconnected players.
                    _ => "greedy",
                };
                let seed = seed.rotate_left(i as u32 * 8) ^ i as u64;
                let mut bot = bot_by_name(name, seed)
                    .or_else(|| bot_by_name("greedy", seed))
                    .expect("known bot");
                if matches!(s.kind, SeatKind::Bot(_)) {
                    bot.set_propose_trades(true);
                }
                Some(bot)
            })
            .collect();
        Game {
            state,
            bots,
            actions: Vec::new(),
            log: VecDeque::new(),
        }
    }

    fn version(&self) -> u32 {
        self.actions.len() as u32
    }

    fn record(&mut self, seat: u8, action: Action, events: &[Event]) {
        self.actions.push((seat, action));
        self.log.extend(events.iter().copied());
        while self.log.len() > LOG_KEEP {
            self.log.pop_front();
        }
    }
}

pub struct Room {
    id: String,
    config: ServerConfig,
    store: Option<Store>,
    replays: Arc<ReplayArchive>,
    /// Set once the game is over and archived.
    replay_id: Option<String>,
    accounts: Option<Arc<Accounts>>,
    /// The game counts for ratings (with two or more signed-in players).
    ranked: bool,
    scenario: Scenario,
    rules: HouseRules,
    /// Rating changes of the finished ranked game.
    rating: Vec<RatingChange>,
    /// When to (re)read or compute the ratings of a finished ranked game.
    rate_due: Option<Instant>,
    seed: u64,
    bot_level: BotLevel,
    host: u8,
    seats: Vec<Seat>,
    game: Option<Game>,
    conns: HashMap<ConnId, Conn>,
    next_conn: ConnId,
    bot_due: Option<Instant>,
    turn_deadline: Option<Instant>,
    save_due: Option<Instant>,
    idle_since: Option<Instant>,
    /// Autopilot state last announced to clients, per seat.
    autopilot_shown: Vec<bool>,
}

impl Room {
    /// A fresh lobby with the creator on seat 0. Returns the creator's token.
    pub fn new_lobby(
        id: String,
        options: RoomOptions,
        creator: String,
        account: Option<SeatAccount>,
        registry: &Registry,
    ) -> (Room, String) {
        let RoomOptions {
            players,
            bots,
            ranked,
            scenario,
            mut rules,
        } = options;
        if rules.turn_timer_secs == 0 && registry.config().default_turn_timer_secs > 0 {
            rules.turn_timer_secs = registry.config().default_turn_timer_secs;
        }
        let token = ids::token();
        let name = display_name(&creator, account.as_ref(), "玩家 1");
        let now = Instant::now();
        let mut seats: Vec<Seat> = (0..players)
            .map(|_| Seat {
                kind: SeatKind::Open,
                connections: 0,
                disconnected_at: None,
            })
            .collect();
        seats[0].kind = SeatKind::Human {
            name,
            token: token.clone(),
            account,
        };
        let room = Room {
            id,
            config: registry.config().clone(),
            store: registry.store().cloned(),
            replays: registry.replays().clone(),
            replay_id: None,
            accounts: registry.accounts().cloned(),
            ranked,
            scenario,
            rules,
            rating: Vec::new(),
            rate_due: None,
            seed: ids::seed(),
            bot_level: bots,
            host: 0,
            autopilot_shown: vec![false; seats.len()],
            seats,
            game: None,
            conns: HashMap::new(),
            next_conn: 1,
            bot_due: None,
            turn_deadline: None,
            save_due: Some(now),
            idle_since: Some(now),
        };
        (room, token)
    }

    /// Rebuilds a room from its record by replaying the action log.
    pub fn restore(record: RoomRecord, registry: &Registry) -> Result<Room, String> {
        let now = Instant::now();
        let seats: Vec<Seat> = record
            .seats
            .iter()
            .map(|s| {
                let kind = match s {
                    SeatRecord::Open => SeatKind::Open,
                    SeatRecord::Human {
                        name,
                        token,
                        account,
                    } => SeatKind::Human {
                        name: name.clone(),
                        token: token.clone(),
                        account: account.clone(),
                    },
                    SeatRecord::Bot { level } => SeatKind::Bot(*level),
                };
                let disconnected_at = matches!(kind, SeatKind::Human { .. }).then_some(now);
                Seat {
                    kind,
                    connections: 0,
                    disconnected_at,
                }
            })
            .collect();
        if !(2..=isle_core::MAX_PLAYERS).contains(&seats.len())
            || record.host as usize >= seats.len()
        {
            return Err("invalid seats".into());
        }
        let game = if record.started {
            let config = build_game_config(record.scenario, seats.len() as u8, record.rules);
            let state = GameState::new(config, record.seed).map_err(|e| format!("{e:?}"))?;
            let mut game = Game::new(state, &seats, record.seed, registry.config());
            let mut events = Vec::new();
            for (i, (seat, action)) in record.actions.iter().enumerate() {
                events.clear();
                game.state
                    .apply_with(*seat, *action, &mut events)
                    .map_err(|e| format!("action {i} ({action:?}) failed on replay: {e}"))?;
                game.record(*seat, *action, &events);
            }
            Some(game)
        } else {
            None
        };
        let mut room = Room {
            id: record.id,
            config: registry.config().clone(),
            store: registry.store().cloned(),
            replays: registry.replays().clone(),
            replay_id: record.replay,
            accounts: registry.accounts().cloned(),
            ranked: record.ranked,
            scenario: record.scenario,
            rules: record.rules,
            rating: Vec::new(),
            rate_due: None,
            seed: record.seed,
            bot_level: record.bot_level,
            host: record.host,
            autopilot_shown: vec![false; seats.len()],
            seats,
            game,
            conns: HashMap::new(),
            next_conn: 1,
            bot_due: None,
            turn_deadline: None,
            save_due: None,
            idle_since: Some(now),
        };
        room.archive_if_finished();
        // A finished ranked game: read back (or, after a crash, compute)
        // its rating changes.
        if room.ranked && room.replay_id.is_some() {
            room.rate_due = Some(now);
        }
        Ok(room)
    }

    pub async fn run(mut self, mut rx: mpsc::Receiver<RoomCmd>) {
        let mut shutdown = None;
        loop {
            let wake = self.next_wake();
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    Some(RoomCmd::Shutdown { done }) => {
                        shutdown = Some(done);
                        break;
                    }
                    Some(cmd) => self.handle(cmd, Instant::now()),
                    None => break,
                },
                _ = async {
                    match wake {
                        Some(t) => sleep_until(t).await,
                        None => std::future::pending().await,
                    }
                } => {}
            }
            if !self.tick(Instant::now()).await {
                // Idle for too long: the room is gone for good.
                if let Some(store) = &self.store {
                    store.delete(&self.id).await;
                }
                return;
            }
        }
        if self.store.is_some() {
            self.save().await;
        }
        if let Some(done) = shutdown {
            let _ = done.send(());
        }
    }

    // ------------------------------------------------------------- timing

    fn next_wake(&self) -> Option<Instant> {
        let mut wake = [
            self.bot_due,
            self.turn_deadline,
            self.save_due,
            self.rate_due,
            self.idle_deadline(),
            self.next_autopilot(),
        ];
        wake.sort();
        wake.into_iter().flatten().next()
    }

    fn refresh_turn_deadline(&mut self, now: Instant) {
        self.turn_deadline = if self.rules.turn_timer_secs > 0
            && self.humans_online()
            && self.game.as_ref().is_some_and(|g| g.state.winner.is_none())
        {
            Some(now + Duration::from_secs(self.rules.turn_timer_secs as u64))
        } else {
            None
        };
    }

    fn turn_remaining_ms(&self, now: Instant) -> Option<u32> {
        let deadline = self.turn_deadline?;
        if !self.humans_online() || self.game.as_ref().is_none_or(|g| g.state.winner.is_some()) {
            return None;
        }
        let rem = deadline.saturating_duration_since(now).as_millis();
        Some(rem.min(u32::MAX as u128) as u32)
    }

    fn idle_deadline(&self) -> Option<Instant> {
        let since = self.idle_since?;
        let ttl = match &self.game {
            None => self.config.idle_ttl_lobby,
            Some(g) if g.state.winner.is_some() => self.config.idle_ttl_finished,
            Some(_) => self.config.idle_ttl_game,
        };
        Some(since + ttl)
    }

    /// When the next disconnected player's autopilot kicks in.
    fn next_autopilot(&self) -> Option<Instant> {
        self.game.as_ref()?;
        let now = Instant::now();
        self.seats
            .iter()
            .filter(|s| matches!(s.kind, SeatKind::Human { .. }) && s.connections == 0)
            .filter_map(|s| s.disconnected_at.map(|t| t + self.config.autopilot_after))
            .filter(|&t| t > now)
            .min()
    }

    /// Runs due timers. Returns `false` when the room should close.
    async fn tick(&mut self, now: Instant) -> bool {
        if self.bot_due.is_some_and(|t| t <= now) {
            self.bot_due = None;
            self.machine_step().await;
        }
        let now = Instant::now();
        if self.turn_deadline.is_some_and(|t| t <= now) {
            if let Some(p) = self.blocking_human(now) {
                self.step_seat_with_bot(p).await;
            }
            let now = Instant::now();
            if self.turn_deadline.is_some_and(|t| t <= now) {
                self.refresh_turn_deadline(now);
            }
        }
        let now = Instant::now();
        let autopilot: Vec<bool> = (0..self.seats.len() as u8)
            .map(|i| self.on_autopilot(i, now))
            .collect();
        if autopilot != self.autopilot_shown {
            self.autopilot_shown = autopilot;
            self.broadcast_room(now);
            self.reschedule(now);
        }
        if self.rate_due.is_some_and(|t| t <= now) {
            self.rate().await;
        }
        if self.save_due.is_some_and(|t| t <= now) {
            self.save().await;
        }
        !self.idle_deadline().is_some_and(|t| t <= now)
    }

    fn bot_delay(&self) -> Duration {
        let Some(game) = &self.game else {
            return Duration::ZERO;
        };
        let s = &game.state;
        let ms = match s.phase {
            Phase::SetupSettlement | Phase::SetupRoad { .. } => 450.0,
            Phase::PreRoll => 750.0,
            Phase::Discard => 350.0,
            Phase::Main if s.trade.is_some() => 700.0,
            _ => 650.0,
        };
        Duration::from_secs_f64(ms / 1000.0 * self.config.bot_delay_scale)
    }

    fn reschedule(&mut self, now: Instant) {
        if self.machine_actor(now).is_some() {
            if self.bot_due.is_none() {
                self.bot_due = Some(now + self.bot_delay());
            }
        } else {
            self.bot_due = None;
        }
    }

    fn mark_dirty(&mut self, now: Instant) {
        if self.store.is_some() && self.save_due.is_none() {
            self.save_due = Some(now + SAVE_DELAY);
        }
    }

    // ------------------------------------------------------------- seats

    fn humans_online(&self) -> bool {
        self.seats
            .iter()
            .any(|s| matches!(s.kind, SeatKind::Human { .. }) && s.connections > 0)
    }

    fn on_autopilot(&self, seat: u8, now: Instant) -> bool {
        let s = &self.seats[seat as usize];
        self.game.as_ref().is_some_and(|g| g.state.winner.is_none())
            && matches!(s.kind, SeatKind::Human { .. })
            && s.connections == 0
            && s.disconnected_at
                .is_some_and(|t| now >= t + self.config.autopilot_after)
    }

    fn is_machine(&self, seat: u8, now: Instant) -> bool {
        match self.seats[seat as usize].kind {
            SeatKind::Bot(_) | SeatKind::Open => true,
            SeatKind::Human { .. } => self.on_autopilot(seat, now),
        }
    }

    /// The seat a bot should play for now. Play pauses while no human is
    /// connected, so nobody comes back to a game that finished without them.
    fn machine_actor(&self, now: Instant) -> Option<u8> {
        let game = self.game.as_ref()?;
        if !self.humans_online() {
            return None;
        }
        let s = &game.state;
        let n = s.num_players() as u8;
        match s.phase {
            Phase::GameOver => None,
            Phase::Discard => {
                (0..n).find(|&p| s.pending_discard[p as usize] > 0 && self.is_machine(p, now))
            }
            Phase::Main if s.trade.is_some() => {
                let offer = s.trade.expect("checked");
                (0..n)
                    .find(|&q| {
                        q != s.current
                            && offer.responses[q as usize] == TradeResponse::Pending
                            && self.is_machine(q, now)
                    })
                    .or_else(|| {
                        let any_accepted = (0..n).any(|q| {
                            q != s.current
                                && offer.responses[q as usize] == TradeResponse::Accepted
                        });
                        let all_replied = (0..n).all(|q| {
                            q == s.current || offer.responses[q as usize] != TradeResponse::Pending
                        });
                        (self.is_machine(s.current, now) && (any_accepted || all_replied))
                            .then_some(s.current)
                    })
            }
            _ => s.to_move().filter(|&p| self.is_machine(p, now)),
        }
    }

    /// Connected human seat currently holding up the game, if any.
    fn blocking_human(&self, now: Instant) -> Option<u8> {
        let game = self.game.as_ref()?;
        if !self.humans_online() || self.machine_actor(now).is_some() {
            return None;
        }
        let s = &game.state;
        let n = s.num_players() as u8;
        match s.phase {
            Phase::GameOver => None,
            Phase::Discard => {
                (0..n).find(|&p| s.pending_discard[p as usize] > 0 && !self.is_machine(p, now))
            }
            Phase::Main if s.trade.is_some() => {
                let offer = s.trade.expect("checked");
                (0..n)
                    .find(|&q| {
                        q != s.current
                            && offer.responses[q as usize] == TradeResponse::Pending
                            && !self.is_machine(q, now)
                    })
                    .or_else(|| (!self.is_machine(s.current, now)).then_some(s.current))
            }
            _ => s.to_move().filter(|&p| !self.is_machine(p, now)),
        }
    }

    fn seat_name(&self, seat: Option<u8>) -> String {
        match seat.map(|s| &self.seats[s as usize].kind) {
            Some(SeatKind::Human { name, .. }) => name.clone(),
            Some(_) => "AI".into(),
            None => "观众".into(),
        }
    }

    // ------------------------------------------------------------- commands

    fn handle(&mut self, cmd: RoomCmd, now: Instant) {
        match cmd {
            RoomCmd::Attach {
                name,
                token,
                account,
                out,
                reply,
            } => {
                let result = self.attach(name, token, account, out, now);
                let _ = reply.send(result);
            }
            RoomCmd::Detach { conn } => self.detach(conn, now),
            RoomCmd::Msg { conn, msg } => {
                if let Err(code) = self.on_msg(conn, msg, now) {
                    self.send(conn, &ServerMsg::Error { code });
                }
            }
            RoomCmd::Shutdown { .. } => unreachable!("handled in run"),
        }
    }

    fn attach(
        &mut self,
        name: String,
        token: Option<String>,
        account: Option<SeatAccount>,
        out: Outbox,
        now: Instant,
    ) -> Result<Attached, String> {
        let name = display_name(&name, account.as_ref(), "玩家");
        // The seat token wins; otherwise a signed-in player gets back the
        // seat their account holds (another device, cleared storage).
        let by_token = token.as_ref().and_then(|t| {
            self.seats
                .iter()
                .position(|s| matches!(&s.kind, SeatKind::Human { token, .. } if token == t))
        });
        let by_account = account.as_ref().and_then(|a| {
            self.seats.iter().position(
                |s| matches!(&s.kind, SeatKind::Human { account: Some(b), .. } if b.id == a.id),
            )
        });
        let mut seat = by_token.or(by_account);
        let mut token = seat.map(|i| match &self.seats[i].kind {
            SeatKind::Human { token, .. } => token.clone(),
            _ => unreachable!("matched a human seat"),
        });
        if seat.is_none()
            && self.game.is_none()
            && let Some(i) = self
                .seats
                .iter()
                .position(|s| matches!(s.kind, SeatKind::Open))
        {
            let t = ids::token();
            self.seats[i].kind = SeatKind::Human {
                name,
                token: t.clone(),
                account,
            };
            seat = Some(i);
            token = Some(t);
            self.mark_dirty(now);
        }
        let seat = seat.map(|s| s as u8);
        let conn = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(
            conn,
            Conn {
                seat,
                out,
                last_chat: None,
            },
        );
        let was_online = self.humans_online();
        if let Some(s) = seat {
            let s = &mut self.seats[s as usize];
            s.connections += 1;
            s.disconnected_at = None;
        }
        if !was_online && self.humans_online() {
            self.refresh_turn_deadline(now);
        }
        self.idle_since = None;

        self.send(
            conn,
            &ServerMsg::Joined {
                room: self.id.clone(),
                seat,
                token,
            },
        );
        self.broadcast_room(now);
        if self.game.is_some() {
            let snapshot = self.snapshot(seat, now);
            self.send(conn, &snapshot);
        }
        self.reschedule(now);
        Ok(Attached { conn })
    }

    fn detach(&mut self, conn: ConnId, now: Instant) {
        let Some(c) = self.conns.remove(&conn) else {
            return;
        };
        if let Some(s) = c.seat {
            let seat = &mut self.seats[s as usize];
            seat.connections = seat.connections.saturating_sub(1);
            if seat.connections == 0 {
                seat.disconnected_at = Some(now);
            }
        }
        if !self.humans_online() {
            self.turn_deadline = None;
        }
        if self.conns.is_empty() {
            self.idle_since = Some(now);
        }
        // Before the start, a host who leaves hands over to someone present,
        // so the others are not stuck waiting.
        if self.game.is_none()
            && self.seats[self.host as usize].connections == 0
            && let Some(next) = self
                .seats
                .iter()
                .position(|s| matches!(s.kind, SeatKind::Human { .. }) && s.connections > 0)
        {
            self.host = next as u8;
            self.mark_dirty(now);
        }
        self.broadcast_room(now);
        self.reschedule(now);
    }

    fn on_msg(&mut self, conn: ConnId, msg: ClientMsg, now: Instant) -> Result<(), String> {
        let seat = self.conns.get(&conn).ok_or("BadMessage")?.seat;
        match msg {
            ClientMsg::Create { .. } | ClientMsg::Join { .. } => Err("BadMessage".into()),

            ClientMsg::Act { version: _, action } => {
                // `version` is informational: every action is validated
                // against the current state, which is authoritative.
                let seat = seat.ok_or("Spectator")?;
                let game = self.game.as_mut().ok_or("NotStarted")?;
                let mut events = Vec::new();
                game.state
                    .apply_with(seat, action, &mut events)
                    .map_err(|e| format!("{e:?}"))?;
                self.after_action(seat, action, events, now);
                Ok(())
            }

            ClientMsg::SetSeat { seat: target, bot } => {
                if seat != Some(self.host) {
                    return Err("NotHost".into());
                }
                if self.game.is_some() {
                    return Err("AlreadyStarted".into());
                }
                let t = target as usize;
                if t >= self.seats.len()
                    || target == self.host
                    || matches!(self.seats[t].kind, SeatKind::Human { .. })
                {
                    return Err("InvalidSeat".into());
                }
                self.seats[t].kind = bot.map_or(SeatKind::Open, SeatKind::Bot);
                self.mark_dirty(now);
                self.broadcast_room(now);
                Ok(())
            }

            ClientMsg::SetRanked { ranked } => {
                if seat != Some(self.host) {
                    return Err("NotHost".into());
                }
                if self.game.is_some() {
                    return Err("AlreadyStarted".into());
                }
                self.ranked = ranked;
                self.mark_dirty(now);
                self.broadcast_room(now);
                Ok(())
            }

            ClientMsg::SetRules { rules } => {
                if seat != Some(self.host) {
                    return Err("NotHost".into());
                }
                if self.game.is_some() {
                    return Err("AlreadyStarted".into());
                }
                if !rules.is_valid() {
                    return Err("BadMessage".into());
                }
                self.rules = rules;
                self.mark_dirty(now);
                self.broadcast_room(now);
                Ok(())
            }

            ClientMsg::Start => {
                if seat != Some(self.host) {
                    return Err("NotHost".into());
                }
                if self.game.is_some() {
                    return Err("AlreadyStarted".into());
                }
                for s in &mut self.seats {
                    if matches!(s.kind, SeatKind::Open) {
                        s.kind = SeatKind::Bot(self.bot_level);
                    }
                }
                let config = build_game_config(self.scenario, self.seats.len() as u8, self.rules);
                let state = GameState::new(config, self.seed).map_err(|e| format!("{e:?}"))?;
                self.game = Some(Game::new(state, &self.seats, self.seed, &self.config));
                self.refresh_turn_deadline(now);
                tracing::info!(room = %self.id, "game started");
                self.mark_dirty(now);
                self.broadcast_room(now);
                let ids: Vec<ConnId> = self.conns.keys().copied().collect();
                for c in ids {
                    let snapshot = self.snapshot(self.conns[&c].seat, now);
                    self.send(c, &snapshot);
                }
                self.reschedule(now);
                Ok(())
            }

            ClientMsg::Chat { text } => {
                let c = self.conns.get_mut(&conn).ok_or("BadMessage")?;
                if c.last_chat.is_some_and(|t| now < t + CHAT_INTERVAL) {
                    return Err("RateLimited".into());
                }
                c.last_chat = Some(now);
                let text = clean_chat(&text).ok_or("BadMessage")?;
                let line = ServerMsg::Chat(ChatLine {
                    seat,
                    name: self.seat_name(seat),
                    text,
                });
                self.broadcast(&line);
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------- game flow

    /// Lets the bot (or autopilot) to move play once. Bots think on the
    /// blocking thread pool so a long search never stalls other rooms.
    async fn machine_step(&mut self) {
        let Some(p) = self.machine_actor(Instant::now()) else {
            return;
        };
        self.step_seat_with_bot(p).await;
    }

    async fn step_seat_with_bot(&mut self, p: u8) {
        let game = self.game.as_mut().expect("game running");
        let state = game.state;
        let mut legal = Vec::new();
        state.legal_actions(p, &mut legal);
        if legal.is_empty() {
            tracing::error!(room = %self.id, seat = p, "no legal action for actor");
            return;
        }
        let Some(mut bot) = game.bots[p as usize].take() else {
            return;
        };
        let thinking = tokio::task::spawn_blocking(move || {
            let action = bot.choose(&state, p, &legal);
            (bot, action)
        });
        let (bot, action) = match thinking.await {
            Ok(done) => done,
            Err(e) => {
                tracing::error!(room = %self.id, seat = p, "bot panicked: {e}; replacing it");
                let game = self.game.as_mut().expect("game running");
                game.bots[p as usize] = bot_by_name("greedy", self.seed ^ p as u64);
                return;
            }
        };
        let game = self.game.as_mut().expect("game running");
        game.bots[p as usize] = Some(bot);
        let mut events = Vec::new();
        if let Err(e) = game.state.apply_with(p, action, &mut events) {
            tracing::error!(room = %self.id, seat = p, ?action, "bot played an illegal action: {e}");
            return;
        }
        self.after_action(p, action, events, Instant::now());
    }

    fn after_action(&mut self, seat: u8, action: Action, events: Vec<Event>, now: Instant) {
        let game = self.game.as_mut().expect("game running");
        game.record(seat, action, &events);
        if let Some(w) = game.state.winner {
            tracing::info!(room = %self.id, winner = w, actions = game.actions.len(), "game over");
        }
        self.refresh_turn_deadline(now);
        self.broadcast_update(&events, now);
        if self.archive_if_finished() {
            self.broadcast_room(now);
        }
        self.mark_dirty(now);
        self.reschedule(now);
    }

    /// Archives a finished game as a replay (once). Returns whether it did.
    fn archive_if_finished(&mut self) -> bool {
        let Some(game) = self.game.as_ref() else {
            return false;
        };
        if game.state.winner.is_none() || self.replay_id.is_some() {
            return false;
        }
        let players = self
            .seats
            .iter()
            .map(|s| match &s.kind {
                SeatKind::Human { name, .. } => ReplayPlayer {
                    name: name.clone(),
                    bot: None,
                },
                SeatKind::Bot(level) => ReplayPlayer {
                    name: String::new(),
                    bot: Some(*level),
                },
                SeatKind::Open => ReplayPlayer {
                    name: String::new(),
                    bot: Some(self.bot_level),
                },
            })
            .collect();
        let record = ReplayRecord::new(
            game.state.config,
            self.seed,
            players,
            &game.actions,
            game.state.winner,
        );
        let id = ids::room_id();
        self.replays.put(&id, &record);
        tracing::info!(room = %self.id, replay = %id, "replay archived");
        self.replay_id = Some(id);
        if self.ranked {
            self.rate_due = Some(Instant::now());
        }
        self.save_due.get_or_insert(Instant::now());
        true
    }

    /// Rates a finished ranked game (or reads back its stored result) and
    /// tells everyone. Games with fewer than two signed-in players are not
    /// rated. Places: the winner first, then by points, ties shared.
    async fn rate(&mut self) {
        self.rate_due = None;
        let (Some(accounts), Some(game), Some(id)) = (
            self.accounts.clone(),
            self.game.as_ref(),
            self.replay_id.clone(),
        ) else {
            return;
        };
        let Some(winner) = game.state.winner.map(usize::from) else {
            return;
        };
        let n = self.seats.len();
        let points: Vec<u8> = (0..n as u8).map(|p| game.state.victory_points(p)).collect();
        let place = |p: usize| -> u8 {
            if p == winner {
                1
            } else {
                2 + (0..n)
                    .filter(|&q| q != winner && points[q] > points[p])
                    .count() as u8
            }
        };
        let entries: Vec<MatchEntry> = self
            .seats
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match &s.kind {
                SeatKind::Human {
                    account: Some(a), ..
                } => Some(MatchEntry {
                    account_id: a.id,
                    seat: i as u8,
                    place: place(i),
                    points: points[i],
                }),
                _ => None,
            })
            .collect();
        if entries.len() < 2 {
            return;
        }
        match accounts.record_match(id.clone(), n as u8, entries).await {
            Ok(results) => {
                tracing::info!(room = %self.id, replay = %id, players = results.len(), "game rated");
                self.rating = results.iter().map(|(c, _)| c.clone()).collect();
                for (change, rating) in results {
                    if let SeatKind::Human {
                        account: Some(a), ..
                    } = &mut self.seats[change.seat as usize].kind
                    {
                        a.rating = Some(rating);
                    }
                }
                self.mark_dirty(Instant::now());
                self.broadcast_room(Instant::now());
            }
            Err(e) => {
                tracing::error!(room = %self.id, "could not rate the game: {e}");
                // Recording is idempotent: try again later.
                self.rate_due = Some(Instant::now() + Duration::from_secs(30));
            }
        }
    }

    // ------------------------------------------------------------- messages

    fn room_info(&self, you: Option<u8>, now: Instant) -> RoomInfo {
        let seats = self
            .seats
            .iter()
            .enumerate()
            .map(|(i, s)| match &s.kind {
                SeatKind::Open => SeatView::Open,
                SeatKind::Bot(level) => SeatView::Bot { level: *level },
                SeatKind::Human { name, account, .. } => SeatView::Human {
                    name: name.clone(),
                    registered: account.is_some(),
                    rating: account.as_ref().and_then(|a| a.rating),
                    connected: s.connections > 0,
                    autopilot: self.on_autopilot(i as u8, now),
                },
            })
            .collect();
        RoomInfo {
            id: self.id.clone(),
            host: self.host,
            you,
            started: self.game.is_some(),
            seats,
            spectators: self.conns.values().filter(|c| c.seat.is_none()).count() as u32,
            replay: self.replay_id.clone(),
            ranked: self.ranked,
            rating: self.rating.clone(),
            scenario: self.scenario,
            rules: self.rules,
        }
    }

    fn legal_for(&self, viewer: Option<u8>) -> LegalActions {
        let game = self.game.as_ref().expect("game running");
        let mut actions = Vec::new();
        if let Some(seat) = viewer {
            game.state.legal_actions(seat, &mut actions);
        }
        LegalActions {
            version: game.version(),
            actions,
        }
    }

    fn snapshot(&self, viewer: Option<u8>, now: Instant) -> ServerMsg {
        let game = self.game.as_ref().expect("game running");
        ServerMsg::Snapshot(Snapshot {
            board: board_info(&game.state),
            view: player_view(&game.state, viewer),
            legal: self.legal_for(viewer),
            log: game.log.iter().map(|e| e.redacted_for(viewer)).collect(),
            turn_remaining_ms: self.turn_remaining_ms(now),
        })
    }

    /// Sends each connection the update projected for its seat. Views are
    /// built once per distinct viewer.
    fn broadcast_update(&mut self, events: &[Event], now: Instant) {
        let turn_remaining_ms = self.turn_remaining_ms(now);
        let game = self.game.as_ref().expect("game running");
        let mut frames: HashMap<Option<u8>, Arc<str>> = HashMap::new();
        let viewers: Vec<(ConnId, Option<u8>)> =
            self.conns.iter().map(|(id, c)| (*id, c.seat)).collect();
        for (_, viewer) in &viewers {
            frames.entry(*viewer).or_insert_with(|| {
                let msg = ServerMsg::Update(Update {
                    view: player_view(&game.state, *viewer),
                    legal: self.legal_for(*viewer),
                    events: events.iter().map(|e| e.redacted_for(*viewer)).collect(),
                    turn_remaining_ms,
                });
                serde_json::to_string(&msg).expect("serializable").into()
            });
        }
        for (conn, viewer) in viewers {
            self.push(conn, Out::Text(frames[&viewer].clone()));
        }
    }

    fn broadcast_room(&mut self, now: Instant) {
        let targets: Vec<(ConnId, Option<u8>)> =
            self.conns.iter().map(|(id, c)| (*id, c.seat)).collect();
        for (conn, seat) in targets {
            let msg = ServerMsg::Room(self.room_info(seat, now));
            self.send(conn, &msg);
        }
    }

    fn broadcast(&mut self, msg: &ServerMsg) {
        let frame: Arc<str> = serde_json::to_string(msg).expect("serializable").into();
        let targets: Vec<ConnId> = self.conns.keys().copied().collect();
        for conn in targets {
            self.push(conn, Out::Text(frame.clone()));
        }
    }

    fn send(&mut self, conn: ConnId, msg: &ServerMsg) {
        let frame: Arc<str> = serde_json::to_string(msg).expect("serializable").into();
        self.push(conn, Out::Text(frame));
    }

    /// Queues a frame; a connection whose queue is full is dropped.
    fn push(&mut self, conn: ConnId, frame: Out) {
        let Some(c) = self.conns.get(&conn) else {
            return;
        };
        if c.out.try_send(frame).is_err() {
            tracing::warn!(room = %self.id, conn, "dropping slow or closed connection");
            self.detach(conn, Instant::now());
        }
    }

    // ------------------------------------------------------------- storage

    fn record(&self) -> RoomRecord {
        RoomRecord {
            format: RoomRecord::FORMAT,
            id: self.id.clone(),
            seed: self.seed,
            bot_level: self.bot_level,
            host: self.host,
            seats: self
                .seats
                .iter()
                .map(|s| match &s.kind {
                    SeatKind::Open => SeatRecord::Open,
                    SeatKind::Human {
                        name,
                        token,
                        account,
                    } => SeatRecord::Human {
                        name: name.clone(),
                        token: token.clone(),
                        account: account.clone(),
                    },
                    SeatKind::Bot(level) => SeatRecord::Bot { level: *level },
                })
                .collect(),
            started: self.game.is_some(),
            actions: self
                .game
                .as_ref()
                .map(|g| g.actions.clone())
                .unwrap_or_default(),
            replay: self.replay_id.clone(),
            ranked: self.ranked,
            scenario: self.scenario,
            rules: self.rules,
        }
    }

    async fn save(&mut self) {
        self.save_due = None;
        if let Some(store) = &self.store
            && let Err(e) = store.save(self.record()).await
        {
            tracing::error!(room = %self.id, "could not save room: {e}");
        }
    }
}
