//! Messages exchanged over the WebSocket between the browser and the server.
//!
//! Frames are JSON text. A connection belongs to one room: its first message
//! is [`ClientMsg::Create`] or [`ClientMsg::Join`], answered by
//! [`ServerMsg::Joined`]. After that the server pushes [`ServerMsg::Room`]
//! whenever seats or presence change, a [`ServerMsg::Snapshot`] when the game
//! starts or the client (re)connects, and an [`ServerMsg::Update`] after every
//! action. Views and events are always projected for the receiving seat.

use isle_core::{Action, Event};
use serde::{Deserialize, Serialize};

use crate::ranking::{RatingChange, SeatRating};
use crate::{BoardInfo, PlayerView};
use isle_core::Scenario;

/// Maximum length of a player name, in characters.
pub const MAX_NAME_CHARS: usize = 16;
/// Maximum length of a chat message, in characters.
pub const MAX_CHAT_CHARS: usize = 200;

/// Strength of computer players filling seats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum BotLevel {
    Easy,
    Normal,
    Hard,
}

impl BotLevel {
    /// Name understood by `isle_ai::bot_by_name`.
    pub fn bot_name(self) -> &'static str {
        match self {
            BotLevel::Easy => "weighted",
            BotLevel::Normal => "greedy",
            BotLevel::Hard => "mcts",
        }
    }

    /// Level of a bot given by its `bot_by_name` name (with options).
    pub fn of_bot_name(name: &str) -> BotLevel {
        match name.split([':', '/']).next().unwrap_or("") {
            "mcts" => BotLevel::Hard,
            "greedy" => BotLevel::Normal,
            _ => BotLevel::Easy,
        }
    }
}

/// What occupies a seat, as everyone sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", content = "value")]
pub enum SeatView {
    Open,
    Human {
        name: String,
        /// Signed in: `name` is the account's username.
        registered: bool,
        /// Rating of a signed-in player.
        rating: Option<SeatRating>,
        connected: bool,
        /// A bot is playing for this player while they are disconnected.
        autopilot: bool,
    },
    Bot {
        level: BotLevel,
    },
}

/// Optional DIY house rules and per-action turn timer for a room.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HouseRules {
    /// Custom victory point target (`None` = scenario default, else `6..=16`).
    #[serde(default)]
    pub victory_points: Option<u8>,
    #[serde(default)]
    pub friendly_robber: bool,
    #[serde(default)]
    pub paired_turns: bool,
    /// Balanced 36-card dice deck (reshuffled with 5 cards left).
    #[serde(default)]
    pub event_deck: bool,
    /// Per-action turn timer in seconds (`0` = disabled, else `1..=300`).
    #[serde(default)]
    pub turn_timer_secs: u16,
    /// Optional custom map (`IM1:...`), overriding procedural board generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub custom_map: Option<isle_core::CustomMap>,
}

impl HouseRules {
    pub fn is_valid(&self) -> bool {
        self.victory_points.is_none_or(|vp| (6..=16).contains(&vp))
            && self.turn_timer_secs <= 300
            && self
                .custom_map
                .is_none_or(|cm| isle_core::Board::from_custom(&cm).is_ok())
    }

    pub fn is_valid_for(&self, scenario: Scenario, players: u8) -> bool {
        self.is_valid()
            && self.custom_map.is_none_or(|cm| {
                cm.layout == isle_core::GameConfig::scenario(scenario, players).layout()
            })
    }
}

/// Summary of an active public room in `/api/rooms`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PublicRoomSummary {
    pub id: String,
    pub host_name: String,
    pub scenario: Scenario,
    pub humans: u8,
    pub max_players: u8,
    pub spectators: u32,
    pub ranked: bool,
    pub started: bool,
    pub has_custom_map: bool,
}

