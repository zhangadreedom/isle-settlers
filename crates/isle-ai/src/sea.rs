//! Seafaring heuristics shared by the bots: where ships should sail.
//!
//! Everything here is a no-op on maps without sea (it only looks at sea
//! edges), so classic play is unaffected.

use isle_core::topology::{NONE, bits, vertices_in};
use isle_core::{BitSet, GameState};

use crate::greedy::spot_value;

/// Settlement value of the island bonus a spot would earn `p`.
const ISLAND_BONUS_VALUE: f32 = 6.0;
/// Exploration value per unexplored fog tile a vertex reveals (expected
/// free resource plus the chance of uncovering rich land or gold).
const FOG_TILE_VALUE: f32 = 4.5;
/// Extra pull towards the fog before `p` has earned their outer-island bonus.
const FOG_SEEK_BONUS: f32 = 3.0;
/// Free sea edges searched beyond the tip of the fleet.
const SEARCH_DEPTH: usize = 6;
/// Value kept per extra ship needed to get there.
const DECAY: [f32; SEARCH_DEPTH + 1] = [1.0, 0.85, 0.7, 0.58, 0.48, 0.4, 0.33];

/// Whether `p` currently has a ship/building reaching any site for mission `m`.
pub(crate) fn explorer_track_reached(s: &GameState, p: u8, m: usize) -> bool {
    match m {
        0 => bits(s.board.pirate_lairs).any(|t| s.can_reach_pirate_lair(p, t)),
        1 => bits(s.board.deep_shoals).any(|t| s.can_reach_deep_shoal(p, t)),
        2 => bits(s.board.spice_islands).any(|t| s.can_reach_spice_island(p, t)),
        _ => false,
    }
}

fn explorer_needed_site_edges(s: &GameState, p: u8) -> [(BitSet, f32); 3] {
    let topo = s.topo();
    let ps = &s.players[p as usize];
    let masks = [
        s.board.pirate_lairs,
        s.board.deep_shoals,
        s.board.spice_islands,
    ];
    let mut out = [(BitSet::EMPTY, 0.0f32); 3];
    for m in 0..3 {
        if ps.explorer_missions[m] < 3 && !explorer_track_reached(s, p, m) {
            let edges = bits(masks[m]).fold(BitSet::EMPTY, |acc, t| acc | topo.tile_edge_mask(t));
            let base = if m == 2 { 10.5 } else { 9.0 }
                + if s.explorer_champions[m].is_none() {
                    1.5
                } else {
                    0.0
                };
            out[m] = (edges, base);
        }
    }
    out
}

fn explorer_edge_bonus(s: &GameState, p: u8, edge: u8) -> f32 {
    let sites = explorer_needed_site_edges(s, p);
    let mut best = 0.0f32;
    for &(edges, val) in &sites {
        if edges.contains(edge) {
            best = best.max(val);
        }
    }
    best
}

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

/// Exploration value of reaching `v` when it borders unexplored fog tiles.
fn fog_value(s: &GameState, p: u8, v: u8) -> f32 {
    if !s.board.layout.has_fog() {
        return 0.0;
    }
    let count = s.topo().vertex_tiles[v as usize]
        .iter()
        .filter(|&&t| t != NONE && s.board.terrain[t as usize] == isle_core::Terrain::Fog)
        .count();
    if count == 0 {
        return 0.0;
    }
    let seek = if s.players[p as usize].islands == 0 {
        FOG_SEEK_BONUS
    } else {
        0.0
    };
    seek + FOG_TILE_VALUE * count as f32
}

/// Where `p`'s next ship could attach: own buildings and the open tips of
/// their ships (not through opponents).
fn fleet_reach(s: &GameState, p: u8, ships: BitSet) -> BitSet {
    let own = s.buildings_of(p);
    let opponents = (s.occupied_vertices() & !own) | s.opponent_knights(p);
    own | (s.topo().endpoints_of(ships) & !opponents)
}

