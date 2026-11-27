//! # isle-proto
//!
//! What each seat is allowed to see. The server (and the in-browser match
//! host) never sends a raw [`GameState`]: it sends a [`PlayerView`] projected
//! for the viewer plus events passed through [`Event::redacted_for`].
//!
//! Hidden information: other players' hand composition and development
//! cards, the order of the development deck, and the RNG state (future dice).
//! Card *counts* are public.

use isle_core::topology::{NONE, edges_in, vertices_in};
use isle_core::{
    BARBARIAN_STEPS, BoardLayout, GameConfig, GameState, HarborKind, Phase, Resource, ResourceSet,
    Terrain, TradeOffer,
};
use serde::{Deserialize, Serialize};

pub use isle_core::{
    Action, CityTrack, Commodity, CommoditySet, CustomMap, DevCard, Event, RuleError, TradeGood,
    TradeResponse,
};

pub mod account;
pub mod net;
pub mod ranking;
pub mod replay;

pub use replay::{ReplayError, ReplayPlayer, ReplayRecord};

/// Owner value meaning "nobody" in per-spot arrays.
pub const EMPTY: u8 = u8::MAX;
/// Flag OR-ed into a building owner when the building is a city.
pub const CITY_FLAG: u8 = 0x80;

/// Static board layout, sent once per match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BoardInfo {
    pub config: GameConfig,
    /// Island shape; selects the topology tables.
    pub layout: BoardLayout,
    /// Terrain per tile.
    pub terrain: Vec<Terrain>,
    /// Number token per tile (0 for the desert).
    pub numbers: Vec<u8>,
    /// Harbor per harbor slot.
    pub harbors: Vec<HarborKind>,
    /// Island per tile: 0 for the sea, from 1 (the home island) on.
    pub islands: Vec<u8>,
}

/// Public information about a seat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlayerPublic {
    pub seat: u8,
    pub resource_count: u8,
    /// Unplayed development cards (including victory point cards).
    pub dev_card_count: u8,
    /// Development cards played face up, indexed by `DevCard`.
    pub played_dev_cards: [u8; 5],
    pub roads_left: u8,
    pub ships_left: u8,
    pub settlements_left: u8,
    pub cities_left: u8,
    /// Islands that earned the settling bonus (bit `i` = island `i`).
    pub islands: u8,
    pub longest_road: u8,
    /// Points from buildings, longest road, largest army and islands.
    pub public_victory_points: u8,
    /// Harbor access bits: 1 << resource for 2:1 harbors, 1 << 5 for 3:1.
    pub harbors: u8,
    pub commodity_count: u8,
    pub city_progress: [u8; 3],
    pub knights_left: [u8; 3],
    pub defender_points: u8,
}

/// The viewer's own private information.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PrivateInfo {
    pub seat: u8,
    pub hand: ResourceSet,
    /// Playable development cards plus victory point cards, by `DevCard`.
    pub dev_cards: [u8; 5],
    /// Cards bought this turn (playable from the next turn), by `DevCard`.
    pub new_dev_cards: [u8; 5],
    /// Total points including hidden victory point cards.
    pub victory_points: u8,
    /// Best maritime ratio for giving each resource.
    pub trade_ratios: [u8; 5],
    pub commodities: CommoditySet,
    pub commodity_trade_ratio: u8,
}

/// Revealed board state on fog maps, updated as unexplored tiles flip.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FogBoard {
    pub terrain: Vec<Terrain>,
    pub numbers: Vec<u8>,
    pub islands: Vec<u8>,
}

/// A knight piece on an intersection in Citadels mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct KnightView {
    pub vertex: u8,
    pub owner: u8,
    pub level: u8,
    pub active: bool,
    pub can_act: bool,
    pub promoted: bool,
}

/// Public Citadels (Cities & Knights) board state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CitadelView {
    pub commodity_bank: CommoditySet,
    pub barbarian_pos: u8,
    pub barbarian_steps: u8,
    pub metropolises: [Option<u8>; 3],
    pub metropolis_owners: [Option<u8>; 3],
    pub knights: Vec<KnightView>,
}