/// Room membership and lobby state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RoomInfo {
    pub id: String,
    /// Seat of the player who created the room and may change seats / start.
    pub host: u8,
    /// The receiving connection's seat; `None` for spectators.
    pub you: Option<u8>,
    pub started: bool,
    pub seats: Vec<SeatView>,
    pub spectators: u32,
    /// Id of the finished game's replay (`/api/replays/{id}`).
    pub replay: Option<String>,
    /// Ranked room: the game counts for the ratings of signed-in players
    /// when at least two of them play.
    pub ranked: bool,
    /// Rating changes once a ranked game is over and rated.
    pub rating: Vec<RatingChange>,
    pub scenario: Scenario,
    #[serde(default)]
    pub rules: HouseRules,
    #[serde(default)]
    pub is_public: bool,
}

/// Legal actions for the receiving seat, tagged with the state version they
/// belong to. Open-ended trade proposals are never listed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LegalActions {
    pub version: u32,
    pub actions: Vec<Action>,
}

/// Full game state for one connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Snapshot {
    pub board: BoardInfo,
    pub view: PlayerView,
    pub legal: LegalActions,
    /// Recent events (redacted for this seat), oldest first.
    pub log: Vec<Event>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "turnRemainingMs")]
    pub turn_remaining_ms: Option<u32>,
}

/// State after an action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Update {
    pub view: PlayerView,
    pub legal: LegalActions,
    /// What happened (redacted for this seat).
    pub events: Vec<Event>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "turnRemainingMs")]
    pub turn_remaining_ms: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChatLine {
    /// Sender's seat; `None` for spectators.
    pub seat: Option<u8>,
    pub name: String,
    pub text: String,
}

/// Browser to server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", content = "value")]
pub enum ClientMsg {
    /// First message: create a room with `players` seats and take seat 0.
    /// Seats nobody joins are played by bots of level `bots`. With an
    /// account `session` token the seat belongs to that account and `name`
    /// is replaced by its username.
    Create {
        players: u8,
        bots: BotLevel,
        name: String,
        /// Ranked room (see [`RoomInfo::ranked`]).
        #[serde(default)]
        ranked: bool,
        /// Classic, a seafaring map or Citadels (2–6 seats); absent means classic.
        #[serde(default)]
        scenario: Scenario,
        #[serde(default)]
        rules: HouseRules,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional = nullable))]
        session: Option<String>,
        /// Whether this room is listed in the public lobby directory (`/api/rooms`).
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional, as = "Option<bool>"))]
        is_public: bool,
    },
    /// First message: join a room. With the token from an earlier `Joined`
    /// you get your seat back; otherwise you take a free seat in the lobby
    /// or watch as a spectator. With an account `session` token you also get
    /// back the seat that account holds, from any device.
    Join {
        room: String,
        name: String,
        token: Option<String>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(optional = nullable))]
        session: Option<String>,
    },
    /// Play an action computed from `legal.actions` (or a trade proposal) at
    /// state `version`.
    Act {
        version: u32,
        action: Action,
    },
    /// Host only, before the start: `None` opens the seat, `Some` puts a bot there.
    SetSeat {
        seat: u8,
        bot: Option<BotLevel>,
    },
    /// Host only, before the start: make the room ranked or casual.
    SetRanked {
        ranked: bool,
    },
    /// Host only, before the start: update DIY house rules and turn timer.
    SetRules {
        rules: HouseRules,
    },
    /// Host only, before start or in-game: show or hide the room in `/api/rooms`.
    SetPublic {
        is_public: bool,
    },
    /// Host only, before the start: remove a human from `seat`, demoting them to spectator.
    KickSeat {
        seat: u8,
    },
    /// Host only, before the start: swap seats `a` and `b`.
    SwapSeats {
        a: u8,
        b: u8,
    },
    /// Host only, once the game is finished: return to lobby (`!immediate`) or start a new game immediately.
    Rematch {
        immediate: bool,
    },
    /// Host only: start the game. Open seats are filled with bots.
    Start,
    Chat {
        text: String,
    },
}

