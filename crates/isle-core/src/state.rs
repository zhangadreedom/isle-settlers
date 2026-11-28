//! Game state and read-only queries.

use crate::bitset::BitSet;
use crate::board::{Board, CustomMap};
use crate::resources::{CityTrack, CommoditySet, ResourceSet, Terrain};
use crate::rng::Rng;
use crate::topology::*;

/// Maximum number of seats (the extended board seats six).
pub const MAX_PLAYERS: usize = 6;
/// Size of the largest development deck.
pub const MAX_DEV_DECK: usize = 34;
/// Unexplored tiles on the largest fog map (including custom maps).
pub const MAX_FOG_TILES: usize = 24;
/// Number tokens in the largest hidden fog stack (including custom maps).
pub const MAX_FOG_NUMBERS: usize = 24;
pub const MAX_ROADS: u8 = 15;
/// Ships per player on seafaring maps.
pub const MAX_SHIPS: u8 = 15;
pub const MAX_SETTLEMENTS: u8 = 5;
pub const MAX_CITIES: u8 = 4;
/// Knights per rank (Basic, Strong, Mighty) per player in Citadels mode.
pub const MAX_KNIGHTS_PER_LEVEL: u8 = 2;
/// Steps before the barbarian fleet lands in Citadels mode.
pub const BARBARIAN_STEPS: u8 = 7;
/// Maximum fish tokens held by a player in Fishermen mode.
pub const MAX_FISH_HAND: u8 = 7;
/// Stone bridges per player in Caravans mode.
pub const MAX_BRIDGES: u8 = 3;
/// Total camel caravan segments in the supply in Caravans mode.
pub const MAX_CAMELS: u8 = 22;
/// Maximum gold coins a player can hold in Caravans mode.
pub const MAX_COINS: u8 = 99;
/// Minimum gold coins required to claim Wealthiest Settler in Caravans mode.
pub const WEALTHIEST_MIN_COINS: u8 = 4;
/// Size of the balanced 36-card dice deck.
pub const DICE_DECK_SIZE: usize = 36;
/// Cards remaining in the dice deck that trigger the New Year reshuffle.
pub const DICE_DECK_RESHUFFLE_AT: u8 = 5;
/// Players holding more cards than this discard half when a 7 is rolled.
pub const DISCARD_LIMIT: u32 = 7;
pub const LONGEST_ROAD_MIN: u8 = 5;
pub const LARGEST_ARMY_MIN: u8 = 3;

/// Canonical 36 ordered `(d1, d2)` pairs (`1..=6` x `1..=6`).
pub const CANONICAL_DICE_DECK: [(u8, u8); DICE_DECK_SIZE] = {
    let mut deck = [(1u8, 1u8); DICE_DECK_SIZE];
    let mut i = 0;
    let mut d1 = 1u8;
    while d1 <= 6 {
        let mut d2 = 1u8;
        while d2 <= 6 {
            deck[i] = (d1, d2);
            i += 1;
            d2 += 1;
        }
        d1 += 1;
    }
    deck
};

/// Development card types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[repr(u8)]
pub enum DevCard {
    Knight = 0,
    VictoryPoint = 1,
    RoadBuilding = 2,
    YearOfPlenty = 3,
    Monopoly = 4,
}

impl DevCard {
    pub const ALL: [DevCard; 5] = [
        DevCard::Knight,
        DevCard::VictoryPoint,
        DevCard::RoadBuilding,
        DevCard::YearOfPlenty,
        DevCard::Monopoly,
    ];

    /// Copies of each card in the deck of a board.
    pub const fn count_in_deck(self, layout: BoardLayout) -> u8 {
        match (self, layout.is_extended()) {
            (DevCard::Knight, true) => 20,
            (DevCard::Knight, false) => 14,
            (DevCard::VictoryPoint, _) => 5,
            (_, true) => 3,
            (_, false) => 2,
        }
    }

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Counts of development cards indexed by `DevCard`.
pub type DevCounts = [u8; 5];

/// Rule parameters that depend on the board.
impl BoardLayout {
    /// Cards of each resource in the bank at the start.
    pub const fn bank_per_resource(self) -> u8 {
        if self.is_extended() { 24 } else { 19 }
    }

    /// Cards of each commodity in the bank at the start (Citadels mode).
    pub const fn bank_per_commodity(self) -> u8 {
        if self.is_extended() { 18 } else { 12 }
    }

    /// Development cards in the deck.
    pub const fn dev_deck_size(self) -> usize {
        if self.is_extended() { 34 } else { 25 }
    }

    /// After each turn, out-of-turn building (special build or paired turn) occurs.
    pub const fn has_special_build(self) -> bool {
        self.is_extended()
    }

    /// Seats the board is designed for.
    pub const fn max_players(self) -> u8 {
        if self.is_extended() { 6 } else { 4 }
    }

    /// Points for a player's first settlement on each island other than
    /// the ones they started on.
    pub const fn island_bonus(self) -> u8 {
        if self.has_ships() { 2 } else { 0 }
    }
}

/// What is played: the classic island, a seafaring map, Citadels, Fishermen, or Caravans.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Scenario {
    #[default]
    Classic,
    /// Seafaring: the archipelago, 2–6 players.
    Archipelago,
    /// Seafaring: unexplored fog tiles revealed by ships, 2–6 players.
    FogIslands,
    /// Citadels (Cities & Knights): commodities, city tracks, knights and barbarian raids, 2–6 players.
    Citadels,
    /// Fishermen of Catan: coastal fishing grounds, trout lake, fish tokens and the Old Boot, 2–6 players.
    Fishermen,
    /// Caravans & Golden River: oasis, golden river, bridges, camel caravans and gold coins, 2–6 players.
    Caravans,
}

impl Scenario {
    pub const fn victory_points(self) -> u8 {
        match self {
            Scenario::Classic | Scenario::Fishermen => 10,
            Scenario::Archipelago | Scenario::FogIslands | Scenario::Caravans => 12,
            Scenario::Citadels => 13,
        }
    }

    /// Seats this scenario can be played with.
    pub const fn max_players(self) -> u8 {
        MAX_PLAYERS as u8
    }

    #[inline]
    pub const fn is_citadels(self) -> bool {
        matches!(self, Scenario::Citadels)
    }

    #[inline]
    pub const fn is_fishermen(self) -> bool {
        matches!(self, Scenario::Fishermen)
    }