/// Best settlement or exploration spot reachable from the vertices in `starts`
/// over free sea edges (away from the pirate, not through opponents), decayed
/// by the number of ships still needed. `taken` are edges counted as occupied.
fn sea_search(s: &GameState, p: u8, starts: BitSet, taken: BitSet) -> f32 {
    let topo = s.topo();
    let spots = s.settlement_spots(p, false);
    let has_fog = s.board.layout.has_fog();
    let explorers = s.config.scenario.is_explorers();
    let exp_sites = if explorers {
        explorer_needed_site_edges(s, p)
    } else {
        [(BitSet::EMPTY, 0.0); 3]
    };
    let blocked = (s.occupied_vertices() & !s.buildings_of(p)) | s.opponent_knights(p);
    let open = s.board.sea_edges & !s.pirate_edges() & !taken;
    let mut best = 0.0f32;
    let mut seen = starts;
    let mut layer = starts;
    for depth in 0..=SEARCH_DEPTH {
        for v in vertices_in(layer & spots) {
            best = best.max((spot_value(s, p, v) + island_bonus_value(s, p, v)) * DECAY[depth]);
        }
        if has_fog {
            for v in vertices_in(layer) {
                best = best.max(fog_value(s, p, v) * DECAY[depth]);
            }
        }
        if explorers {
            let next_decay = DECAY[(depth + 1).min(SEARCH_DEPTH)];
            for v in vertices_in(layer & !blocked) {
                let ve = topo.vertex_edge_mask[v as usize] & open;
                for &(edges, val) in &exp_sites {
                    if val > 0.0 && !(ve & edges).is_empty() {
                        best = best.max(val * next_decay);
                    }
                }
            }
        }
        if depth == SEARCH_DEPTH {
            break;
        }
        let mut next = BitSet::EMPTY;
        for v in vertices_in(layer & !blocked) {
            let links = &topo.vertex_edges[v as usize];
            for k in 0..3 {
                let e = links[k];
                if e == NONE {
                    break;
                }
                if open.contains(e) {
                    next.insert(topo.vertex_vertices[v as usize][k]);
                }
            }
        }
        layer = next & !seen;
        if layer.is_empty() {
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
    let far = if reach.contains(a) { b } else { a };
    let val = sea_search(s, p, BitSet::bit(far), s.occupied_edges() | BitSet::bit(e));
    if s.config.scenario.is_explorers() {
        val.max(explorer_edge_bonus(s, p, e))
    } else {
        val
    }
}

/// Value of moving `p`'s ship from `from` to `to`: what the ship would lead
/// to there, minus what it leads to now.
pub(crate) fn move_value(s: &GameState, p: u8, from: u8, to: u8) -> f32 {
    let mut without = *s;
    without.ships[p as usize].remove(from);
    if s.config.scenario.is_explorers() {
        ship_value(&without, p, to) - ship_value(&without, p, from)
    } else {
        ship_value(&without, p, to) - ship_value(s, p, from)
    }
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
    if launch.is_empty() {
        return 0.0;
    }
    sea_search(s, p, launch, s.occupied_edges()) * DECAY[1]
}

/// How close `q`'s fleet is to an island bonus: 1 for a spot one ship away,
/// decaying with distance, 0 if none (evaluation feature).
pub(crate) fn island_progress(s: &GameState, q: u8) -> f32 {
    let explorers = s.config.scenario.is_explorers();
    if s.board.layout.island_bonus() == 0
        || (s.players[q as usize].settlements_left == 0 && !explorers)
    {
        return 0.0;
    }
    let topo = s.topo();
    let reach = fleet_reach(s, q, s.ships[q as usize]);
    let spots = if s.players[q as usize].settlements_left > 0 {
        s.settlement_spots(q, false)
    } else {
        BitSet::EMPTY
    };
    let exp_edges = if explorers {
        explorer_needed_site_edges(s, q)
            .into_iter()
            .fold(BitSet::EMPTY, |acc, (e, _)| acc | e)
    } else {
        BitSet::EMPTY
    };
    let blocked = (s.occupied_vertices() & !s.buildings_of(q)) | s.opponent_knights(q);
    let open = s.board.sea_edges & !s.pirate_edges() & !s.occupied_edges();
    let has_fog = s.board.layout.has_fog() && s.players[q as usize].islands == 0;
    let mut fog_best = 0.0f32;
    let mut seen = reach;
    let mut layer = reach;
    for depth in 0..=SEARCH_DEPTH {
        if vertices_in(layer & spots).any(|v| island_bonus_value(s, q, v) > 0.0) {
            return DECAY[depth];
        }
        if !exp_edges.is_empty()
            && vertices_in(layer & !blocked)
                .any(|v| !(topo.vertex_edge_mask[v as usize] & open & exp_edges).is_empty())
        {
            return DECAY[(depth + 1).min(SEARCH_DEPTH)];
        }
        if has_fog && fog_best == 0.0 && vertices_in(layer).any(|v| fog_value(s, q, v) > 0.0) {
            fog_best = 0.65 * DECAY[depth];
        }
        let mut next = BitSet::EMPTY;
        for v in vertices_in(layer & !blocked) {
            for k in 0..3 {
                let e = topo.vertex_edges[v as usize][k];
                if e == NONE {
                    break;
                }
                if open.contains(e) {
                    next.insert(topo.vertex_vertices[v as usize][k]);
                }
            }
        }
        layer = next & !seen;
        if layer.is_empty() {
            break;
        }
        seen |= layer;
    }
    fog_best
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
            .filter(|&v| !(topo.vertex_edge_mask[v as usize] & g.board.sea_edges).is_empty())
            .max_by_key(|&v| topo.vertex_pos[v as usize].0)
            .unwrap();
        g.settlements[0].insert(v);
        g.players[0].settlements_left -= 1;
        g.players[0].home_islands = 1 << 1;
        assert!(best_sea_target(&g, 0) > 0.0);
        assert!(island_progress(&g, 0) > 0.0);
        let best_ship = isle_core::topology::edges_in(g.ship_spots(0))
            .map(|e| ship_value(&g, 0, e))
            .fold(0.0f32, f32::max);
        assert!(best_ship > 0.0);
    }

    #[test]
    fn extended_six_player_seafaring_maps_see_islands_and_fog() {
        for scenario in [Scenario::Archipelago, Scenario::FogIslands] {
            let mut g = GameState::new(GameConfig::scenario(scenario, 6), 5).unwrap();
            g.setup_step = 12;
            g.phase = isle_core::Phase::Main;
            g.dice = Some((2, 3));
            let topo = g.topo();
            let v = vertices_in(g.setup_spots())
                .filter(|&v| {
                    !(topo.vertex_edge_mask[v as usize] & g.board.sea_edges & !g.pirate_edges())
                        .is_empty()
                })
                .max_by_key(|&v| topo.vertex_pos[v as usize].0)
                .unwrap();
            g.settlements[0].insert(v);
            g.players[0].settlements_left -= 1;
            g.players[0].home_islands = 1 << 1;
            assert!(best_sea_target(&g, 0) > 0.0, "{scenario:?}");
            assert!(island_progress(&g, 0) > 0.0, "{scenario:?}");
        }
    }
}
