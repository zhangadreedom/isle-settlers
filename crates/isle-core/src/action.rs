//! Actions players submit, events the engine emits, and rule violations.

use std::fmt;

use crate::resources::{CityTrack, Commodity, CommoditySet, Resource, ResourceSet, Terrain};
use crate::state::DevCard;

/// Either a basic resource or an urban commodity (Citadels maritime trade).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
pub enum TradeGood {
    Resource(Resource),
    Commodity(Commodity),
}

impl TradeGood {
    #[inline]
    pub const fn code(self) -> u8 {
        match self {
            TradeGood::Resource(r) => r as u8,
            TradeGood::Commodity(c) => 5 + c as u8,
        }
    }

    #[inline]
    pub fn from_code(b: u8) -> Option<Self> {
        if b < 5 {
            Resource::from_index(b as usize).map(TradeGood::Resource)
        } else {
            Commodity::from_index((b - 5) as usize).map(TradeGood::Commodity)
        }
    }
}

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
    /// Seafaring: a ship on a sea edge. Free during setup and Road Building.
    BuildShip(u8),
    /// Seafaring: move one open ship once per turn.
    MoveShip {
        from: u8,
        to: u8,
    },
    /// Seafaring: resources for gold fields (or Citadels Aqueduct), as many
    /// as owed (fewer if the bank runs short).
    ChooseGold(ResourceSet),
    /// Citadels: discard half of resources + commodities combined after a 7.
    DiscardCitadel {
        resources: ResourceSet,
        commodities: CommoditySet,
    },
    /// Citadels: trade commodities (4:1, or 2:1 with Trade Lv3) or buy a
    /// commodity with resources at the player's maritime ratio.
    MaritimeTradeGood {
        give: TradeGood,
        get: TradeGood,
    },
    /// Citadels: advance `track` by one level; `metropolis` names the city
    /// crowned when claiming a Metropolis at level 4 or 5.
    UpgradeCity {
        track: CityTrack,
        metropolis: Option<u8>,
    },
    /// Citadels: recruit a level-1 knight at `vertex`.
    RecruitKnight(u8),
    /// Citadels: activate an inactive knight at `vertex` with 1 grain.
    ActivateKnight(u8),
    /// Citadels: promote a knight at `vertex` by one level.
    PromoteKnight(u8),
    /// Citadels: move an active knight along own routes (displacing a weaker
    /// opponent knight if present).
    MoveKnight {
        from: u8,
        to: u8,
    },
    /// Citadels: use an active knight adjacent to the robber/pirate to drive
    /// it to `tile` and rob `victim`.
    ChaseRobber {
        vertex: u8,
        tile: u8,
        victim: Option<u8>,
    },
    /// Fishermen: pass the Old Boot to another player with at least as many public VP.
    PassOldBoot(u8),
    /// Fishermen (2 fish): send the robber back to the desert / inland lake.
    FishDriveOffRobber,
    /// Fishermen (3 fish): steal one random card from `victim`.
    FishSteal(u8),
    /// Fishermen (4 fish): take one resource card of choice from the bank.
    FishTakeResource(Resource),
    /// Fishermen (5 fish): build a road on `edge`.
    FishBuildRoad(u8),
    /// Fishermen (7 fish): draw one development card.
    FishBuyDevCard,
    /// Caravans: place a camel caravan segment on `edge`.
    PlaceCamel(u8),
    /// Caravans: build a stone bridge across a golden river `edge`.
    BuildBridge(u8),
    /// Caravans / Explorers (2 coins): buy one resource card of choice from the bank.
    SpendCoinsResource(Resource),
    /// Explorers (1 Wool + 1 Ore): raid a pirate lair on `tile`.
    RaidPirateLair(u8),
    /// Explorers (1 Lumber + 1 Grain): haul fish from a deep shoal on `tile`.
    HaulFishShoal(u8),
    /// Explorers (1 Wool + 1 Grain): trade at a spice island on `tile`.
    TradeSpice(u8),
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
            Action::BuildShip(e) => out.extend([17, e]),
            Action::MoveShip { from, to } => out.extend([18, from, to]),
            Action::ChooseGold(set) => {
                out.push(19);
                out.extend(set.0);
            }
            Action::DiscardCitadel {
                resources,
                commodities,
            } => {
                out.push(20);
                out.extend(resources.0);
                out.extend(commodities.0);
            }
            Action::MaritimeTradeGood { give, get } => out.extend([21, give.code(), get.code()]),
            Action::UpgradeCity { track, metropolis } => {
                out.extend([22, track as u8, metropolis.unwrap_or(u8::MAX)]);
            }
            Action::RecruitKnight(v) => out.extend([23, v]),
            Action::ActivateKnight(v) => out.extend([24, v]),
            Action::PromoteKnight(v) => out.extend([25, v]),
            Action::MoveKnight { from, to } => out.extend([26, from, to]),
            Action::ChaseRobber {
                vertex,
                tile,
                victim,
            } => out.extend([27, vertex, tile, victim.unwrap_or(u8::MAX)]),
            Action::PassOldBoot(target) => out.extend([28, target]),
            Action::FishDriveOffRobber => out.push(29),
            Action::FishSteal(victim) => out.extend([30, victim]),
            Action::FishTakeResource(r) => out.extend([31, resource(r)]),
            Action::FishBuildRoad(e) => out.extend([32, e]),
            Action::FishBuyDevCard => out.push(33),
            Action::PlaceCamel(e) => out.extend([34, e]),
            Action::BuildBridge(e) => out.extend([35, e]),
            Action::SpendCoinsResource(r) => out.extend([36, resource(r)]),
            Action::RaidPirateLair(t) => out.extend([37, t]),
            Action::HaulFishShoal(t) => out.extend([38, t]),
            Action::TradeSpice(t) => out.extend([39, t]),
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
        let cset = |from: usize| -> Option<CommoditySet> {
            Some(CommoditySet(rest.get(from..from + 3)?.try_into().ok()?))
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
            17 => (Action::BuildShip(arg(0)?), 1),
            18 => (
                Action::MoveShip {
                    from: arg(0)?,
                    to: arg(1)?,
                },
                2,
            ),
            19 => (Action::ChooseGold(set(0)?), 5),
            20 => (
                Action::DiscardCitadel {
                    resources: set(0)?,
                    commodities: cset(5)?,
                },
                8,
            ),
            21 => (
                Action::MaritimeTradeGood {
                    give: TradeGood::from_code(arg(0)?)?,
                    get: TradeGood::from_code(arg(1)?)?,
                },
                2,
            ),
            22 => {
                let m = arg(1)?;
                (
                    Action::UpgradeCity {
                        track: CityTrack::from_index(arg(0)? as usize)?,
                        metropolis: (m != u8::MAX).then_some(m),
                    },
                    2,
                )
            }
            23 => (Action::RecruitKnight(arg(0)?), 1),
            24 => (Action::ActivateKnight(arg(0)?), 1),
            25 => (Action::PromoteKnight(arg(0)?), 1),
            26 => (
                Action::MoveKnight {
                    from: arg(0)?,
                    to: arg(1)?,
                },
                2,
            ),
            27 => {
                let victim = arg(2)?;
                (
                    Action::ChaseRobber {
                        vertex: arg(0)?,
                        tile: arg(1)?,
                        victim: (victim != u8::MAX).then_some(victim),
                    },
                    3,
                )
            }
            28 => (Action::PassOldBoot(arg(0)?), 1),
            29 => (Action::FishDriveOffRobber, 0),
            30 => (Action::FishSteal(arg(0)?), 1),
            31 => (Action::FishTakeResource(resource(0)?), 1),
            32 => (Action::FishBuildRoad(arg(0)?), 1),
            33 => (Action::FishBuyDevCard, 0),
            34 => (Action::PlaceCamel(arg(0)?), 1),
            35 => (Action::BuildBridge(arg(0)?), 1),
            36 => (Action::SpendCoinsResource(resource(0)?), 1),
            37 => (Action::RaidPirateLair(arg(0)?), 1),
            38 => (Action::HaulFishShoal(arg(0)?), 1),
            39 => (Action::TradeSpice(arg(0)?), 1),
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
    /// Partner finished their paired turn.
    PairedTurnEnded {
        player: u8,
    },
    ShipBuilt {
        player: u8,
        edge: u8,
    },
    ShipMoved {
        player: u8,
        from: u8,
        to: u8,
    },
    PirateMoved {
        player: u8,
        tile: u8,
    },
    /// Gold fields produced this many picks for these players.
    GoldOwed {
        owed: [u8; crate::MAX_PLAYERS],
    },
    GoldChosen {
        player: u8,
        resources: ResourceSet,
    },
    /// First settlement on a new island: bonus points.
    IslandSettled {
        player: u8,
        island: u8,
        points: u8,
    },
    /// Fog maps: an unexplored tile was turned face up (`reward` is the free
    /// resource card drawn from the bank for regular land, if any).
    TileRevealed {
        player: u8,
        tile: u8,
        terrain: Terrain,
        number: u8,
        reward: Option<Resource>,
    },
    CommoditiesProduced {
        player: u8,
        commodities: CommoditySet,
    },
    CommoditiesDiscarded {
        player: u8,
        commodities: CommoditySet,
    },
    /// `commodity` is `None` in views of players other than thief and victim.
    CommodityStolen {
        thief: u8,
        victim: u8,
        commodity: Option<Commodity>,
    },
    GoodTraded {
        player: u8,
        give: TradeGood,
        ratio: u8,
        get: TradeGood,
    },
    CityUpgraded {
        player: u8,
        track: CityTrack,
        level: u8,
        metropolis: Option<u8>,
    },
    KnightRecruited {
        player: u8,
        vertex: u8,
    },
    KnightActivated {
        player: u8,
        vertex: u8,
    },
    KnightPromoted {
        player: u8,
        vertex: u8,
        level: u8,
    },
    KnightMoved {
        player: u8,
        from: u8,
        to: u8,
        displaced: Option<(u8, Option<u8>)>,
    },
    BarbarianAdvanced {
        pos: u8,
    },
    BarbariansRepelled {
        cities: u8,
        defense: u8,
        sole_defender: Option<u8>,
    },
    CityPillaged {
        player: u8,
        vertex: u8,
    },
    FishCaught {
        player: u8,
        count: u8,
    },
    OldBootDrawn {
        player: u8,
    },
    OldBootPassed {
        from: u8,
        to: u8,
    },
    FishSpent {
        player: u8,
        cost: u8,
    },
    FishResourceTaken {
        player: u8,
        resource: Resource,
    },
    CamelPlaced {
        player: u8,
        edge: u8,
    },
    BridgeBuilt {
        player: u8,
        edge: u8,
    },
    CoinsGained {
        player: u8,
        amount: u8,
    },
    CoinsSpent {
        player: u8,
        cost: u8,
        resource: Resource,
    },
    ExplorerMission {
        player: u8,
        mission: u8,
        step: u8,
        champion_claimed: bool,
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
            Event::CommodityStolen { thief, victim, .. }
                if viewer != Some(thief) && viewer != Some(victim) =>
            {
                Event::CommodityStolen {
                    thief,
                    victim,
                    commodity: None,
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
    /// Roads go on land, ships on the sea.
    WrongGround,
    PirateBlocks,
    ShipAlreadyMoved,
    ShipCannotMove,
    NoGoldOwed,
    WrongGoldAmount,
    /// Friendly robber: may not target a player with 2 or fewer public points.
    FriendlyRobber,
    NoCityForUpgrade,
    InvalidMetropolis,
    NotYourKnight,
    KnightNotActive,
    KnightCannotAct,
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
            RuleError::WrongGround => "roads go on land, ships on the sea",
            RuleError::PirateBlocks => "the pirate blocks that spot",
            RuleError::ShipAlreadyMoved => "only one ship move per turn",
            RuleError::ShipCannotMove => "that ship cannot move",
            RuleError::NoGoldOwed => "you have no gold to pick",
            RuleError::WrongGoldAmount => "pick exactly the gold you are owed",
            RuleError::FriendlyRobber => "friendly robber protects players with 2 or fewer points",
            RuleError::NoCityForUpgrade => "you need a city to advance urban improvements",
            RuleError::InvalidMetropolis => "invalid metropolis city selection",
            RuleError::NotYourKnight => "you have no knight there",
            RuleError::KnightNotActive => "that knight is not active",
            RuleError::KnightCannotAct => "that knight cannot act right now",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for RuleError {}
