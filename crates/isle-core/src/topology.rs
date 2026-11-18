//! Static board topologies: tiles, vertices (intersections), edges (paths).
//!
//! The tables are generated at compile time by `build.rs`, one [`Topology`]
//! per [`BoardLayout`]. Indices are `u8`; sets of vertices and of edges are
//! `u128` bitmasks.

/// Sentinel used to pad fixed-size adjacency arrays.
pub const NONE: u8 = u8::MAX;

/// Which island is played.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum BoardLayout {
    /// 19 tiles, 9 harbors: 2–4 players.
    #[default]
    Base,
    /// 30 tiles, 11 harbors, special building phase: 5–6 players.
    Extended,
}

impl BoardLayout {
    #[inline]
    pub fn topology(self) -> &'static Topology {
        match self {
            BoardLayout::Base => &BASE,
            BoardLayout::Extended => &EXTENDED,
        }
    }

    /// The board normally used with `players` seats.
    pub fn for_players(players: u8) -> BoardLayout {
        if players > 4 {
            BoardLayout::Extended
        } else {
            BoardLayout::Base
        }
    }
}

/// Adjacency tables of one board. All slices are indexed by tile, vertex or
/// edge number; padded entries hold [`NONE`].
#[derive(Debug)]
pub struct Topology {
    pub num_tiles: usize,
    pub num_vertices: usize,
    pub num_edges: usize,
    /// Lattice position of each tile centre.
    pub tile_pos: &'static [(i16, i16)],
    /// Lattice position of each vertex.
    pub vertex_pos: &'static [(i16, i16)],
    /// Corners of each tile, clockwise from the top.
    pub tile_vertices: &'static [[u8; 6]],
    /// Sides of each tile; side `i` joins corners `i` and `i + 1`.
    pub tile_edges: &'static [[u8; 6]],
    /// Neighbouring tile across each side.
    pub tile_neighbors: &'static [[u8; 6]],
    /// Tiles touching each vertex (1–3).
    pub vertex_tiles: &'static [[u8; 3]],
    /// Adjacent vertices (2–3), index-aligned with `vertex_edges`.
    pub vertex_vertices: &'static [[u8; 3]],
    /// Edges at each vertex: `vertex_edges[v][k]` joins `v` and `vertex_vertices[v][k]`.
    pub vertex_edges: &'static [[u8; 3]],
    /// Endpoints of each edge.
    pub edge_vertices: &'static [[u8; 2]],
    /// Vertices adjacent to each vertex.
    pub vertex_adj_mask: &'static [u128],
    /// Edges touching each vertex.
    pub vertex_edge_mask: &'static [u128],
    /// Vertices of each tile.
    pub tile_vertex_mask: &'static [u128],
    /// Coastal edges, clockwise from the top.
    pub coast_edges: &'static [u8],
    /// Coastal edge of each harbor slot.
    pub harbor_edges: &'static [u8],
    pub all_vertices: u128,
    pub all_edges: u128,
}

include!(concat!(env!("OUT_DIR"), "/topology_gen.rs"));

impl Topology {
    /// Union of the vertices adjacent to any vertex in `mask`.
    #[inline]
    pub fn neighbors_of(&self, mask: u128) -> u128 {
        vertices_in(mask).fold(0, |acc, v| acc | self.vertex_adj_mask[v as usize])
    }

    /// Union of the edges touching any vertex in `mask`.
    #[inline]
    pub fn edges_touching(&self, mask: u128) -> u128 {
        vertices_in(mask).fold(0, |acc, v| acc | self.vertex_edge_mask[v as usize])
    }

    /// Union of the endpoints of every edge in `mask`.
    #[inline]
    pub fn endpoints_of(&self, mask: u128) -> u128 {
        edges_in(mask).fold(0, |acc, e| {
            let [a, b] = self.edge_vertices[e as usize];
            acc | (1u128 << a) | (1u128 << b)
        })
    }
}

/// Iterates the indices of the set bits of a mask.
#[inline]
pub fn bits(mut mask: u128) -> impl Iterator<Item = u8> {
    std::iter::from_fn(move || {
        if mask == 0 {
            return None;
        }
        let i = mask.trailing_zeros() as u8;
        mask &= mask - 1;
        Some(i)
    })
}

/// Iterates the vertices of a vertex mask.
#[inline]
pub fn vertices_in(mask: u128) -> impl Iterator<Item = u8> {
    bits(mask)
}

/// Iterates the edges of an edge mask.
#[inline]
pub fn edges_in(mask: u128) -> impl Iterator<Item = u8> {
    bits(mask)
}

