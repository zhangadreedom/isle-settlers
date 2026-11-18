//! Game state and read-only queries.

use crate::board::Board;
use crate::resources::ResourceSet;
use crate::rng::Rng;
use crate::topology::*;

/// Maximum number of seats (the extended board seats six).
pub const MAX_PLAYERS: usize = 6;
/// Size of the largest development deck.
pub const MAX_DEV_DECK: usize = 34;
pub const MAX_ROADS: u8 = 15;
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
            (DevCard::Knight, BoardLayout::Base) => 14,
            (DevCard::Knight, BoardLayout::Extended) => 20,
            (DevCard::VictoryPoint, _) => 5,
            (_, BoardLayout::Base) => 2,
            (_, BoardLayout::Extended) => 3,
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
            BoardLayout::Base => 19,
            BoardLayout::Extended => 24,
        }
    }

    /// Development cards in the deck.
    pub const fn dev_deck_size(self) -> usize {
        match self {
            BoardLayout::Base => 25,
            BoardLayout::Extended => 34,
        }
    }

    /// After each turn, the other players may build in turn order.
    pub const fn has_special_build(self) -> bool {
        matches!(self, BoardLayout::Extended)
    }

    /// Seats the board is designed for.
    pub const fn max_players(self) -> u8 {
        match self {
            BoardLayout::Base => 4,
            BoardLayout::Extended => 6,
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
}

impl GameConfig {
    /// Standard rules for `num_players` seats.
    pub fn new(num_players: u8) -> Self {
        GameConfig {
            num_players,
            ..Self::default()
        }
    }

    /// Board used by this configuration: the extended island seats 5–6.
    #[inline]
    pub fn layout(&self) -> BoardLayout {
        BoardLayout::for_players(self.num_players)
    }
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            num_players: 4,
            victory_points: 10,
            balanced_board: true,
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
    /// The current player must move the robber (after a 7 or a knight).
    MoveRobber,
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
    pub settlements_left: u8,
    pub cities_left: u8,
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
            settlements_left: MAX_SETTLEMENTS,
            cities_left: MAX_CITIES,
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
    pub bank: ResourceSet,
    /// Shuffled deck; cards are drawn from the end of `dev_deck[..dev_deck_len]`.
    pub dev_deck: [DevCard; MAX_DEV_DECK],
    pub dev_deck_len: u8,
    pub robber: u8,
    pub phase: Phase,
    pub current: u8,
    /// Index into the snake order of initial placements.
    pub setup_step: u8,
    /// Completed turns since the end of setup.
    pub turn: u32,
    /// Dice rolled this turn, `None` before rolling.
    pub dice: Option<(u8, u8)>,
    pub dev_card_played: bool,
    pub pending_discard: [u8; MAX_PLAYERS],
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
        if !(2..=MAX_PLAYERS as u8).contains(&config.num_players) {
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
        Ok(GameState {
            config,
            board,
            players: [PlayerState::new(); MAX_PLAYERS],
            settlements: [0; MAX_PLAYERS],
            cities: [0; MAX_PLAYERS],
            roads: [0; MAX_PLAYERS],
            bank: ResourceSet([layout.bank_per_resource(); 5]),
            dev_deck,
            dev_deck_len: deck_len as u8,
            robber: board.desert,
            phase: Phase::SetupSettlement,
            current: 0,
            setup_step: 0,
            turn: 0,
            dice: None,
            dev_card_played: false,
            pending_discard: [0; MAX_PLAYERS],
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

    #[inline]
    pub fn occupied_edges(&self) -> u128 {
        (0..self.num_players()).fold(0, |m, p| m | self.roads[p])
    }

    pub fn vertex_owner(&self, v: u8) -> Option<u8> {
        let bit = 1u128 << v;
        (0..self.num_players() as u8).find(|&p| self.buildings_of(p) & bit != 0)
    }

    pub fn edge_owner(&self, e: u8) -> Option<u8> {
        let bit = 1u128 << e;
        (0..self.num_players() as u8).find(|&p| self.roads[p as usize] & bit != 0)
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
        vp
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

    /// Vertices where `p` may place a settlement. During setup no road is required.
    pub fn settlement_spots(&self, p: u8, require_road: bool) -> u128 {
        let topo = self.topo();
        let occ = self.occupied_vertices();
        let free = topo.all_vertices & !(occ | topo.neighbors_of(occ));
        if require_road {
            free & topo.endpoints_of(self.roads[p as usize])
        } else {
            free
        }
    }

    /// Edges where `p` may place a road connected to their network. A road
    /// cannot continue through an intersection occupied by an opponent.
    pub fn road_spots(&self, p: u8) -> u128 {
        let topo = self.topo();
        let own = self.buildings_of(p);
        let opponents = self.occupied_vertices() & !own;
        let reach = own | (topo.endpoints_of(self.roads[p as usize]) & !opponents);
        topo.edges_touching(reach) & !self.occupied_edges()
    }

    /// Bitmask of players `thief` may rob after moving the robber to `tile`:
    /// opponents with a building on the tile who hold at least one card.
    pub fn robber_victims(&self, tile: u8, thief: u8) -> u8 {
        let tv = self.topo().tile_vertex_mask[tile as usize];
        (0..self.num_players() as u8)
            .filter(|&q| {
                q != thief
                    && self.buildings_of(q) & tv != 0
                    && !self.players[q as usize].hand.is_empty()
            })
            .fold(0, |m, q| m | 1 << q)
    }

    /// Length of `p`'s longest continuous road. Opponent buildings split roads.
    pub fn compute_longest_road(&self, p: u8) -> u8 {
        let roads = self.roads[p as usize];
        if roads == 0 {
            return 0;
        }
        let topo = self.topo();
        let blocked = self.occupied_vertices() & !self.buildings_of(p);
        vertices_in(topo.endpoints_of(roads))
            .map(|v| longest_from(topo, v, roads, 0, blocked))
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
            if b & seen_v != 0 || self.roads[p] & seen_e != 0 {
                return Err("two players share a spot".into());
            }
            seen_v |= b;
            seen_e |= self.roads[p];
            if self.settlements[p].count_ones() as u8 + ps.settlements_left != MAX_SETTLEMENTS
                || self.cities[p].count_ones() as u8 + ps.cities_left != MAX_CITIES
                || self.roads[p].count_ones() as u8 + ps.roads_left != MAX_ROADS
            {
                return Err(format!("player {p} piece counts inconsistent"));
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
        if self.robber as usize >= topo.num_tiles {
            return Err("robber off board".into());
        }
        if (seen_v & !topo.all_vertices) != 0 || (seen_e & !topo.all_edges) != 0 {
            return Err("piece off board".into());
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

/// Longest trail (no edge reused) starting at `v` within `roads`, never
/// passing through a `blocked` vertex.
fn longest_from(topo: &Topology, v: u8, roads: u128, used: u128, blocked: u128) -> u8 {
    let mut best = 0;
    let links = &topo.vertex_edges[v as usize];
    for k in 0..3 {
        let e = links[k];
        if e == NONE {
            break;
        }
        let bit = 1u128 << e;
        if roads & bit == 0 || used & bit != 0 {
            continue;
        }
        let u = topo.vertex_vertices[v as usize][k];
        let len = if blocked & (1u128 << u) != 0 {
            1
        } else {
            1 + longest_from(topo, u, roads, used | bit, blocked)
        };
        best = best.max(len);
    }
    best
}
