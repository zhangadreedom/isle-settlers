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

use crate::{BoardInfo, PlayerView};

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
}

/// What occupies a seat, as everyone sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", content = "value")]
pub enum SeatView {
    Open,
    Human {
        name: String,
        connected: bool,
        /// A bot is playing for this player while they are disconnected.
        autopilot: bool,
    },
    Bot {
        level: BotLevel,
    },
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
}

/// State after an action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Update {
    pub view: PlayerView,
    pub legal: LegalActions,
    /// What happened (redacted for this seat).
    pub events: Vec<Event>,
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
    /// Seats nobody joins are played by bots of level `bots`.
    Create {
        players: u8,
        bots: BotLevel,
        name: String,
    },
    /// First message: join a room. With the token from an earlier `Joined`
    /// you get your seat back; otherwise you take a free seat in the lobby
    /// or watch as a spectator.
    Join {
        room: String,
        name: String,
        token: Option<String>,
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
    /// `TooManyRooms`, `NotHost`, `AlreadyStarted`, `NotStarted`, `Spectator`,
    /// `Stale`, `RateLimited`, `InvalidSeat`.
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
            },
            ClientMsg::Join {
                room: "ABCD".into(),
                name: "B".into(),
                token: None,
            },
            ClientMsg::Act {
                version: 3,
                action: Action::BuildRoad(5),
            },
            ClientMsg::SetSeat {
                seat: 2,
                bot: Some(BotLevel::Easy),
            },
            ClientMsg::Start,
        ];
        for m in msgs {
            let json = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<ClientMsg>(&json).unwrap(), m);
        }
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
