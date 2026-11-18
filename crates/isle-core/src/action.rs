//! Actions players submit, events the engine emits, and rule violations.

use std::fmt;

use crate::resources::{Resource, ResourceSet};
use crate::state::DevCard;

/// Everything a player can do. The acting seat is passed separately to
/// [`crate::GameState::apply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
pub enum Action {
    RollDice,
    /// Free during setup and Road Building.
    BuildRoad(u8),
    /// Free during setup.
    BuildSettlement(u8),
    BuildCity(u8),
    BuyDevCard,
    PlayKnight,
    PlayRoadBuilding,
    /// Take these cards (two, or fewer if the bank runs short) from the bank.
    PlayYearOfPlenty(ResourceSet),
    PlayMonopoly(Resource),
    /// Move the robber and steal from `victim`. `victim` must be `None` if and
    /// only if nobody on the tile can be robbed.
    MoveRobber {
        tile: u8,
        victim: Option<u8>,
    },
    /// Return exactly half (rounded down) of the hand after a 7.
    Discard(ResourceSet),
    /// Trade with the bank at the best ratio available for `give`.
    MaritimeTrade {
        give: Resource,
        get: Resource,
    },
    /// Offer cards to the other players. Replaces any open offer.
    ProposeTrade {
        give: ResourceSet,
        get: ResourceSet,
    },
    /// Accept or reject the open offer (other players only).
    RespondTrade {
        accept: bool,
    },
    /// Execute the open offer with a player who accepted it.
    ConfirmTrade {
        partner: u8,
    },
    CancelTrade,
    EndTurn,
}

impl Action {
    /// Appends a compact encoding: a tag byte followed by the operands.
    /// Used for replays, where the log of a game should stay small.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let resource = |r: Resource| r as u8;
        match *self {
            Action::RollDice => out.push(0),
            Action::BuildRoad(e) => out.extend([1, e]),
            Action::BuildSettlement(v) => out.extend([2, v]),
            Action::BuildCity(v) => out.extend([3, v]),
            Action::BuyDevCard => out.push(4),
            Action::PlayKnight => out.push(5),
            Action::PlayRoadBuilding => out.push(6),
            Action::PlayYearOfPlenty(set) => {
                out.push(7);
                out.extend(set.0);
            }
            Action::PlayMonopoly(r) => out.extend([8, resource(r)]),
            Action::MoveRobber { tile, victim } => out.extend([9, tile, victim.unwrap_or(u8::MAX)]),
            Action::Discard(set) => {
                out.push(10);
                out.extend(set.0);
            }
            Action::MaritimeTrade { give, get } => out.extend([11, resource(give), resource(get)]),
            Action::ProposeTrade { give, get } => {
                out.push(12);
                out.extend(give.0);
                out.extend(get.0);
            }
            Action::RespondTrade { accept } => out.extend([13, accept as u8]),
            Action::ConfirmTrade { partner } => out.extend([14, partner]),
            Action::CancelTrade => out.push(15),
            Action::EndTurn => out.push(16),
        }
    }

    /// Decodes one action from the front of `bytes` (see [`encode`](Self::encode)),
    /// returning it with the number of bytes read.
    pub fn decode(bytes: &[u8]) -> Option<(Action, usize)> {
        let (&tag, rest) = bytes.split_first()?;
        let arg = |i: usize| rest.get(i).copied();
        let resource = |i: usize| arg(i).and_then(|r| Resource::from_index(r as usize));
        let set = |from: usize| -> Option<ResourceSet> {
            Some(ResourceSet(rest.get(from..from + 5)?.try_into().ok()?))
        };
        let (action, operands) = match tag {
            0 => (Action::RollDice, 0),
            1 => (Action::BuildRoad(arg(0)?), 1),
            2 => (Action::BuildSettlement(arg(0)?), 1),
            3 => (Action::BuildCity(arg(0)?), 1),
            4 => (Action::BuyDevCard, 0),
            5 => (Action::PlayKnight, 0),
            6 => (Action::PlayRoadBuilding, 0),
            7 => (Action::PlayYearOfPlenty(set(0)?), 5),
            8 => (Action::PlayMonopoly(resource(0)?), 1),
            9 => {
                let victim = arg(1)?;
                (
                    Action::MoveRobber {
                        tile: arg(0)?,
                        victim: (victim != u8::MAX).then_some(victim),
                    },
                    2,
                )
            }
            10 => (Action::Discard(set(0)?), 5),
            11 => (
                Action::MaritimeTrade {
                    give: resource(0)?,
                    get: resource(1)?,
                },
                2,
            ),
            12 => (
                Action::ProposeTrade {
                    give: set(0)?,
                    get: set(5)?,
                },
                10,
            ),
            13 => (
                Action::RespondTrade {
                    accept: arg(0)? != 0,
                },
                1,
            ),
            14 => (Action::ConfirmTrade { partner: arg(0)? }, 1),
            15 => (Action::CancelTrade, 0),
            16 => (Action::EndTurn, 0),
            _ => return None,
        };
        Some((action, 1 + operands))
    }
}

