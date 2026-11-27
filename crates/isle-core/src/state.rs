//! Game state and read-only queries.

use crate::board::{Board, FOG_HOME_NUMBER_POOL, FOG_NUMBER_POOL, FOG_TERRAIN_POOL};
use crate::resources::{ResourceSet, Terrain};
use crate::rng::Rng;
use crate::topology::*;

/// Maximum number of seats (the extended board seats six).
pub const MAX_PLAYERS: usize = 6;
/// Size of the largest development deck.
pub const MAX_DEV_DECK: usize = 34;
/// Unexplored tiles on a fog map.
pub const MAX_FOG_TILES: usize = 13;
/// Number tokens in the hidden fog stack.
pub const MAX_FOG_NUMBERS: usize = 9;
pub const MAX_ROADS: u8 = 15;
/// Ships per player on seafaring maps.
pub const MAX_SHIPS: u8 = 15;
pub const MAX_SETTLEMENTS: u8 = 5;
pub const MAX_CITIES: u8 = 4;
/// Players holding more cards than this discard half when a 7 is rolled.
pub const DISCARD_LIMIT: u32 = 7;
pub const LONGEST_ROAD_MIN: u8 = 5;
pub const LARGEST_ARMY_MIN: u8 = 3;

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
        match (self, layout) {
            (DevCard::Knight, BoardLayout::Extended) => 20,
            (DevCard::Knight, _) => 14,
            (DevCard::VictoryPoint, _) => 5,
            (_, BoardLayout::Extended) => 3,
            (_, _) => 2,
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
        match self {
            BoardLayout::Extended => 24,
            _ => 19,
        }
    }

    /// Development cards in the deck.
    pub const fn dev_deck_size(self) -> usize {
        match self {
            BoardLayout::Extended => 34,
            _ => 25,
        }
    }

    /// After each turn, the other players may build in turn order.
    pub const fn has_special_build(self) -> bool {
        matches!(self, BoardLayout::Extended)
    }

    /// Seats the board is designed for.
    pub const fn max_players(self) -> u8 {
        match self {
            BoardLayout::Extended => 6,
            _ => 4,
        }
    }

    /// Points for a player's first settlement on each island other than
    /// the ones they started on.
    pub const fn island_bonus(self) -> u8 {
        match self {
            BoardLayout::Archipelago | BoardLayout::FogIslands => 2,
            _ => 0,
        }
    }
}

/// What is played: the classic island (sized by the number of players) or
/// a seafaring map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Scenario {
    #[default]
    Classic,
    /// Seafaring: the archipelago, 2–4 players.
    Archipelago,
    /// Seafaring: unexplored fog tiles revealed by ships, 2–4 players.
    FogIslands,
}

impl Scenario {
    pub const fn victory_points(self) -> u8 {
        match self {
            Scenario::Classic => 10,
            Scenario::Archipelago | Scenario::FogIslands => 12,
        }
    }