/// World-space position of a lattice point for a hex of circumradius `size`.
/// Returns `(x, z)` with `z` pointing "down" the board.
#[inline]
pub fn lattice_to_world((x, y): (i16, i16), size: f32) -> (f32, f32) {
    (x as f32 * size * 0.866_025_4, y as f32 * size * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [&Topology; 2] = [&BASE, &EXTENDED];

    #[test]
    fn counts() {
        assert_eq!(
            (BASE.num_tiles, BASE.num_vertices, BASE.num_edges),
            (19, 54, 72)
        );
        assert_eq!((BASE.coast_edges.len(), BASE.harbor_edges.len()), (30, 9));
        assert_eq!(EXTENDED.num_tiles, 30);
        assert_eq!(EXTENDED.harbor_edges.len(), 11);
        for t in ALL {
            // Euler's formula for a disc tiled by hexes: E = V + F - 1.
            assert_eq!(t.num_edges, t.num_vertices + t.num_tiles - 1);
            assert_eq!(t.tile_pos.len(), t.num_tiles);
            assert_eq!(t.vertex_pos.len(), t.num_vertices);
            assert_eq!(t.edge_vertices.len(), t.num_edges);
        }
        assert!(EXTENDED.num_vertices <= MAX_VERTICES && EXTENDED.num_edges <= MAX_EDGES);
    }

    #[test]
    fn tiles_have_six_distinct_corners_and_sides() {
        for topo in ALL {
            for t in 0..topo.num_tiles {
                let mut vs = topo.tile_vertices[t].to_vec();
                vs.sort();
                vs.dedup();
                assert_eq!(vs.len(), 6);
                for i in 0..6 {
                    let e = topo.tile_edges[t][i] as usize;
                    let (a, b) = (topo.tile_vertices[t][i], topo.tile_vertices[t][(i + 1) % 6]);
                    let mut ev = topo.edge_vertices[e];
                    ev.sort();
                    assert_eq!(ev, [a.min(b), a.max(b)]);
                }
            }
        }
    }

    #[test]
    fn vertex_degrees() {
        for topo in ALL {
            let mut tile_incidences = 0;
            for v in 0..topo.num_vertices {
                let tiles = topo.vertex_tiles[v].iter().filter(|&&t| t != NONE).count();
                let links = topo.vertex_vertices[v]
                    .iter()
                    .filter(|&&n| n != NONE)
                    .count();
                assert!((1..=3).contains(&tiles));
                assert!((2..=3).contains(&links));
                assert_eq!(topo.vertex_adj_mask[v].count_ones() as usize, links);
                assert_eq!(topo.vertex_edge_mask[v].count_ones() as usize, links);
                for k in 0..3 {
                    let (n, e) = (topo.vertex_vertices[v][k], topo.vertex_edges[v][k]);
                    assert_eq!(n == NONE, e == NONE);
                    if n != NONE {
                        let ev = topo.edge_vertices[e as usize];
                        assert!(ev.contains(&(v as u8)) && ev.contains(&n));
                    }
                }
                tile_incidences += tiles;
            }
            assert_eq!(tile_incidences, topo.num_tiles * 6);
        }
    }

    #[test]
    fn neighbors_are_symmetric() {
        for topo in ALL {
            for t in 0..topo.num_tiles {
                for i in 0..6 {
                    let n = topo.tile_neighbors[t][i];
                    if n != NONE {
                        assert_eq!(topo.tile_neighbors[n as usize][(i + 3) % 6], t as u8);
                        assert_eq!(
                            topo.tile_edges[t][i],
                            topo.tile_edges[n as usize][(i + 3) % 6]
                        );
                    }
                }
            }
        }
        // The base board's centre tile has six neighbours.
        let centre = BASE.tile_pos.iter().position(|&p| p == (0, 0)).unwrap();
        assert!(BASE.tile_neighbors[centre].iter().all(|&n| n != NONE));
    }

    #[test]
    fn harbors_do_not_share_vertices() {
        for topo in ALL {
            let mut seen = 0u128;
            for &e in topo.harbor_edges {
                assert!(topo.coast_edges.contains(&e));
                let [a, b] = topo.edge_vertices[e as usize];
                let m = (1u128 << a) | (1u128 << b);
                assert_eq!(seen & m, 0);
                seen |= m;
            }
        }
    }

    #[test]
    fn bit_iterators() {
        let m = (1u128 << 3) | (1u128 << 79);
        assert_eq!(bits(m).collect::<Vec<_>>(), vec![3, 79]);
        assert_eq!(BoardLayout::for_players(4), BoardLayout::Base);
        assert_eq!(BoardLayout::for_players(5), BoardLayout::Extended);
    }
}