/// Something that happened. Some events carry private information
/// (`Stolen`, `DevCardBought`) that must be filtered per viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
pub enum Event {
    DiceRolled {
        player: u8,
        dice: (u8, u8),
    },
    ResourcesProduced {
        player: u8,
        resources: ResourceSet,
    },
    StartingResources {
        player: u8,
        resources: ResourceSet,
    },
    Discarded {
        player: u8,
        resources: ResourceSet,
    },
    RobberMoved {
        player: u8,
        tile: u8,
    },
    /// `resource` is `None` in views of players other than thief and victim.
    Stolen {
        thief: u8,
        victim: u8,
        resource: Option<Resource>,
    },
    RoadBuilt {
        player: u8,
        edge: u8,
    },
    SettlementBuilt {
        player: u8,
        vertex: u8,
    },
    CityBuilt {
        player: u8,
        vertex: u8,
    },
    /// `card` is `None` in views of other players.
    DevCardBought {
        player: u8,
        card: Option<DevCard>,
    },
    DevCardPlayed {
        player: u8,
        card: DevCard,
    },
    YearOfPlentyTaken {
        player: u8,
        resources: ResourceSet,
    },
    MonopolyTaken {
        player: u8,
        resource: Resource,
        taken: [u8; crate::MAX_PLAYERS],
    },
    MaritimeTraded {
        player: u8,
        give: ResourceSet,
        get: Resource,
    },
    TradeProposed {
        player: u8,
        give: ResourceSet,
        get: ResourceSet,
    },
    TradeResponded {
        player: u8,
        accept: bool,
    },
    TradeExecuted {
        player: u8,
        partner: u8,
        give: ResourceSet,
        get: ResourceSet,
    },
    TradeCancelled {
        player: u8,
    },
    LongestRoadChanged {
        owner: Option<u8>,
    },
    LargestArmyChanged {
        owner: Option<u8>,
    },
    TurnEnded {
        player: u8,
    },
    /// A player passed during the special building phase.
    SpecialBuildPassed {
        player: u8,
    },
    GameWon {
        player: u8,
        points: u8,
    },
}

impl Event {
    /// This event as seen by `viewer` (`None` for a spectator): private
    /// details about other players are removed.
    pub fn redacted_for(self, viewer: Option<u8>) -> Event {
        match self {
            Event::Stolen { thief, victim, .. }
                if viewer != Some(thief) && viewer != Some(victim) =>
            {
                Event::Stolen {
                    thief,
                    victim,
                    resource: None,
                }
            }
            Event::DevCardBought { player, .. } if viewer != Some(player) => {
                Event::DevCardBought { player, card: None }
            }
            e => e,
        }
    }
}

/// Receives events from [`crate::GameState::apply_with`]. `()` discards them
/// at zero cost, which is what search algorithms use.
pub trait EventSink {
    fn emit(&mut self, event: Event);
}

impl EventSink for () {
    #[inline(always)]
    fn emit(&mut self, _event: Event) {}
}

impl EventSink for Vec<Event> {
    #[inline]
    fn emit(&mut self, event: Event) {
        self.push(event);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum RuleError {
    GameOver,
    InvalidPlayer,
    NotYourTurn,
    WrongPhase,
    InsufficientResources,
    NoPiecesLeft,
    InvalidVertex,
    InvalidEdge,
    Occupied,
    TooCloseToBuilding,
    NotConnected,
    NotYourSettlement,
    InvalidTile,
    RobberMustMove,
    InvalidVictim,
    NoSuchCard,
    DevCardAlreadyPlayed,
    NoRoadSpot,
    DeckEmpty,
    BankShort,
    InvalidTrade,
    NoOpenTrade,
    PartnerHasNotAccepted,
    WrongDiscardAmount,
    NothingToDiscard,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            RuleError::GameOver => "the game is over",
            RuleError::InvalidPlayer => "no such player",
            RuleError::NotYourTurn => "it is not your turn",
            RuleError::WrongPhase => "not allowed at this point of the turn",
            RuleError::InsufficientResources => "not enough resources",
            RuleError::NoPiecesLeft => "no pieces of that kind left",
            RuleError::InvalidVertex => "no such intersection",
            RuleError::InvalidEdge => "no such path",
            RuleError::Occupied => "that spot is taken",
            RuleError::TooCloseToBuilding => "too close to another building",
            RuleError::NotConnected => "not connected to your roads",
            RuleError::NotYourSettlement => "you have no settlement there",
            RuleError::InvalidTile => "no such tile",
            RuleError::RobberMustMove => "the robber must move to a different tile",
            RuleError::InvalidVictim => "that player cannot be robbed",
            RuleError::NoSuchCard => "you have no playable card of that kind",
            RuleError::DevCardAlreadyPlayed => "only one development card per turn",
            RuleError::NoRoadSpot => "there is nowhere to build a road",
            RuleError::DeckEmpty => "no development cards left",
            RuleError::BankShort => "the bank does not have those cards",
            RuleError::InvalidTrade => "invalid trade",
            RuleError::NoOpenTrade => "there is no open trade offer",
            RuleError::PartnerHasNotAccepted => "that player has not accepted",
            RuleError::WrongDiscardAmount => "you must discard exactly half of your hand",
            RuleError::NothingToDiscard => "you do not need to discard",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for RuleError {}