/// Server to browser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", content = "value")]
pub enum ServerMsg {
    /// Answer to `Create` / `Join`. Keep `token` to reconnect to `seat`.
    Joined {
        room: String,
        seat: Option<u8>,
        token: Option<String>,
    },
    Room(RoomInfo),
    Snapshot(Snapshot),
    Update(Update),
    Chat(ChatLine),
    /// `code` is a `RuleError` name or one of: `BadMessage`, `RoomNotFound`,
    /// `TooManyRooms`, `NotHost`, `AlreadyStarted`, `NotStarted`, `NotFinished`,
    /// `Spectator`, `Kicked`, `Stale`, `RateLimited`, `InvalidSeat`,
    /// `SessionExpired` (the account session sent with `Create` / `Join` is
    /// unknown, expired or revoked), `AccountsDisabled`, `ServerError`
    /// (temporary, retry).
    Error {
        code: String,
    },
}

/// Cleans up a user-supplied name: trims, drops control characters and caps
/// the length. Returns `None` if nothing is left.
pub fn clean_name(raw: &str) -> Option<String> {
    let name: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME_CHARS)
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Cleans up a chat message the same way (longer limit).
pub fn clean_chat(raw: &str) -> Option<String> {
    let text: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_CHAT_CHARS)
        .collect();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sanitized() {
        assert_eq!(clean_name("  阿明\n "), Some("阿明".into()));
        assert_eq!(clean_name("\u{7}\t "), None);
        assert_eq!(
            clean_name(&"x".repeat(40)).unwrap().chars().count(),
            MAX_NAME_CHARS
        );
        assert_eq!(
            clean_chat(&"好".repeat(500)).unwrap().chars().count(),
            MAX_CHAT_CHARS
        );
    }

    #[test]
    fn messages_round_trip_as_json() {
        let msgs = [
            ClientMsg::Create {
                players: 4,
                bots: BotLevel::Normal,
                name: "A".into(),
                ranked: true,
                scenario: Scenario::Archipelago,
                rules: HouseRules {
                    victory_points: Some(12),
                    friendly_robber: true,
                    paired_turns: false,
                    event_deck: true,
                    turn_timer_secs: 45,
                    custom_map: None,
                },
                session: Some("ab".repeat(32)),
                is_public: true,
            },
            ClientMsg::Join {
                room: "ABCD".into(),
                name: "B".into(),
                token: None,
                session: None,
            },
            ClientMsg::Act {
                version: 3,
                action: Action::BuildRoad(5),
            },
            ClientMsg::SetSeat {
                seat: 2,
                bot: Some(BotLevel::Easy),
            },
            ClientMsg::SetRanked { ranked: false },
            ClientMsg::SetRules {
                rules: HouseRules {
                    victory_points: Some(8),
                    friendly_robber: false,
                    paired_turns: true,
                    event_deck: false,
                    turn_timer_secs: 60,
                    custom_map: None,
                },
            },
            ClientMsg::SetPublic { is_public: true },
            ClientMsg::KickSeat { seat: 1 },
            ClientMsg::SwapSeats { a: 0, b: 2 },
            ClientMsg::Rematch { immediate: false },
            ClientMsg::Start,
        ];
        for m in msgs {
            let json = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<ClientMsg>(&json).unwrap(), m);
        }
        // Clients from before accounts send no `session`.
        let json = r#"{"type":"Join","value":{"room":"R","name":"N","token":null}}"#;
        assert!(matches!(
            serde_json::from_str::<ClientMsg>(json).unwrap(),
            ClientMsg::Join { session: None, .. }
        ));
        let json = r#"{"type":"Act","value":{"version":1,"action":{"type":"MoveRobber","value":{"tile":3,"victim":null}}}}"#;
        assert_eq!(
            serde_json::from_str::<ClientMsg>(json).unwrap(),
            ClientMsg::Act {
                version: 1,
                action: Action::MoveRobber {
                    tile: 3,
                    victim: None
                }
            }
        );
    }
}