/// Everything one seat may know about the game right now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    /// Seat this view is for; `None` for spectators.
    pub viewer: Option<u8>,
    pub phase: Phase,
    pub current: u8,
    /// Seat expected to act next (during discards: lowest seat still owing;
    /// during a special building phase: the builder).
    pub to_move: Option<u8>,
    pub turn: u32,
    pub dice: Option<(u8, u8)>,
    pub dev_card_played: bool,
    pub robber: u8,
    /// Seafaring: the pirate's sea tile.
    pub pirate: Option<u8>,
    /// Seafaring: the current player has moved a ship this turn.
    pub ship_moved: bool,
    pub bank: ResourceSet,
    pub dev_deck_len: u8,
    /// Per vertex: `EMPTY`, owner seat, or owner | `CITY_FLAG`.
    pub buildings: Vec<u8>,
    /// Per edge: `EMPTY` or owner seat.
    pub roads: Vec<u8>,
    /// Per edge: `EMPTY` or the owner of a ship there.
    pub ships: Vec<u8>,
    pub players: Vec<PlayerPublic>,
    pub me: Option<PrivateInfo>,
    /// Cards each seat still has to discard after a 7.
    pub pending_discard: Vec<u8>,
    /// Gold picks each seat still has to make.
    pub pending_gold: Vec<u8>,
    pub trade: Option<TradeOffer>,
    pub longest_road_owner: Option<u8>,
    pub largest_army_owner: Option<u8>,
    pub winner: Option<u8>,
    /// Total points per seat, revealed once the game is over.
    pub final_points: Option<Vec<u8>>,
    /// Fog maps: current board tiles as revealed so far.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog: Option<FogBoard>,
    /// Citadels mode: commodities, metropolises, knights and barbarian fleet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citadels: Option<CitadelView>,
}

pub fn board_info(state: &GameState) -> BoardInfo {
    BoardInfo {
        config: state.config,
        layout: state.board.layout,
        terrain: state.board.tiles().to_vec(),
        numbers: state.board.numbers[..state.topo().num_tiles].to_vec(),
        harbors: state.board.harbor_kinds().to_vec(),
        islands: state.board.tile_island[..state.topo().num_tiles].to_vec(),
    }
}

