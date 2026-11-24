//! Seafaring heuristics shared by the bots: where ships should sail.
//!
//! Everything here is a no-op on maps without sea (it only looks at sea
//! edges), so classic play is unaffected.

use isle_core::GameState;
use isle_core::topology::{NONE, vertices_in};

use crate::greedy::spot_value;

/// Settlement value of the island bonus a spot would earn `p`.
const ISLAND_BONUS_VALUE: f32 = 6.0;
/// Free sea edges searched beyond the tip of the fleet.
const SEARCH_DEPTH: usize = 6;
/// Value kept per extra ship needed to get there.
const DECAY: [f32; SEARCH_DEPTH + 1] = [1.0, 0.85, 0.7, 0.58, 0.48, 0.4, 0.33];

/// Extra value of settling `v` for `p`: the bonus for a first settlement on
/// an island they did not start on.
pub(crate) fn island_bonus_value(s: &GameState, p: u8, v: u8) -> f32 {
    if s.board.layout.island_bonus() == 0 {
        return 0.0;
    }
    let island = s.board.vertex_island[v as usize];
    let ps = &s.players[p as usize];
    if island == 0 || (ps.home_islands | ps.islands) & (1 << island) != 0 {
        0.0
    } else {
        ISLAND_BONUS_VALUE
    }
}

/// Where `p`'s next ship could attach: own buildings and the open tips of
/// their ships (not through opponents).
fn fleet_reach(s: &GameState, p: u8, ships: u128) -> u128 {
    let own = s.buildings_of(p);
    let opponents = s.occupied_vertices() & !own;
    own | (s.topo().endpoints_of(ships) & !opponents)
}

/// Best settlement spot reachable from the vertices in `starts` over free
/// sea edges (away from the pirate, not through opponents), decayed by the
/// number of ships still needed. `taken` are edges counted as occupied.
fn sea_search(s: &GameState, p: u8, starts: u128, taken: u128) -> f32 {
    let topo = s.topo();
    let spots = s.settlement_spots(p, false);
    let blocked = s.occupied_vertices() & !s.buildings_of(p);
    let open = s.board.sea_edges & !s.pirate_edges() & !taken;
    let mut best = 0.0f32;
    let mut seen = starts;
    let mut layer = starts;
    for depth in 0..=SEARCH_DEPTH {
        for v in vertices_in(layer & spots) {
            best = best.max((spot_value(s, p, v) + island_bonus_value(s, p, v)) * DECAY[depth]);
        }
        if depth == SEARCH_DEPTH {
            break;
        }
        let mut next = 0u128;
        for v in vertices_in(layer & !blocked) {
            let links = &topo.vertex_edges[v as usize];
            for k in 0..3 {
                let e = links[k];
                if e == NONE {
                    break;
                }
                if open & (1u128 << e) != 0 {
                    next |= 1u128 << topo.vertex_vertices[v as usize][k];
                }
            }
        }
        layer = next & !seen;
        if layer == 0 {
            break;
        }
        seen |= layer;
    }
    best
}

/// Value of a ship on edge `e` for `p`: the best spot it leads towards.
pub(crate) fn ship_value(s: &GameState, p: u8, e: u8) -> f32 {
    let ships = s.ships[p as usize];
    let reach = fleet_reach(s, p, ships);
    let [a, b] = s.topo().edge_vertices[e as usize];
    let far = if reach & (1u128 << a) != 0 { b } else { a };
    sea_search(s, p, 1u128 << far, s.occupied_edges() | (1u128 << e))
}

/// Value of moving `p`'s ship from `from` to `to`: what the ship would lead
/// to there, minus what it leads to now.
pub(crate) fn move_value(s: &GameState, p: u8, from: u8, to: u8) -> f32 {
    let mut without = *s;
    without.ships[p as usize] &= !(1u128 << from);
    ship_value(&without, p, to) - ship_value(s, p, from)
}

/// Best spot `p` could reach by sea from what they have now (0 if none,
/// or on maps without sea).
pub(crate) fn best_sea_target(s: &GameState, p: u8) -> f32 {
    if !s.board.layout.has_ships() || s.players[p as usize].ships_left == 0 {
        return 0.0;
    }
    let reach = fleet_reach(s, p, s.ships[p as usize]);
    // Start from vertices where a ship can actually be placed.
    let launch = s.topo().endpoints_of(s.ship_spots(p)) & reach;
    if launch == 0 {
        return 0.0;
    }
    sea_search(s, p, launch, s.occupied_edges()) * DECAY[1]
}

/// How close `q`'s fleet is to an island bonus: 1 for a spot one ship away,
/// decaying with distance, 0 if none (evaluation feature).
pub(crate) fn island_progress(s: &GameState, q: u8) -> f32 {
    if s.board.layout.island_bonus() == 0 || s.players[q as usize].settlements_left == 0 {
        return 0.0;
    }
    let topo = s.topo();
    let reach = fleet_reach(s, q, s.ships[q as usize]);
    let spots = s.settlement_spots(q, false);
    let blocked = s.occupied_vertices() & !s.buildings_of(q);
    let open = s.board.sea_edges & !s.pirate_edges() & !s.occupied_edges();
    let mut seen = reach;
    let mut layer = reach;
    for depth in 0..=SEARCH_DEPTH {
        if vertices_in(layer & spots).any(|v| island_bonus_value(s, q, v) > 0.0) {
            return DECAY[depth];
        }
        let mut next = 0u128;
        for v in vertices_in(layer & !blocked) {
            for k in 0..3 {
                let e = topo.vertex_edges[v as usize][k];
                if e == NONE {
                    break;
                }
                if open & (1u128 << e) != 0 {
                    next |= 1u128 << topo.vertex_vertices[v as usize][k];
                }
            }
        }
        layer = next & !seen;
        if layer == 0 {
            break;
        }
        seen |= layer;
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use isle_core::{GameConfig, GameState, Scenario};

    #[test]
    fn classic_boards_have_no_sea_targets() {
        let g = GameState::new(GameConfig::default(), 1).unwrap();
        assert_eq!(best_sea_target(&g, 0), 0.0);
        assert_eq!(island_progress(&g, 0), 0.0);
    }

    #[test]
    fn coastal_settlements_see_the_islands() {
        let mut g = GameState::new(GameConfig::scenario(Scenario::Archipelago, 3), 2).unwrap();
        g.setup_step = 6;
        g.phase = isle_core::Phase::Main;
        g.dice = Some((1, 2));
        // A coastal settlement on the home island facing the east.
        let topo = g.topo();
        let v = vertices_in(g.setup_spots())
            .filter(|&v| topo.vertex_edge_mask[v as usize] & g.board.sea_edges != 0)
            .max_by_key(|&v| topo.vertex_pos[v as usize].0)
            .unwrap();
        g.settlements[0] |= 1 << v;
        g.players[0].settlements_left -= 1;
        g.players[0].home_islands = 1 << 1;
        assert!(best_sea_target(&g, 0) > 0.0);
        assert!(island_progress(&g, 0) > 0.0);
        let best_ship = isle_core::topology::edges_in(g.ship_spots(0))
            .map(|e| ship_value(&g, 0, e))
            .fold(0.0f32, f32::max);
        assert!(best_ship > 0.0);
    }
}