    /// Seats this scenario can be played with.
    pub const fn max_players(self) -> u8 {
        match self {
            Scenario::Classic => MAX_PLAYERS as u8,
            Scenario::Archipelago | Scenario::FogIslands => 4,
        }
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

    /// Board used by this configuration: the classic island grows to the
    /// extended one for 5–6 seats.
    #[inline]
    pub fn layout(&self) -> BoardLayout {
        match self.scenario {
            Scenario::Classic => BoardLayout::for_players(self.num_players),
            Scenario::Archipelago => BoardLayout::Archipelago,
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
    /// Islands this player started on (bit `i` = island `i`).
    pub home_islands: u8,
    /// Islands that earned this player the settling bonus.
    pub islands: u8,
    /// Cached length of this player's longest road.
    pub longest_road: u8,
    /// Harbor access bits (`HarborKind::bit`).
    pub harbors: u8,
}

impl PlayerState {
    const fn new() -> Self {
        PlayerState {
            hand: ResourceSet::EMPTY,
            dev_cards: [0; 5],
            new_dev_cards: [0; 5],
            played_dev_cards: [0; 5],
            roads_left: MAX_ROADS,
            ships_left: MAX_SHIPS,
            settlements_left: MAX_SETTLEMENTS,
            cities_left: MAX_CITIES,
            home_islands: 0,
            islands: 0,
            longest_road: 0,
            harbors: 0,
        }
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
    pub settlements: [u128; MAX_PLAYERS],
    pub cities: [u128; MAX_PLAYERS],
    pub roads: [u128; MAX_PLAYERS],
    /// Seafaring: ships, on edges along the sea.
    pub ships: [u128; MAX_PLAYERS],
    pub bank: ResourceSet,
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
    pub current: u8,
    /// Index into the snake order of initial placements.
    pub setup_step: u8,
    /// Completed turns since the end of setup.
    pub turn: u32,
    /// Dice rolled this turn, `None` before rolling.
    pub dice: Option<(u8, u8)>,
    pub dev_card_played: bool,
    /// The current player moved a ship this turn.
    pub ship_moved: bool,
    /// Ships the current player built this turn (they may not move yet).
    pub new_ships: u128,
    pub pending_discard: [u8; MAX_PLAYERS],
    /// Gold resources each player still has to pick.
    pub pending_gold: [u8; MAX_PLAYERS],
    pub trade: Option<TradeOffer>,
    pub longest_road_owner: Option<u8>,
    pub largest_army_owner: Option<u8>,
    pub winner: Option<u8>,
    pub rng: Rng,
}

impl GameState {
    /// New game with a random board derived from `seed`.
    pub fn new(config: GameConfig, seed: u64) -> Result<GameState, ConfigError> {
        let mut rng = Rng::new(seed);
        let board = Board::generate(&mut rng, config.layout(), config.balanced_board);
        Self::with_board(config, board, rng.next_u64())
    }

    /// New game on a given board.
    pub fn with_board(
        config: GameConfig,
        board: Board,
        seed: u64,
    ) -> Result<GameState, ConfigError> {
        if !(2..=config.scenario.max_players()).contains(&config.num_players) {
            return Err(ConfigError::PlayerCount);
        }
        if config.victory_points < 3 {
            return Err(ConfigError::VictoryPoints);
        }
        let layout = config.layout();
        if board.layout != layout {
            return Err(ConfigError::BoardLayout);
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
            fog_terrain.copy_from_slice(&FOG_TERRAIN_POOL);
            fog_terrain_len = MAX_FOG_TILES as u8;
            rng.shuffle(&mut fog_terrain);
            fog_numbers.copy_from_slice(&FOG_NUMBER_POOL);
            fog_numbers_len = MAX_FOG_NUMBERS as u8;
            rng.shuffle(&mut fog_numbers);
        }
        Ok(GameState {
            config,
            board,
            players: [PlayerState::new(); MAX_PLAYERS],
            settlements: [0; MAX_PLAYERS],
            cities: [0; MAX_PLAYERS],
            roads: [0; MAX_PLAYERS],
            ships: [0; MAX_PLAYERS],
            bank: ResourceSet([layout.bank_per_resource(); 5]),
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
            current: 0,
            setup_step: 0,
            turn: 0,
            dice: None,
            dev_card_played: false,
            ship_moved: false,
            new_ships: 0,
            pending_discard: [0; MAX_PLAYERS],
            pending_gold: [0; MAX_PLAYERS],
            trade: None,
            longest_road_owner: None,
            largest_army_owner: None,
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
    /// responses: the builder during a special building phase, otherwise
    /// the player whose turn it is.
    #[inline]
    pub fn active_player(&self) -> u8 {
        match self.phase {
            Phase::SpecialBuild { builder } => builder,
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
    pub fn buildings_of(&self, p: u8) -> u128 {
        self.settlements[p as usize] | self.cities[p as usize]
    }

    #[inline]
    pub fn occupied_vertices(&self) -> u128 {
        (0..self.num_players()).fold(0, |m, p| m | self.settlements[p] | self.cities[p])
    }

    /// Edges holding a road or a ship.
    #[inline]
    pub fn occupied_edges(&self) -> u128 {
        (0..self.num_players()).fold(0, |m, p| m | self.roads[p] | self.ships[p])
    }

    pub fn vertex_owner(&self, v: u8) -> Option<u8> {
        let bit = 1u128 << v;
        (0..self.num_players() as u8).find(|&p| self.buildings_of(p) & bit != 0)
    }

    /// Owner of the road on `e`.
    pub fn edge_owner(&self, e: u8) -> Option<u8> {
        let bit = 1u128 << e;
        (0..self.num_players() as u8).find(|&p| self.roads[p as usize] & bit != 0)
    }

    /// Owner of the ship on `e`.
    pub fn ship_owner(&self, e: u8) -> Option<u8> {
        let bit = 1u128 << e;
        (0..self.num_players() as u8).find(|&p| self.ships[p as usize] & bit != 0)
    }

    /// Points visible to everyone (buildings and special cards).
    pub fn public_victory_points(&self, p: u8) -> u8 {
        let i = p as usize;
        let mut vp = self.settlements[i].count_ones() as u8 + 2 * self.cities[i].count_ones() as u8;
        if self.longest_road_owner == Some(p) {
            vp += 2;
        }
        if self.largest_army_owner == Some(p) {
            vp += 2;
        }
        vp + self.board.layout.island_bonus() * self.players[i].islands.count_ones() as u8
    }

    /// Total points including hidden victory point cards.
    pub fn victory_points(&self, p: u8) -> u8 {
        self.public_victory_points(p)
            + self.players[p as usize].dev_cards[DevCard::VictoryPoint.index()]
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

    /// Vertices where `p` may build a settlement on land, keeping the
    /// distance rule; with `require_road`, at the end of their own road or
    /// ship. (Initial placements use [`setup_spots`](Self::setup_spots).)
    pub fn settlement_spots(&self, p: u8, require_road: bool) -> u128 {
        let topo = self.topo();
        let occ = self.occupied_vertices();
        let free = self.board.land_vertices & !(occ | topo.neighbors_of(occ));
        if require_road {
            free & topo.endpoints_of(self.roads[p as usize] | self.ships[p as usize])
        } else {
            free
        }
    }

    /// Vertices open for an initial settlement (on seafaring maps only the
    /// home island).
    #[inline]
    pub fn setup_spots(&self) -> u128 {
        self.settlement_spots(0, false) & self.board.setup_vertices
    }

    /// Edges where `p` may place a road connected to their network. A road
    /// cannot continue through an intersection occupied by an opponent, nor
    /// directly from a ship.
    pub fn road_spots(&self, p: u8) -> u128 {
        let topo = self.topo();
        let own = self.buildings_of(p);
        let opponents = self.occupied_vertices() & !own;
        let reach = own | (topo.endpoints_of(self.roads[p as usize]) & !opponents);
        topo.edges_touching(reach) & self.board.land_edges & !self.occupied_edges()
    }

    /// Edges of the tile the pirate is on (no ship may be built or moved
    /// there).
    #[inline]
    pub fn pirate_edges(&self) -> u128 {
        if self.pirate == NONE {
            0
        } else {
            self.topo().tile_edge_mask(self.pirate)
        }
    }

    /// Edges where `p` may place a ship, if `ships` were their fleet: along
    /// the sea, away from the pirate, next to their own building or the open
    /// end of one of their ships. Ships do not continue through an
    /// intersection occupied by an opponent, nor directly from a road.
    pub fn ship_spots_with(&self, p: u8, ships: u128) -> u128 {
        if !self.board.layout.has_ships() {
            return 0;
        }
        let topo = self.topo();
        let own = self.buildings_of(p);
        let opponents = self.occupied_vertices() & !own;
        let reach = own | (topo.endpoints_of(ships) & !opponents);
        let taken = self.occupied_edges() & !(self.ships[p as usize] & !ships);
        topo.edges_touching(reach) & self.board.sea_edges & !self.pirate_edges() & !taken
    }

    /// Edges where `p` may build a ship now.
    #[inline]
    pub fn ship_spots(&self, p: u8) -> u128 {
        self.ship_spots_with(p, self.ships[p as usize])
    }

    /// `p`'s ships that may move this turn: not built this turn, away from
    /// the pirate, and at the open end of a line (one end touches neither
    /// their building nor another of their ships).
    pub fn movable_ships(&self, p: u8) -> u128 {
        let ships = self.ships[p as usize];
        if ships == 0 {
            return 0;
        }
        let topo = self.topo();
        let own = self.buildings_of(p);
        let mut out = 0;
        for e in edges_in(ships & !self.new_ships & !self.pirate_edges()) {
            let others = ships & !(1u128 << e);
            let open = topo.edge_vertices[e as usize].iter().any(|&v| {
                own & (1u128 << v) == 0 && topo.vertex_edge_mask[v as usize] & others == 0
            });
            if open {
                out |= 1u128 << e;
            }
        }
        out
    }

    /// Where the ship on `from` may move to.
    pub fn ship_moves(&self, p: u8, from: u8) -> u128 {
        let rest = self.ships[p as usize] & !(1u128 << from);
        self.ship_spots_with(p, rest) & !(1u128 << from)
    }

    /// Bitmask of players `thief` may rob after moving the robber to a land
    /// `tile` (opponents with a building on it) or the pirate to a sea tile
    /// (opponents with a ship along it), if they hold at least one card.
    pub fn robber_victims(&self, tile: u8, thief: u8) -> u8 {
        let topo = self.topo();
        let sea = self.board.is_sea(tile);
        let tv = topo.tile_vertex_mask[tile as usize];
        let te = if sea { topo.tile_edge_mask(tile) } else { 0 };
        (0..self.num_players() as u8)
            .filter(|&q| {
                let near = if sea {
                    self.ships[q as usize] & te != 0
                } else {
                    self.buildings_of(q) & tv != 0
                };
                q != thief && near && !self.players[q as usize].hand.is_empty()
            })
            .fold(0, |m, q| m | 1 << q)
    }

    /// Length of `p`'s longest continuous trade route: roads, and on
    /// seafaring maps ships. Opponent buildings split routes; a route turns
    /// from road to ship only at `p`'s own settlement or city.
    pub fn compute_longest_road(&self, p: u8) -> u8 {
        let (roads, ships) = (self.roads[p as usize], self.ships[p as usize]);
        if roads | ships == 0 {
            return 0;
        }
        let topo = self.topo();
        let own = self.buildings_of(p);
        let blocked = self.occupied_vertices() & !own;
        let route = Route {
            topo,
            roads,
            ships,
            own,
            blocked,
        };
        vertices_in(topo.endpoints_of(roads | ships))
            .map(|v| route.longest_from(v, None, 0))
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
        let mut seen_v = 0u128;
        let mut seen_e = 0u128;
        for p in 0..n {
            let ps = &self.players[p];
            if self.settlements[p] & self.cities[p] != 0 {
                return Err(format!(
                    "player {p} has a settlement and a city on one vertex"
                ));
            }
            let b = self.settlements[p] | self.cities[p];
            let pieces = self.roads[p] | self.ships[p];
            if b & seen_v != 0 || pieces & seen_e != 0 || self.roads[p] & self.ships[p] != 0 {
                return Err("two pieces share a spot".into());
            }
            seen_v |= b;
            seen_e |= pieces;
            if self.settlements[p].count_ones() as u8 + ps.settlements_left != MAX_SETTLEMENTS
                || self.cities[p].count_ones() as u8 + ps.cities_left != MAX_CITIES
                || self.roads[p].count_ones() as u8 + ps.roads_left != MAX_ROADS
                || self.ships[p].count_ones() as u8 + ps.ships_left != MAX_SHIPS
            {
                return Err(format!("player {p} piece counts inconsistent"));
            }
            if b & !self.board.land_vertices != 0
                || self.roads[p] & !self.board.land_edges != 0
                || self.ships[p] & !self.board.sea_edges != 0
                || (self.ships[p] != 0 && !layout.has_ships())
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
        }
        for v in vertices_in(seen_v) {
            if topo.vertex_adj_mask[v as usize] & seen_v != 0 {
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
        if self.new_ships & !self.ships[self.current as usize] != 0 {
            return Err("new ships that are not the current player's".into());
        }
        if (seen_v & !topo.all_vertices) != 0 || (seen_e & !topo.all_edges) != 0 {
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
            for &num in FOG_HOME_NUMBER_POOL.iter().chain(FOG_NUMBER_POOL.iter()) {
                expected_nums[num as usize] += 1;
            }
            if num_counts != expected_nums {
                return Err("fog number tokens not conserved".into());
            }
            let route_vertices = topo.endpoints_of(seen_e);
            for t in 0..topo.num_tiles {
                if self.board.terrain[t] == Terrain::Fog
                    && route_vertices & topo.tile_vertex_mask[t] != 0
                {
                    return Err(format!("route touches unrevealed fog tile {t}"));
                }
            }
        } else if self.fog_terrain_len != 0 || self.fog_numbers_len != 0 {
            return Err("non-fog board has fog stacks".into());
        }
        if let Phase::SpecialBuild { builder } = self.phase
            && (!layout.has_special_build() || builder == self.current || builder as usize >= n)
        {
            return Err("invalid special building phase".into());
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
    roads: u128,
    ships: u128,
    /// The player's own buildings, where roads and ships join.
    own: u128,
    /// Intersections occupied by opponents.
    blocked: u128,
}

impl Route<'_> {
    /// Longest trail (no edge reused) starting at `v`, having arrived by a
    /// ship (`Some(true)`), a road (`Some(false)`) or not at all, never
    /// passing through a blocked vertex.
    fn longest_from(&self, v: u8, arrived_by_ship: Option<bool>, used: u128) -> u8 {
        let topo = self.topo;
        let mut best = 0;
        let links = &topo.vertex_edges[v as usize];
        for k in 0..3 {
            let e = links[k];
            if e == NONE {
                break;
            }
            let bit = 1u128 << e;
            if (self.roads | self.ships) & bit == 0 || used & bit != 0 {
                continue;
            }
            let ship = self.ships & bit != 0;
            if arrived_by_ship.is_some_and(|s| s != ship) && self.own & (1u128 << v) == 0 {
                continue;
            }
            let u = topo.vertex_vertices[v as usize][k];
            let len = if self.blocked & (1u128 << u) != 0 {
                1
            } else {
                1 + self.longest_from(u, Some(ship), used | bit)
            };
            best = best.max(len);
        }
        best
    }
}