/// Projects `state` for `viewer` (`None` = spectator).
pub fn player_view(state: &GameState, viewer: Option<u8>) -> PlayerView {
    let n = state.num_players();
    let topo = state.topo();
    let mut buildings = vec![EMPTY; topo.num_vertices];
    let mut roads = vec![EMPTY; topo.num_edges];
    let mut ships = vec![EMPTY; topo.num_edges];
    for p in 0..n {
        for v in vertices_in(state.settlements[p]) {
            buildings[v as usize] = p as u8;
        }
        for v in vertices_in(state.cities[p]) {
            buildings[v as usize] = p as u8 | CITY_FLAG;
        }
        for e in edges_in(state.roads[p]) {
            roads[e as usize] = p as u8;
        }
        for e in edges_in(state.ships[p]) {
            ships[e as usize] = p as u8;
        }
    }
    let players = (0..n as u8)
        .map(|p| {
            let ps = &state.players[p as usize];
            PlayerPublic {
                seat: p,
                resource_count: ps.hand.total() as u8,
                dev_card_count: ps.dev_card_count() as u8,
                played_dev_cards: ps.played_dev_cards,
                roads_left: ps.roads_left,
                ships_left: ps.ships_left,
                settlements_left: ps.settlements_left,
                cities_left: ps.cities_left,
                islands: ps.islands,
                longest_road: ps.longest_road,
                public_victory_points: state.public_victory_points(p),
                harbors: ps.harbors,
                commodity_count: ps.commodities.total() as u8,
                city_progress: ps.city_progress,
                knights_left: ps.knights_left,
                defender_points: ps.defender_points,
            }
        })
        .collect();
    let me = viewer.filter(|&p| (p as usize) < n).map(|p| {
        let ps = &state.players[p as usize];
        PrivateInfo {
            seat: p,
            hand: ps.hand,
            dev_cards: ps.dev_cards,
            new_dev_cards: ps.new_dev_cards,
            victory_points: state.victory_points(p),
            trade_ratios: Resource::ALL.map(|r| state.trade_ratio(p, r)),
            commodities: ps.commodities,
            commodity_trade_ratio: state.commodity_trade_ratio(p),
        }
    });
    PlayerView {
        viewer,
        phase: state.phase,
        current: state.current,
        to_move: state.to_move(),
        turn: state.turn,
        dice: state.dice,
        dev_card_played: state.dev_card_played,
        robber: state.robber,
        pirate: (state.pirate != NONE).then_some(state.pirate),
        ship_moved: state.ship_moved,
        bank: state.bank,
        dev_deck_len: state.dev_deck_len,
        buildings,
        roads,
        ships,
        players,
        me,
        pending_discard: state.pending_discard[..n].to_vec(),
        pending_gold: state.pending_gold[..n].to_vec(),
        trade: state.trade,
        longest_road_owner: state.longest_road_owner,
        largest_army_owner: state.largest_army_owner,
        winner: state.winner,
        final_points: state
            .winner
            .map(|_| (0..n as u8).map(|p| state.victory_points(p)).collect()),
        fog: state.board.layout.has_fog().then(|| FogBoard {
            terrain: state.board.tiles().to_vec(),
            numbers: state.board.numbers[..topo.num_tiles].to_vec(),
            islands: state.board.tile_island[..topo.num_tiles].to_vec(),
        }),
        citadels: state.config.scenario.is_citadels().then(|| {
            let knights = (0..topo.num_vertices as u8)
                .filter(|&v| state.knight_owner[v as usize] != NONE)
                .map(|v| {
                    let active = state.knight_active.contains(v);
                    KnightView {
                        vertex: v,
                        owner: state.knight_owner[v as usize],
                        level: state.knight_level[v as usize],
                        active,
                        can_act: active && !state.knight_fresh.contains(v),
                        promoted: state.knight_promoted.contains(v),
                    }
                })
                .collect();
            CitadelView {
                commodity_bank: state.commodity_bank,
                barbarian_pos: state.barbarian_pos,
                barbarian_steps: BARBARIAN_STEPS,
                metropolises: state.metropolises.map(|v| (v != NONE).then_some(v)),
                metropolis_owners: state.metropolis_owners,
                knights,
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isle_ai::{Bot, WeightedRandomBot};
    use isle_core::{GameConfig, Rng};

    /// Mid-game states from bot play (classic, fog islands and citadels).
    fn sample_states() -> Vec<GameState> {
        let mut out = Vec::new();
        for (scenario, seeds) in [
            (isle_core::Scenario::Classic, 0..20u64),
            (isle_core::Scenario::FogIslands, 20..35u64),
            (isle_core::Scenario::Citadels, 35..50u64),
        ] {
            for seed in seeds {
                let mut g = GameState::new(GameConfig::scenario(scenario, 4), seed).unwrap();
                let mut bots: Vec<WeightedRandomBot> = (0..4)
                    .map(|k| WeightedRandomBot::new(seed * 7 + k))
                    .collect();
                let mut legal = Vec::new();
                for step in 0..400 {
                    let Some(p) = g.to_move() else { break };
                    g.legal_actions(p, &mut legal);
                    let a = bots[p as usize].choose(&g, p, &legal);
                    g.apply(p, a).unwrap();
                    if step % 50 == 49 {
                        out.push(g);
                    }
                }
            }
        }
        out
    }

    /// Changes everything a viewer must not be able to observe: other seats'
    /// hand/commodity composition and development cards (keeping counts), the
    /// deck order, hidden fog stacks and the RNG.
    fn scramble_hidden(g: &GameState, viewer: u8, seed: u64) -> GameState {
        let mut h = *g;
        for q in 0..h.num_players() {
            if q == viewer as usize {
                continue;
            }
            let total = h.players[q].hand.total() as u8;
            h.players[q].hand =
                ResourceSet::single(Resource::from_index((q + 1) % 5).unwrap(), total);
            let ctotal = h.players[q].commodities.total() as u8;
            h.players[q].commodities =
                CommoditySet::single(Commodity::from_index((q + 1) % 3).unwrap(), ctotal);
            let d = h.players[q].dev_cards;
            h.players[q].dev_cards = [d[1], d[2], d[3], d[4], d[0]];
            let nd = h.players[q].new_dev_cards;
            h.players[q].new_dev_cards = [nd[4], nd[0], nd[1], nd[2], nd[3]];
        }
        let mut rng = Rng::new(seed);
        let len = h.dev_deck_len as usize;
        rng.shuffle(&mut h.dev_deck[..len]);
        let ft = h.fog_terrain_len as usize;
        rng.shuffle(&mut h.fog_terrain[..ft]);
        let fn_len = h.fog_numbers_len as usize;
        rng.shuffle(&mut h.fog_numbers[..fn_len]);
        h.rng = Rng::new(seed ^ 0xABCD);
        h
    }

    #[test]
    fn views_do_not_depend_on_hidden_information() {
        for (i, g) in sample_states().iter().enumerate() {
            for viewer in 0..g.num_players() as u8 {
                let h = scramble_hidden(g, viewer, i as u64);
                // Scrambling VP cards would change the final score reveal.
                if g.winner.is_none() {
                    assert_eq!(player_view(g, Some(viewer)), player_view(&h, Some(viewer)));
                }
            }
        }
    }

    #[test]
    fn views_show_own_private_info() {
        for g in sample_states() {
            for viewer in 0..g.num_players() as u8 {
                let v = player_view(&g, Some(viewer));
                let me = v.me.unwrap();
                assert_eq!(me.hand, g.players[viewer as usize].hand);
                assert_eq!(me.commodities, g.players[viewer as usize].commodities);
                assert_eq!(me.commodity_trade_ratio, g.commodity_trade_ratio(viewer));
                assert_eq!(me.victory_points, g.victory_points(viewer));
                assert_eq!(
                    v.players[viewer as usize].resource_count as u32,
                    me.hand.total()
                );
                assert_eq!(
                    v.players[viewer as usize].commodity_count as u32,
                    me.commodities.total()
                );
                assert_eq!(v.citadels.is_some(), g.config.scenario.is_citadels());
            }
            assert!(player_view(&g, None).me.is_none());
        }
    }

    #[test]
    fn private_events_are_redacted() {
        let stolen = Event::Stolen {
            thief: 0,
            victim: 1,
            resource: Some(Resource::Ore),
        };
        assert_eq!(stolen.redacted_for(Some(0)), stolen);
        assert_eq!(stolen.redacted_for(Some(1)), stolen);
        assert_eq!(
            stolen.redacted_for(Some(2)),
            Event::Stolen {
                thief: 0,
                victim: 1,
                resource: None
            }
        );
        assert_eq!(
            stolen.redacted_for(None),
            Event::Stolen {
                thief: 0,
                victim: 1,
                resource: None
            }
        );
        let cstolen = Event::CommodityStolen {
            thief: 0,
            victim: 1,
            commodity: Some(Commodity::Cloth),
        };
        assert_eq!(cstolen.redacted_for(Some(0)), cstolen);
        assert_eq!(cstolen.redacted_for(Some(1)), cstolen);
        assert_eq!(
            cstolen.redacted_for(Some(2)),
            Event::CommodityStolen {
                thief: 0,
                victim: 1,
                commodity: None
            }
        );
        assert_eq!(
            cstolen.redacted_for(None),
            Event::CommodityStolen {
                thief: 0,
                victim: 1,
                commodity: None
            }
        );
        let bought = Event::DevCardBought {
            player: 2,
            card: Some(DevCard::Knight),
        };
        assert_eq!(bought.redacted_for(Some(2)), bought);
        assert_eq!(
            bought.redacted_for(Some(0)),
            Event::DevCardBought {
                player: 2,
                card: None
            }
        );
        let roll = Event::DiceRolled {
            player: 1,
            dice: (3, 4),
        };
        assert_eq!(roll.redacted_for(None), roll);
    }
}