    #[inline]
    pub const fn is_caravans(self) -> bool {
        matches!(self, Scenario::Caravans)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct GameConfig {
    pub num_players: u8,
    pub victory_points: u8,
    /// Keep 6 and 8 tokens off adjacent tiles when generating the board.
    pub balanced_board: bool,
    /// Absent in records from before seafaring: classic.
    #[cfg_attr(feature = "serde", serde(default))]
    pub scenario: Scenario,
    /// Friendly robber: do not move the robber/pirate onto players with <= 2 public VP while avoidable.
    #[cfg_attr(feature = "serde", serde(default))]
    pub friendly_robber: bool,
    /// 5–6 players: use Paired Players turn instead of all-seat Special Build.
    #[cfg_attr(feature = "serde", serde(default))]
    pub paired_turns: bool,
    /// Balanced 36-card dice deck (reshuffled with 5 cards left) instead of independent 2d6 rolls.
    #[cfg_attr(feature = "serde", serde(default))]
    pub event_deck: bool,
    /// Optional custom map (`IM1:...`), overriding procedural board generation.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub custom_map: Option<CustomMap>,
}

impl GameConfig {
    /// Standard rules for `num_players` seats.
    pub fn new(num_players: u8) -> Self {
        GameConfig {
            num_players,
            ..Self::default()
        }
    }

    /// Standard rules of a scenario for `num_players` seats.
    pub fn scenario(scenario: Scenario, num_players: u8) -> Self {
        GameConfig {
            num_players,
            victory_points: scenario.victory_points(),
            scenario,
            ..Self::default()
        }
    }

    /// Board used by this configuration: islands grow to their extended
    /// layouts for 5–6 seats.
    #[inline]
    pub fn layout(&self) -> BoardLayout {
        let ext = self.num_players > 4;
        match self.scenario {
            Scenario::Classic
            | Scenario::Citadels
            | Scenario::Fishermen
            | Scenario::Caravans => BoardLayout::for_players(self.num_players),
            Scenario::Archipelago if ext => BoardLayout::ArchipelagoExtended,
            Scenario::Archipelago => BoardLayout::Archipelago,
            Scenario::FogIslands if ext => BoardLayout::FogIslandsExtended,
            Scenario::FogIslands => BoardLayout::FogIslands,
        }
    }
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            num_players: 4,
            victory_points: 10,
            balanced_board: true,
            scenario: Scenario::Classic,
            friendly_robber: false,
            paired_turns: false,
            event_deck: false,
            custom_map: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    PlayerCount,
    VictoryPoints,
    /// The board does not match the player count.
    BoardLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
pub enum Phase {
    /// Initial placement: the current player places a free settlement.
    SetupSettlement,
    /// Initial placement: a free road touching the settlement just placed.
    SetupRoad {
        vertex: u8,
    },
    /// Start of a turn: roll the dice (a development card may be played first).
    PreRoll,
    /// A 7 was rolled; players listed in `pending_discard` must discard.
    Discard,
    /// The current player must move the robber (after a 7 or a knight), or
    /// on seafaring maps the pirate.
    MoveRobber,
    /// Seafaring: players listed in `pending_gold` pick resources for their
    /// gold fields. During setup `vertex` is the settlement just placed,
    /// whose road or ship comes next.
    Gold {
        vertex: Option<u8>,
    },
    /// Trade, build, buy and play cards in any order, then end the turn.
    Main,
    /// Placing free roads from a Road Building card.
    RoadBuilding {
        remaining: u8,
    },
    /// Extended board: after the current player ends their turn, each other
    /// player in turn order may build or buy development cards (no trading,
    /// no card plays), then pass with `EndTurn`.
    SpecialBuild {
        builder: u8,
    },
    /// 5–6 player Paired Players variant: one partner seat may build, trade
    /// with the bank/harbors and play a development card, then end with `EndTurn`.
    PairedTurn {
        partner: u8,
    },
    /// Caravans: after building a settlement or city, place one camel segment.
    PlaceCamel,
    GameOver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum TradeResponse {
    Pending,
    Accepted,
    Rejected,
}

/// An open offer from the current player to the other players.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TradeOffer {
    /// Cards the current player gives.
    pub give: ResourceSet,
    /// Cards the current player wants in return.
    pub get: ResourceSet,
    pub responses: [TradeResponse; MAX_PLAYERS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlayerState {
    pub hand: ResourceSet,
    /// Citadels: urban commodities (Paper, Cloth, Coin).
    pub commodities: CommoditySet,
    /// Development cards that may be played this turn, plus victory point cards.
    pub dev_cards: DevCounts,
    /// Development cards bought this turn (not playable until the next turn).
    pub new_dev_cards: DevCounts,
    /// Development cards already played face up.
    pub played_dev_cards: DevCounts,
    pub roads_left: u8,
    pub ships_left: u8,
    pub settlements_left: u8,
    pub cities_left: u8,
    /// Citadels: knights remaining by rank (`[Basic, Strong, Mighty]`).
    pub knights_left: [u8; 3],
    /// Citadels: urban improvement level `0..=5` per `CityTrack`.
    pub city_progress: [u8; 3],
    /// Citadels: public victory points earned as sole top defender against barbarians.
    pub defender_points: u8,
    /// Islands this player started on (bit `i` = island `i`).
    pub home_islands: u8,
    /// Islands that earned this player the settling bonus.
    pub islands: u8,
    /// Cached length of this player's longest road.
    pub longest_road: u8,
    /// Harbor access bits (`HarborKind::bit`).
    pub harbors: u8,
    /// Fishermen: fish tokens held (`0..=MAX_FISH_HAND`).
    pub fish: u8,
    /// Caravans: gold coins held (`0..=MAX_COINS`).
    pub coins: u8,
    /// Caravans: stone bridges remaining (`0..=MAX_BRIDGES`).
    pub bridges_left: u8,
}

impl PlayerState {
    const fn new() -> Self {
        PlayerState {
            hand: ResourceSet::EMPTY,
            commodities: CommoditySet::EMPTY,
            dev_cards: [0; 5],
            new_dev_cards: [0; 5],
            played_dev_cards: [0; 5],
            roads_left: MAX_ROADS,
            ships_left: MAX_SHIPS,
            settlements_left: MAX_SETTLEMENTS,
            cities_left: MAX_CITIES,
            knights_left: [MAX_KNIGHTS_PER_LEVEL; 3],
            city_progress: [0; 3],
            defender_points: 0,
            home_islands: 0,
            islands: 0,
            longest_road: 0,
            harbors: 0,
            fish: 0,
            coins: 0,
            bridges_left: MAX_BRIDGES,
        }
    }

    #[inline]
    pub fn total_cards(&self) -> u32 {
        self.hand.total() + self.commodities.total()
    }

    #[inline]
    pub fn knights_played(&self) -> u8 {
        self.played_dev_cards[DevCard::Knight.index()]
    }

    /// Development cards held (playable, new and victory points).
    #[inline]
    pub fn dev_card_count(&self) -> u32 {
        self.dev_cards
            .iter()
            .chain(self.new_dev_cards.iter())
            .map(|&c| c as u32)
            .sum()
    }
}

/// Complete, self-contained game state. `Copy` and heap-free so that search
/// algorithms can clone it with a single `memcpy`.
///
/// Fields are public for fast read access; mutate only through
/// [`GameState::apply`] so the rules and invariants hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GameState {
    pub config: GameConfig,
    pub board: Board,
    pub players: [PlayerState; MAX_PLAYERS],
    pub settlements: [BitSet; MAX_PLAYERS],
    pub cities: [BitSet; MAX_PLAYERS],
    pub roads: [BitSet; MAX_PLAYERS],
    /// Seafaring: ships, on edges along the sea.
    pub ships: [BitSet; MAX_PLAYERS],
    /// Caravans: stone bridges across `board.river_edges`.
    pub bridges: [BitSet; MAX_PLAYERS],
    /// Caravans: neutral camel caravan segments on land edges.
    pub camels: BitSet,
    pub bank: ResourceSet,
    /// Citadels: commodity supply in the bank.
    pub commodity_bank: CommoditySet,
    /// Shuffled deck; cards are drawn from the end of `dev_deck[..dev_deck_len]`.
    pub dev_deck: [DevCard; MAX_DEV_DECK],
    pub dev_deck_len: u8,
    /// Fog maps: shuffled hidden terrain stack (`fog_terrain[..fog_terrain_len]`).
    pub fog_terrain: [Terrain; MAX_FOG_TILES],
    pub fog_terrain_len: u8,
    /// Fog maps: shuffled hidden number stack (`fog_numbers[..fog_numbers_len]`).
    pub fog_numbers: [u8; MAX_FOG_NUMBERS],
    pub fog_numbers_len: u8,
    pub robber: u8,
    /// Sea tile of the pirate, `NONE` on maps without sea.
    pub pirate: u8,
    pub phase: Phase,
    /// Phase resumed once every pending gold choice is made.
    pub after_gold: Phase,
    /// Phase resumed once `Phase::PlaceCamel` resolves.
    pub after_camel: Phase,
    /// Partner seat when inside a Paired Players secondary turn (including interrupts).
    pub in_paired_turn: Option<u8>,
    pub current: u8,
    /// Index into the snake order of initial placements.
    pub setup_step: u8,
    /// Completed turns since the end of setup.
    pub turn: u32,
    /// Dice rolled this turn, `None` before rolling.
    pub dice: Option<(u8, u8)>,
    pub dev_card_played: bool,
    /// The acting player moved a ship this turn.
    pub ship_moved: bool,
    /// Ships the acting player built this turn (they may not move yet).
    pub new_ships: BitSet,
    pub pending_discard: [u8; MAX_PLAYERS],
    /// Gold (or Aqueduct) resources each player still has to pick.
    pub pending_gold: [u8; MAX_PLAYERS],
    pub trade: Option<TradeOffer>,
    pub longest_road_owner: Option<u8>,
    pub largest_army_owner: Option<u8>,
    /// Citadels: owner seat per vertex (`NONE` if no knight).
    pub knight_owner: [u8; MAX_VERTICES],
    /// Citadels: rank (`1..=3`) of the knight at each vertex (`0` if none).
    pub knight_level: [u8; MAX_VERTICES],
    /// Citadels: vertices holding an active knight.
    pub knight_active: BitSet,
    /// Citadels: knights activated this turn (defend immediately, may act next turn).
    pub knight_fresh: BitSet,
    /// Citadels: knights already promoted this turn.
    pub knight_promoted: BitSet,
    /// Citadels: city vertex crowned as the Metropolis for each `CityTrack` (`NONE` if unclaimed).
    pub metropolises: [u8; 3],
    /// Citadels: owner seat of each `CityTrack`'s Metropolis.
    pub metropolis_owners: [Option<u8>; 3],
    /// Citadels: barbarian fleet progress (`0..BARBARIAN_STEPS`).
    pub barbarian_pos: u8,
    /// Balanced 36-card dice deck (`dice_deck[..dice_deck_len]` when `config.event_deck`).
    pub dice_deck: [(u8, u8); DICE_DECK_SIZE],
    pub dice_deck_len: u8,
    /// Fishermen: seat holding the Old Boot (`+1` VP required to win).
    pub old_boot_owner: Option<u8>,
    /// Caravans: seat holding Wealthiest Settler (`+2` VP; 0-coin seats lose 1 VP).
    pub wealthiest_owner: Option<u8>,
    pub winner: Option<u8>,
    pub rng: Rng,
}

impl GameState {
    /// New game with a random board derived from `seed` (or `config.custom_map`).
    pub fn new(config: GameConfig, seed: u64) -> Result<GameState, ConfigError> {
        if !(2..=config.scenario.max_players()).contains(&config.num_players) {
            return Err(ConfigError::PlayerCount);
        }
        if config.victory_points < 3 || config.victory_points > 20 {
            return Err(ConfigError::VictoryPoints);
        }
        let mut rng = Rng::new(seed);
        let board = if let Some(cm) = config.custom_map {
            if cm.layout != config.layout() {
                return Err(ConfigError::BoardLayout);
            }
            Board::from_custom(&cm).map_err(|_| ConfigError::BoardLayout)?
        } else {
            Board::generate(&mut rng, config.layout(), config.balanced_board)
        };
        Self::with_board(config, board, rng.next_u64())
    }

    /// New game on a given board.
    pub fn with_board(
        config: GameConfig,
        mut board: Board,
        seed: u64,
    ) -> Result<GameState, ConfigError> {
        if !(2..=config.scenario.max_players()).contains(&config.num_players) {
            return Err(ConfigError::PlayerCount);
        }
        if config.victory_points < 3 || config.victory_points > 20 {
            return Err(ConfigError::VictoryPoints);
        }
        let layout = config.layout();
        if board.layout != layout {
            return Err(ConfigError::BoardLayout);
        }
        if config.scenario.is_fishermen() {
            board.init_fishing_grounds();
        }
        if config.scenario.is_caravans() {
            board.init_river_edges();
        }
        let mut rng = Rng::new(seed);
        let mut dev_deck = [DevCard::Knight; MAX_DEV_DECK];
        let mut i = 0;
        for card in DevCard::ALL {
            for _ in 0..card.count_in_deck(layout) {
                dev_deck[i] = card;
                i += 1;
            }
        }
        let deck_len = layout.dev_deck_size();
        debug_assert_eq!(i, deck_len);
        rng.shuffle(&mut dev_deck[..deck_len]);
        let mut fog_terrain = [Terrain::Sea; MAX_FOG_TILES];
        let mut fog_terrain_len = 0u8;
        let mut fog_numbers = [0u8; MAX_FOG_NUMBERS];
        let mut fog_numbers_len = 0u8;
        if layout.has_fog() {
            if config.custom_map.is_some() {
                let fog_count = board
                    .tiles()
                    .iter()
                    .filter(|&&t| t == Terrain::Fog)
                    .count();
                let pool = layout.fog_terrain_pool();
                for i in 0..fog_count {
                    fog_terrain[i] = pool[i % pool.len()];
                }
                fog_terrain_len = fog_count as u8;
                rng.shuffle(&mut fog_terrain[..fog_count]);
                let prod = fog_terrain[..fog_count]
                    .iter()
                    .filter(|t| t.produces())
                    .count();
                let npool = layout.fog_number_pool();
                for i in 0..prod {
                    fog_numbers[i] = npool[i % npool.len()];
                }
                fog_numbers_len = prod as u8;
                rng.shuffle(&mut fog_numbers[..prod]);
            } else {
                let ft = layout.fog_terrain_pool();
                fog_terrain[..ft.len()].copy_from_slice(ft);
                fog_terrain_len = ft.len() as u8;
                rng.shuffle(&mut fog_terrain[..ft.len()]);
                let fn_src = layout.fog_number_pool();
                fog_numbers[..fn_src.len()].copy_from_slice(fn_src);
                fog_numbers_len = fn_src.len() as u8;
                rng.shuffle(&mut fog_numbers[..fn_src.len()]);
            }
        }
        let commodity_bank = if config.scenario.is_citadels() {
            CommoditySet([layout.bank_per_commodity(); 3])
        } else {
            CommoditySet::EMPTY
        };
        let (dice_deck, dice_deck_len) = if config.event_deck {
            let mut deck = CANONICAL_DICE_DECK;
            rng.shuffle(&mut deck);
            (deck, DICE_DECK_SIZE as u8)
        } else {
            ([(1, 1); DICE_DECK_SIZE], 0)
        };
        Ok(GameState {
            config,
            board,
            players: [PlayerState::new(); MAX_PLAYERS],
            settlements: [BitSet::EMPTY; MAX_PLAYERS],
            cities: [BitSet::EMPTY; MAX_PLAYERS],
            roads: [BitSet::EMPTY; MAX_PLAYERS],
            ships: [BitSet::EMPTY; MAX_PLAYERS],
            bridges: [BitSet::EMPTY; MAX_PLAYERS],
            camels: BitSet::EMPTY,
            bank: ResourceSet([layout.bank_per_resource(); 5]),
            commodity_bank,
            dev_deck,
            dev_deck_len: deck_len as u8,
            fog_terrain,
            fog_terrain_len,
            fog_numbers,
            fog_numbers_len,
            robber: board.desert,
            pirate: board.pirate_start,
            phase: Phase::SetupSettlement,
            after_gold: Phase::Main,
            after_camel: Phase::Main,
            in_paired_turn: None,
            current: 0,
            setup_step: 0,
            turn: 0,
            dice: None,
            dev_card_played: false,
            ship_moved: false,
            new_ships: BitSet::EMPTY,
            pending_discard: [0; MAX_PLAYERS],
            pending_gold: [0; MAX_PLAYERS],
            trade: None,
            longest_road_owner: None,
            largest_army_owner: None,
            knight_owner: [NONE; MAX_VERTICES],
            knight_level: [0; MAX_VERTICES],
            knight_active: BitSet::EMPTY,
            knight_fresh: BitSet::EMPTY,
            knight_promoted: BitSet::EMPTY,
            metropolises: [NONE; 3],
            metropolis_owners: [None; 3],
            barbarian_pos: 0,
            dice_deck,
            dice_deck_len,
            old_boot_owner: None,
            wealthiest_owner: None,
            winner: None,
            rng,
        })
    }

    #[inline]
    pub fn num_players(&self) -> usize {
        self.config.num_players as usize
    }

    /// Adjacency tables of the board being played.
    #[inline]
    pub fn topo(&self) -> &'static Topology {
        self.board.layout.topology()
    }

    /// The seat whose decisions drive the game outside discards and trade
    /// responses: the builder during a special building phase, the partner
    /// during a paired turn (including its interrupts), otherwise `current`.
    #[inline]
    pub fn active_player(&self) -> u8 {
        if let Some(partner) = self.in_paired_turn {
            return partner;
        }
        match self.phase {
            Phase::SpecialBuild { builder } => builder,
            Phase::PairedTurn { partner } => partner,
            Phase::PlaceCamel => match self.after_camel {
                Phase::SpecialBuild { builder } => builder,
                Phase::PairedTurn { partner } => partner,
                _ => self.current,
            },
            _ => self.current,
        }
    }

    /// Seat placing at a given step of the snake setup order (0,1,..,n-1,n-1,..,0).
    #[inline]
    pub fn setup_player(&self, step: u8) -> u8 {
        let n = self.config.num_players;
        if step < n { step } else { 2 * n - 1 - step }
    }

    /// `true` during the second round of initial placement.
    #[inline]
    pub fn is_second_setup_round(&self) -> bool {
        self.setup_step >= self.config.num_players
    }

    /// The player expected to act next, or `None` once the game is over.
    /// During discards this is the lowest seat that still owes cards; the
    /// engine accepts discards from any owing player in any order.
    pub fn to_move(&self) -> Option<u8> {
        match self.phase {
            Phase::GameOver => None,
            Phase::Discard => self
                .pending_discard
                .iter()
                .position(|&d| d > 0)
                .map(|p| p as u8),
            Phase::Gold { .. } => self
                .pending_gold
                .iter()
                .position(|&d| d > 0)
                .map(|p| p as u8),
            _ => Some(self.active_player()),
        }
    }

    #[inline]
    pub fn buildings_of(&self, p: u8) -> BitSet {
        self.settlements[p as usize] | self.cities[p as usize]
    }

    #[inline]
    pub fn occupied_vertices(&self) -> BitSet {
        (0..self.num_players()).fold(BitSet::EMPTY, |m, p| m | self.settlements[p] | self.cities[p])
    }

    /// All vertices holding a knight of any seat.
    pub fn all_knights(&self) -> BitSet {
        let mut m = BitSet::EMPTY;
        for v in 0..self.topo().num_vertices {
            if self.knight_owner[v] != NONE {
                m.insert(v as u8);
            }
        }
        m
    }

    /// Vertices holding a knight of seat `p`.
    pub fn knights_of(&self, p: u8) -> BitSet {
        let mut m = BitSet::EMPTY;
        for v in 0..self.topo().num_vertices {
            if self.knight_owner[v] == p {
                m.insert(v as u8);
            }
        }
        m
    }

    /// Vertices holding an opponent's knight.
    #[inline]
    pub fn opponent_knights(&self, p: u8) -> BitSet {
        self.all_knights() & !self.knights_of(p)
    }

    /// Whether city vertex `v` holds a Metropolis.
    #[inline]
    pub fn is_metropolis(&self, v: u8) -> bool {
        self.metropolises.contains(&v)
    }

    /// Cities of `p` that are not crowned as a Metropolis.
    pub fn non_metropolis_cities(&self, p: u8) -> BitSet {
        let mut c = self.cities[p as usize];
        for &mv in &self.metropolises {
            if mv != NONE {
                c.remove(mv);
            }
        }
        c
    }

    /// Active knight strength contributed by seat `p`.
    pub fn active_knight_strength(&self, p: u8) -> u8 {
        vertices_in(self.knights_of(p) & self.knight_active)
            .map(|v| self.knight_level[v as usize])
            .sum()
    }

    /// Edges holding a road, a ship or a bridge.
    #[inline]
    pub fn occupied_edges(&self) -> BitSet {
        (0..self.num_players()).fold(BitSet::EMPTY, |m, p| {
            m | self.roads[p] | self.ships[p] | self.bridges[p]
        })
    }

    pub fn vertex_owner(&self, v: u8) -> Option<u8> {
        let bit = BitSet::bit(v);
        (0..self.num_players() as u8).find(|&p| !(self.buildings_of(p) & bit).is_empty())
    }

    /// Owner of the road on `e`.
    pub fn edge_owner(&self, e: u8) -> Option<u8> {
        let bit = BitSet::bit(e);
        (0..self.num_players() as u8).find(|&p| !(self.roads[p as usize] & bit).is_empty())
    }

    /// Owner of the ship on `e`.
    pub fn ship_owner(&self, e: u8) -> Option<u8> {
        let bit = BitSet::bit(e);
        (0..self.num_players() as u8).find(|&p| !(self.ships[p as usize] & bit).is_empty())
    }

    /// Owner of the stone bridge on `e`.
    pub fn bridge_owner(&self, e: u8) -> Option<u8> {
        let bit = BitSet::bit(e);
        (0..self.num_players() as u8).find(|&p| !(self.bridges[p as usize] & bit).is_empty())
    }

    /// Points visible to everyone (buildings, metropolises, defender honours and special cards).
    pub fn public_victory_points(&self, p: u8) -> u8 {
        let i = p as usize;
        let mut vp = self.settlements[i].count_ones() as u8 + 2 * self.cities[i].count_ones() as u8;
        if self.longest_road_owner == Some(p) {
            vp += 2;
        }
        if self.largest_army_owner == Some(p) {
            vp += 2;
        }
        let metros = self
            .metropolis_owners
            .iter()
            .filter(|&&o| o == Some(p))
            .count() as u8;
        vp += 2 * metros + self.players[i].defender_points;
        vp += self.board.layout.island_bonus() * self.players[i].islands.count_ones() as u8;
        if self.wealthiest_owner == Some(p) {
            vp += 2;
        }
        if self.config.scenario == Scenario::Caravans {
            if !self.camels.is_empty() {
                let topo = self.topo();
                for v in vertices_in(self.buildings_of(p)) {
                    if !(topo.vertex_edge_mask[v as usize] & self.camels).is_empty() {
                        vp += 1;
                    }
                }
            }
            if self.wealthiest_owner.is_some() && self.players[i].coins == 0 {
                vp = vp.saturating_sub(1);
            }
        }
        vp
    }

    /// Total points including hidden victory point cards.
    pub fn victory_points(&self, p: u8) -> u8 {
        self.public_victory_points(p)
            + self.players[p as usize].dev_cards[DevCard::VictoryPoint.index()]
    }

    /// Effective victory point target for `p` (`+1` when holding the Old Boot in Fishermen mode).
    #[inline]
    pub fn target_victory_points(&self, p: u8) -> u8 {
        self.config.victory_points
            + u8::from(
                self.config.scenario == Scenario::Fishermen && self.old_boot_owner == Some(p),
            )
    }

    /// Whether Fishermen fish-spending and Old Boot actions are allowed now.
    #[inline]
    pub(crate) fn can_use_fish(&self) -> bool {
        self.config.scenario.is_fishermen()
            && ((self.phase == Phase::Main && self.trade.is_none())
                || matches!(self.phase, Phase::PairedTurn { .. }))
    }

    /// Best maritime ratio for giving `r`: 2 (special harbor), 3 (generic) or 4.
    pub fn trade_ratio(&self, p: u8, r: crate::resources::Resource) -> u8 {
        let h = self.players[p as usize].harbors;
        if h & (1 << r as u8) != 0 {
            2
        } else if h & (1 << 5) != 0 {
            3
        } else {
            4
        }
    }

    /// Best maritime ratio for giving a commodity in Citadels mode: 2 with
    /// Trade Lv3 (Merchant Guild), otherwise 4.
    #[inline]
    pub fn commodity_trade_ratio(&self, p: u8) -> u8 {
        if self.players[p as usize].city_progress[CityTrack::Trade.index()] >= 3 {
            2
        } else {
            4
        }
    }

    /// Whether advancing `track` by one step claims or steals its Metropolis
    /// (requiring `p` to pick one of their non-metropolis cities).
    pub fn upgrade_claims_metropolis(&self, p: u8, track: CityTrack) -> bool {
        let idx = track.index();
        let next = self.players[p as usize].city_progress[idx] + 1;
        if self.non_metropolis_cities(p).is_empty() {
            return false;
        }
        match next {
            4 => self.metropolis_owners[idx].is_none(),
            5 => match self.metropolis_owners[idx] {
                None => true,
                Some(owner) => owner != p && self.players[owner as usize].city_progress[idx] < 5,
            },
            _ => false,
        }
    }

    /// Vertices where `p` may build a settlement on land, keeping the
    /// distance rule and avoiding knights; with `require_road`, at the end of
    /// their own road or ship. (Initial placements use [`setup_spots`](Self::setup_spots).)
    pub fn settlement_spots(&self, p: u8, require_road: bool) -> BitSet {
        let topo = self.topo();
        let occ = self.occupied_vertices();
        let free = self.board.land_vertices & !(occ | topo.neighbors_of(occ) | self.all_knights());
        if require_road {
            free & topo.endpoints_of(
                self.roads[p as usize] | self.ships[p as usize] | self.bridges[p as usize],
            )
        } else {
            free
        }
    }

    /// Vertices where `p` may recruit a new level-1 knight: empty land
    /// intersections touching `p`'s roads or ships.
    pub fn knight_recruit_spots(&self, p: u8) -> BitSet {
        let topo = self.topo();
        let taken = self.occupied_vertices() | self.all_knights();
        self.board.land_vertices
            & !taken
            & topo.endpoints_of(self.roads[p as usize] | self.ships[p as usize])
    }

    /// Vertices reachable along `p`'s connected route from `from` where an
    /// active knight at `from` may move (empty land vertices, or vertices
    /// holding a strictly weaker opponent knight).
    pub fn knight_move_targets(&self, p: u8, from: u8) -> BitSet {
        let my_lvl = self.knight_level[from as usize];
        if my_lvl == 0 {
            return BitSet::EMPTY;
        }
        let topo = self.topo();
        let own_b = self.buildings_of(p);
        let opp_b = self.occupied_vertices() & !own_b;
        let opp_k = self.opponent_knights(p);
        let blocked = opp_b | opp_k;
        let routes = self.roads[p as usize] | self.ships[p as usize];
        let mut seen = BitSet::bit(from);
        let mut queue = [0u8; MAX_VERTICES];
        let mut head = 0usize;
        let mut tail = 0usize;
        queue[tail] = from;
        tail += 1;
        let mut targets = BitSet::EMPTY;
        while head < tail {
            let v = queue[head];
            head += 1;
            for k in 0..3 {
                let e = topo.vertex_edges[v as usize][k];
                if e == NONE || !routes.contains(e) {
                    continue;
                }
                let u = topo.vertex_vertices[v as usize][k];
                if seen.contains(u) {
                    continue;
                }
                seen.insert(u);
                if self.board.land_vertices.contains(u) {
                    if !self.occupied_vertices().contains(u) && self.knight_owner[u as usize] == NONE
                    {
                        targets.insert(u);
                    } else if opp_k.contains(u) && self.knight_level[u as usize] < my_lvl {
                        targets.insert(u);
                    }
                }
                if !blocked.contains(u) {
                    queue[tail] = u;
                    tail += 1;
                }
            }
        }
        targets
    }

    /// Lowest-numbered empty land vertex connected to `at` along `owner`'s
    /// routes (excluding `exclude`), for a displaced knight's retreat.
    pub fn knight_retreat_spot(&self, owner: u8, at: u8, exclude: u8) -> Option<u8> {
        let topo = self.topo();
        let own_b = self.buildings_of(owner);
        let blocked = (self.occupied_vertices() & !own_b) | self.opponent_knights(owner);
        let routes = self.roads[owner as usize] | self.ships[owner as usize];
        let mut seen = BitSet::bit(at);
        let mut queue = [0u8; MAX_VERTICES];
        let (mut head, mut tail) = (0usize, 1usize);
        queue[0] = at;
        let mut out = BitSet::EMPTY;
        while head < tail {
            let v = queue[head];
            head += 1;
            for k in 0..3 {
                let e = topo.vertex_edges[v as usize][k];
                if e == NONE || !routes.contains(e) {
                    continue;
                }
                let u = topo.vertex_vertices[v as usize][k];
                if seen.contains(u) {
                    continue;
                }
                seen.insert(u);
                if u != exclude
                    && self.board.land_vertices.contains(u)
                    && !self.occupied_vertices().contains(u)
                    && self.knight_owner[u as usize] == NONE
                {
                    out.insert(u);
                }
                if !blocked.contains(u) {
                    queue[tail] = u;
                    tail += 1;
                }
            }
        }
        vertices_in(out).next()
    }

    /// Vertices open for an initial settlement (on seafaring maps only the
    /// home island).
    #[inline]
    pub fn setup_spots(&self) -> BitSet {
        self.settlement_spots(0, false) & self.board.setup_vertices
    }

    /// Edges where `p` may place a road connected to their network. A road
    /// cannot continue through an intersection occupied by an opponent or
    /// opponent knight, directly from a ship, or onto a golden river edge.
    pub fn road_spots(&self, p: u8) -> BitSet {
        let topo = self.topo();
        let own = self.buildings_of(p);
        let opponents = (self.occupied_vertices() & !own) | self.opponent_knights(p);
        let reach = own
            | (topo.endpoints_of(self.roads[p as usize] | self.bridges[p as usize]) & !opponents);
        topo.edges_touching(reach)
            & self.board.land_edges
            & !self.board.river_edges
            & !self.occupied_edges()
    }

    /// Caravans: golden river edges where `p` may build a stone bridge.
    pub fn bridge_spots(&self, p: u8) -> BitSet {
        if self.config.scenario != Scenario::Caravans {
            return BitSet::EMPTY;
        }
        let topo = self.topo();
        let own = self.buildings_of(p);
        let opponents = (self.occupied_vertices() & !own) | self.opponent_knights(p);
        let reach = own
            | (topo.endpoints_of(self.roads[p as usize] | self.bridges[p as usize]) & !opponents);
        topo.edges_touching(reach) & self.board.river_edges & !self.occupied_edges()
    }

    /// Caravans: land edges where the next camel caravan segment may be placed.
    pub fn camel_spots(&self) -> BitSet {
        if self.config.scenario != Scenario::Caravans
            || self.camels.count_ones() >= MAX_CAMELS as u32
        {
            return BitSet::EMPTY;
        }
        let topo = self.topo();
        let oasis = if self.board.desert != NONE {
            topo.tile_vertex_mask[self.board.desert as usize]
        } else {
            BitSet::EMPTY
        };
        let starts = oasis | topo.endpoints_of(self.camels);
        topo.edges_touching(starts) & self.board.land_edges & !self.camels
    }

    /// Edges of the tile the pirate is on (no ship may be built or moved
    /// there).
    #[inline]
    pub fn pirate_edges(&self) -> BitSet {
        if self.pirate == NONE {
            BitSet::EMPTY
        } else {
            self.topo().tile_edge_mask(self.pirate)
        }
    }

    /// Edges where `p` may place a ship, if `ships` were their fleet: along
    /// the sea, away from the pirate, next to their own building or the open
    /// end of one of their ships. Ships do not continue through an
    /// intersection occupied by an opponent or opponent knight, nor directly
    /// from a road.
    pub fn ship_spots_with(&self, p: u8, ships: BitSet) -> BitSet {
        if !self.board.layout.has_ships() {
            return BitSet::EMPTY;
        }
        let topo = self.topo();
        let own = self.buildings_of(p);
        let opponents = (self.occupied_vertices() & !own) | self.opponent_knights(p);
        let reach = own | (topo.endpoints_of(ships) & !opponents);
        let taken = self.occupied_edges() & !(self.ships[p as usize] & !ships);
        topo.edges_touching(reach) & self.board.sea_edges & !self.pirate_edges() & !taken
    }

    /// Edges where `p` may build a ship now.
    #[inline]
    pub fn ship_spots(&self, p: u8) -> BitSet {
        self.ship_spots_with(p, self.ships[p as usize])
    }

    /// `p`'s ships that may move this turn: not built this turn, away from
    /// the pirate, and at the open end of a line (one end touches neither
    /// their building, their knight, nor another of their ships).
    pub fn movable_ships(&self, p: u8) -> BitSet {
        let ships = self.ships[p as usize];
        if ships.is_empty() {
            return BitSet::EMPTY;
        }
        let topo = self.topo();
        let own = self.buildings_of(p) | self.knights_of(p);
        let mut out = BitSet::EMPTY;
        for e in edges_in(ships & !self.new_ships & !self.pirate_edges()) {
            let others = ships & !BitSet::bit(e);
            let open = topo.edge_vertices[e as usize].iter().any(|&v| {
                !own.contains(v) && (topo.vertex_edge_mask[v as usize] & others).is_empty()
            });
            if open {
                out.insert(e);
            }
        }
        out
    }

    /// Where the ship on `from` may move to.
    pub fn ship_moves(&self, p: u8, from: u8) -> BitSet {
        let rest = self.ships[p as usize] & !BitSet::bit(from);
        self.ship_spots_with(p, rest) & !BitSet::bit(from)
    }

    /// Whether moving the robber/pirate to `tile` touches an opponent with
    /// 2 or fewer public victory points.
    pub fn tile_has_novice(&self, tile: u8, thief: u8) -> bool {
        let topo = self.topo();
        let sea = self.board.is_sea(tile);
        let tv = topo.tile_vertex_mask[tile as usize];
        let te = if sea { topo.tile_edge_mask(tile) } else { BitSet::EMPTY };
        (0..self.num_players() as u8).any(|q| {
            if q == thief || self.public_victory_points(q) > 2 {
                return false;
            }
            if sea {
                !(self.ships[q as usize] & te).is_empty()
            } else {
                !(self.buildings_of(q) & tv).is_empty()
            }
        })
    }

    /// Whether `tile` is disallowed by the Friendly Robber house rule (only
    /// enforced when at least one non-novice destination exists).
    pub fn friendly_robber_blocks(&self, tile: u8, thief: u8) -> bool {
        if !self.config.friendly_robber || !self.tile_has_novice(tile, thief) {
            return false;
        }
        (0..self.topo().num_tiles as u8).any(|t| {
            t != self.robber
                && t != self.pirate
                && self.board.terrain[t as usize] != Terrain::Fog
                && !(self.board.is_sea(t) && self.pirate == NONE)
                && !self.tile_has_novice(t, thief)
        })
    }

    /// Bitmask of players `thief` may rob after moving the robber to a land
    /// `tile` (opponents with a building on it) or the pirate to a sea tile
    /// (opponents with a ship along it), if they hold at least one card.
    pub fn robber_victims(&self, tile: u8, thief: u8) -> u8 {
        let topo = self.topo();
        let sea = self.board.is_sea(tile);
        let tv = topo.tile_vertex_mask[tile as usize];
        let te = if sea { topo.tile_edge_mask(tile) } else { BitSet::EMPTY };
        (0..self.num_players() as u8)
            .filter(|&q| {
                let near = if sea {
                    !(self.ships[q as usize] & te).is_empty()
                } else {
                    !(self.buildings_of(q) & tv).is_empty()
                };
                q != thief && near && self.players[q as usize].total_cards() > 0
            })
            .fold(0, |m, q| m | 1 << q)
    }

    /// Length of `p`'s longest continuous trade route: roads, and on
    /// seafaring maps ships. Opponent buildings and opponent knights split
    /// routes; a route turns from road to ship only at `p`'s own building.
    pub fn compute_longest_road(&self, p: u8) -> u8 {
        let (roads, ships) = (
            self.roads[p as usize] | self.bridges[p as usize],
            self.ships[p as usize],
        );
        if (roads | ships).is_empty() {
            return 0;
        }
        let topo = self.topo();
        let own = self.buildings_of(p);
        let blocked = (self.occupied_vertices() & !own) | self.opponent_knights(p);
        let camels = if self.config.scenario == Scenario::Caravans {
            self.camels
        } else {
            BitSet::EMPTY
        };
        let route = Route {
            topo,
            roads,
            ships,
            camels,
            own,
            blocked,
        };
        vertices_in(topo.endpoints_of(roads | ships))
            .map(|v| route.longest_from(v, None, BitSet::EMPTY))
            .max()
            .unwrap_or(0)
    }

    /// Verifies conservation laws and structural invariants. Intended for
    /// tests, fuzzing and debug tooling.
    pub fn check_invariants(&self) -> Result<(), String> {
        let n = self.num_players();
        let layout = self.board.layout;
        let topo = self.topo();
        for r in 0..5 {
            let total: u32 = self.bank.0[r] as u32
                + (0..n)
                    .map(|p| self.players[p].hand.0[r] as u32)
                    .sum::<u32>();
            if total != layout.bank_per_resource() as u32 {
                return Err(format!("resource {r} not conserved: {total}"));
            }
        }
        let expected_comm = if self.config.scenario.is_citadels() {
            layout.bank_per_commodity() as u32
        } else {
            0
        };
        for c in 0..3 {
            let total: u32 = self.commodity_bank.0[c] as u32
                + (0..n)
                    .map(|p| self.players[p].commodities.0[c] as u32)
                    .sum::<u32>();
            if total != expected_comm {
                return Err(format!("commodity {c} not conserved: {total}"));
            }
        }
        for card in DevCard::ALL {
            let c = card.index();
            let in_deck = self.dev_deck[..self.dev_deck_len as usize]
                .iter()
                .filter(|&&d| d == card)
                .count();
            let held: usize = (0..n)
                .map(|p| {
                    let ps = &self.players[p];
                    (ps.dev_cards[c] + ps.new_dev_cards[c] + ps.played_dev_cards[c]) as usize
                })
                .sum();
            if in_deck + held != card.count_in_deck(layout) as usize {
                return Err(format!("dev card {card:?} not conserved"));
            }
        }
        let mut seen_v = BitSet::EMPTY;
        let mut seen_e = BitSet::EMPTY;
        let all_k = self.all_knights();
        for p in 0..n {
            let ps = &self.players[p];
            if !(self.settlements[p] & self.cities[p]).is_empty() {
                return Err(format!(
                    "player {p} has a settlement and a city on one vertex"
                ));
            }
            let b = self.settlements[p] | self.cities[p];
            let pieces = self.roads[p] | self.ships[p] | self.bridges[p];
            if !(b & seen_v).is_empty()
                || !(pieces & seen_e).is_empty()
                || !(self.roads[p] & self.ships[p]).is_empty()
                || !(self.roads[p] & self.bridges[p]).is_empty()
                || !(self.ships[p] & self.bridges[p]).is_empty()
                || !(b & all_k).is_empty()
            {
                return Err("two pieces share a spot".into());
            }
            seen_v |= b;
            seen_e |= pieces;
            if self.settlements[p].count_ones() as u8 + ps.settlements_left != MAX_SETTLEMENTS
                || self.cities[p].count_ones() as u8 + ps.cities_left != MAX_CITIES
                || self.roads[p].count_ones() as u8 + ps.roads_left != MAX_ROADS
                || self.ships[p].count_ones() as u8 + ps.ships_left != MAX_SHIPS
                || self.bridges[p].count_ones() as u8 + ps.bridges_left != MAX_BRIDGES
            {
                return Err(format!("player {p} piece counts inconsistent"));
            }
            for lvl in 1..=3u8 {
                let on_board = (0..topo.num_vertices)
                    .filter(|&v| self.knight_owner[v] == p as u8 && self.knight_level[v] == lvl)
                    .count() as u8;
                if on_board + ps.knights_left[(lvl - 1) as usize] != MAX_KNIGHTS_PER_LEVEL {
                    return Err(format!("player {p} knight level {lvl} count inconsistent"));
                }
            }
            if !(b & !self.board.land_vertices).is_empty()
                || !(self.roads[p] & !self.board.land_edges).is_empty()
                || !(self.roads[p] & self.board.river_edges).is_empty()
                || !(self.ships[p] & !self.board.sea_edges).is_empty()
                || (!self.ships[p].is_empty() && !layout.has_ships())
                || !(self.bridges[p] & !self.board.river_edges).is_empty()
                || (!self.bridges[p].is_empty() && !self.config.scenario.is_caravans())
            {
                return Err(format!("player {p} has a piece on the wrong ground"));
            }
            // Island bonuses: earned on islands they built on but did not
            // start on.
            let built: u8 = vertices_in(b).fold(0, |m, v| {
                m | (1u8 << self.board.vertex_island[v as usize]) & !1
            });
            if ps.islands & !built != 0
                || ps.islands & ps.home_islands != 0
                || (layout.island_bonus() == 0 && ps.islands != 0)
            {
                return Err(format!("player {p} has an unearned island bonus"));
            }
            if !matches!(self.phase, Phase::Gold { .. }) && self.pending_gold[p] != 0 {
                return Err("pending gold outside the gold phase".into());
            }
            if ps.longest_road != self.compute_longest_road(p as u8) {
                return Err(format!("player {p} cached longest road is stale"));
            }
            if self.phase != Phase::Discard && self.pending_discard[p] != 0 {
                return Err("pending discard outside discard phase".into());
            }
            if ps.fish > MAX_FISH_HAND {
                return Err(format!("player {p} fish {} exceeds max", ps.fish));
            }
            if !self.config.scenario.is_fishermen() && ps.fish != 0 {
                return Err(format!("player {p} holds fish outside fishermen scenario"));
            }
            if ps.coins > MAX_COINS {
                return Err(format!("player {p} coins {} exceeds max", ps.coins));
            }
            if !self.config.scenario.is_caravans() && ps.coins != 0 {
                return Err(format!("player {p} holds coins outside caravans scenario"));
            }
        }
        if self.config.scenario.is_caravans() {
            if !(self.board.river_edges & !self.board.land_edges).is_empty() {
                return Err("river edges off land".into());
            }
            if !(self.camels & !self.board.land_edges).is_empty() {
                return Err("camels off land".into());
            }
            if self.camels.count_ones() > MAX_CAMELS as u32 {
                return Err("too many camels".into());
            }
        } else {
            if !self.board.river_edges.is_empty() {
                return Err("river edges on non-caravans board".into());
            }
            if !self.camels.is_empty() {
                return Err("camels on non-caravans board".into());
            }
        }
        if let Some(h) = self.wealthiest_owner {
            if !self.config.scenario.is_caravans() || h as usize >= n {
                return Err("invalid wealthiest_owner".into());
            }
            let c = self.players[h as usize].coins;
            if c < WEALTHIEST_MIN_COINS || (0..n).any(|q| self.players[q].coins > c) {
                return Err("wealthiest owner is not the wealthiest".into());
            }
        } else if self.config.scenario.is_caravans() {
            let max_c = (0..n).map(|q| self.players[q].coins).max().unwrap_or(0);
            if max_c >= WEALTHIEST_MIN_COINS
                && (0..n).filter(|&q| self.players[q].coins == max_c).count() == 1
            {
                return Err("unassigned wealthiest_owner".into());
            }
        }
        if let Some(owner) = self.old_boot_owner
            && (!self.config.scenario.is_fishermen() || owner as usize >= n)
        {
            return Err("invalid old_boot_owner".into());
        }
        if self.config.scenario.is_fishermen() {
            if self.board.fishing_ground_count as usize > crate::board::MAX_FISHING_GROUNDS {
                return Err("fishing_ground_count exceeds max".into());
            }
            for fg in self.board.active_fishing_grounds() {
                if fg.edge as usize >= topo.num_edges
                    || !(2..=12).contains(&fg.number)
                    || fg.number == 7
                {
                    return Err("invalid fishing ground".into());
                }
            }
        } else if self.board.fishing_ground_count != 0 {
            return Err("fishing grounds on non-fishermen board".into());
        }
        if self.config.event_deck {
            if self.dice_deck_len as usize > DICE_DECK_SIZE {
                return Err("dice_deck_len exceeds 36".into());
            }
            for &(d1, d2) in &self.dice_deck[..self.dice_deck_len as usize] {
                if !(1..=6).contains(&d1) || !(1..=6).contains(&d2) {
                    return Err("invalid die face in dice_deck".into());
                }
            }
        } else if self.dice_deck_len != 0 {
            return Err("dice_deck_len nonzero without event_deck".into());
        }
        for t in 0..3 {
            match (self.metropolis_owners[t], self.metropolises[t]) {
                (None, NONE) => {}
                (Some(owner), mv)
                    if (owner as usize) < n && self.cities[owner as usize].contains(mv) => {}
                _ => return Err(format!("metropolis {t} inconsistent")),
            }
        }
        for v in vertices_in(seen_v) {
            if !(topo.vertex_adj_mask[v as usize] & seen_v).is_empty() {
                return Err(format!("distance rule violated at vertex {v}"));
            }
        }
        if let Some(h) = self.longest_road_owner {
            let len = self.players[h as usize].longest_road;
            if len < LONGEST_ROAD_MIN || (0..n).any(|q| self.players[q].longest_road > len) {
                return Err("longest road owner is not the longest".into());
            }
        }
        if let Some(h) = self.largest_army_owner {
            let k = self.players[h as usize].knights_played();
            if k < LARGEST_ARMY_MIN || (0..n).any(|q| self.players[q].knights_played() > k) {
                return Err("largest army owner is not the largest".into());
            }
        }
        if self.robber as usize >= topo.num_tiles
            || !self.board.terrain[self.robber as usize].is_land()
        {
            return Err("robber off land".into());
        }
        if layout.has_ships() != (self.pirate != NONE)
            || (self.pirate != NONE && !self.board.is_sea(self.pirate))
        {
            return Err("pirate off the sea".into());
        }
        let mover = self.active_player() as usize;
        if mover < n && !(self.new_ships & !self.ships[mover]).is_empty() {
            return Err("new ships that are not the acting player's".into());
        }
        if !(seen_v & !topo.all_vertices).is_empty()
            || !(seen_e & !topo.all_edges).is_empty()
            || !(all_k & !self.board.land_vertices).is_empty()
        {
            return Err("piece off board".into());
        }
        if layout.has_fog() {
            let fog_tiles = self
                .board
                .tiles()
                .iter()
                .filter(|&&t| t == Terrain::Fog)
                .count();
            if fog_tiles != self.fog_terrain_len as usize {
                return Err(format!(
                    "fog tile count mismatch: {fog_tiles} on board vs {} in stack",
                    self.fog_terrain_len
                ));
            }
            let fog_producing = self.fog_terrain[..self.fog_terrain_len as usize]
                .iter()
                .filter(|t| t.produces())
                .count();
            if fog_producing != self.fog_numbers_len as usize {
                return Err(format!(
                    "fog number count mismatch: {fog_producing} producing vs {} tokens",
                    self.fog_numbers_len
                ));
            }
            if self.config.custom_map.is_none() {
                let mut counts = [0u8; Terrain::COUNT];
                for &t in self.board.tiles() {
                    if t != Terrain::Fog {
                        counts[t as usize] += 1;
                    }
                }
                for &t in &self.fog_terrain[..self.fog_terrain_len as usize] {
                    counts[t as usize] += 1;
                }
                if counts != layout.full_terrain_counts() {
                    return Err("fog terrain not conserved".into());
                }
                let mut num_counts = [0u8; 13];
                for &num in &self.board.numbers[..topo.num_tiles] {
                    if num != 0 {
                        num_counts[num as usize] += 1;
                    }
                }
                for &num in &self.fog_numbers[..self.fog_numbers_len as usize] {
                    num_counts[num as usize] += 1;
                }
                let mut expected_nums = [0u8; 13];
                for &num in layout
                    .number_pool()
                    .iter()
                    .chain(layout.fog_number_pool().iter())
                {
                    expected_nums[num as usize] += 1;
                }
                if num_counts != expected_nums {
                    return Err("fog number tokens not conserved".into());
                }
            }
            let route_vertices = topo.endpoints_of(seen_e);
            for t in 0..topo.num_tiles {
                if self.board.terrain[t] == Terrain::Fog
                    && !(route_vertices & topo.tile_vertex_mask[t]).is_empty()
                {
                    return Err(format!("route touches unrevealed fog tile {t}"));
                }
            }
        } else if self.fog_terrain_len != 0 || self.fog_numbers_len != 0 {
            return Err("non-fog board has fog stacks".into());
        }
        if let Phase::SpecialBuild { builder } = self.phase
            && (!layout.has_special_build()
                || self.config.paired_turns
                || builder == self.current
                || builder as usize >= n)
        {
            return Err("invalid special building phase".into());
        }
        if let Phase::PairedTurn { partner } = self.phase
            && (!layout.has_special_build()
                || !self.config.paired_turns
                || partner == self.current
                || partner as usize >= n)
        {
            return Err("invalid paired turn phase".into());
        }
        if (self.phase == Phase::GameOver) != self.winner.is_some() {
            return Err("winner and phase disagree".into());
        }
        Ok(())
    }
}

/// One player's pieces for the longest-route search.
struct Route<'a> {
    topo: &'a Topology,
    roads: BitSet,
    ships: BitSet,
    camels: BitSet,
    /// The player's own buildings, where roads and ships join.
    own: BitSet,
    /// Intersections occupied by opponents or opponent knights.
    blocked: BitSet,
}

impl Route<'_> {
    /// Longest trail (no edge reused) starting at `v`, having arrived by a
    /// ship (`Some(true)`), a road (`Some(false)`) or not at all, never
    /// passing through a blocked vertex.
    fn longest_from(&self, v: u8, arrived_by_ship: Option<bool>, used: BitSet) -> u8 {
        let topo = self.topo;
        let mut best = 0;
        let links = &topo.vertex_edges[v as usize];
        for k in 0..3 {
            let e = links[k];
            if e == NONE {
                break;
            }
            let bit = BitSet::bit(e);
            if ((self.roads | self.ships) & bit).is_empty() || !(used & bit).is_empty() {
                continue;
            }
            let ship = self.ships.contains(e);
            if arrived_by_ship.is_some_and(|s| s != ship) && !self.own.contains(v) {
                continue;
            }
            let step = if !self.camels.is_empty() && self.camels.contains(e) {
                2
            } else {
                1
            };
            let u = topo.vertex_vertices[v as usize][k];
            let len = if self.blocked.contains(u) {
                step
            } else {
                step + self.longest_from(u, Some(ship), used | bit)
            };
            best = best.max(len);
        }
        best
    }
}
