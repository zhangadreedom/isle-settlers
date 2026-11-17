//! Static board topology: tiles, vertices (intersections), edges (paths).
//!
//! All tables are generated at compile time by `build.rs`. Indices are `u8`;
//! sets of vertices are `u64` bitmasks and sets of edges are `u128` bitmasks.

include!(concat!(env!("OUT_DIR"), "/topology_gen.rs"));

/// Sentinel used to pad fixed-size adjacency arrays.
pub const NONE: u8 = u8::MAX;

/// Bitmask with every vertex set.
pub const ALL_VERTICES: u64 = (1u64 << NUM_VERTICES) - 1;
/// Bitmask with every edge set.
pub const ALL_EDGES: u128 = (1u128 << NUM_EDGES) - 1;

/// Iterates the indices of the set bits of a vertex mask.
#[inline]
pub fn vertices_in(mut mask: u64) -> impl Iterator<Item = u8> {
    std::iter::from_fn(move || {
        if mask == 0 {
            return None;
        }
        let v = mask.trailing_zeros() as u8;
        mask &= mask - 1;
        Some(v)
    })
}

/// Iterates the indices of the set bits of an edge mask.
#[inline]
pub fn edges_in(mut mask: u128) -> impl Iterator<Item = u8> {
    std::iter::from_fn(move || {
        if mask == 0 {
            return None;
        }
        let e = mask.trailing_zeros() as u8;
        mask &= mask - 1;
        Some(e)
    })
}

/// Union of the vertices adjacent to any vertex in `mask`.
#[inline]
pub fn neighbors_of(mask: u64) -> u64 {
    vertices_in(mask).fold(0, |acc, v| acc | VERTEX_ADJ_MASK[v as usize])
}

/// Union of the edges touching any vertex in `mask`.
#[inline]
pub fn edges_touching(mask: u64) -> u128 {
    vertices_in(mask).fold(0, |acc, v| acc | VERTEX_EDGE_MASK[v as usize])
}

/// Union of the endpoints of every edge in `mask`.
#[inline]
pub fn endpoints_of(mask: u128) -> u64 {
    edges_in(mask).fold(0, |acc, e| {
        let [a, b] = EDGE_VERTICES[e as usize];
        acc | (1u64 << a) | (1u64 << b)
    })
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

    #[test]
    fn counts() {
        assert_eq!(NUM_TILES, 19);
        assert_eq!(NUM_VERTICES, 54);
        assert_eq!(NUM_EDGES, 72);
        assert_eq!(NUM_COAST_EDGES, 30);
        assert_eq!(NUM_HARBORS, 9);
    }

    #[test]
    fn tiles_have_six_distinct_corners_and_sides() {
        for t in 0..NUM_TILES {
            let mut vs = TILE_VERTICES[t].to_vec();
            vs.sort();
            vs.dedup();
            assert_eq!(vs.len(), 6);
            let mut es = TILE_EDGES[t].to_vec();
            es.sort();
            es.dedup();
            assert_eq!(es.len(), 6);
            for i in 0..6 {
                let e = TILE_EDGES[t][i] as usize;
                let (a, b) = (TILE_VERTICES[t][i], TILE_VERTICES[t][(i + 1) % 6]);
                let mut ev = EDGE_VERTICES[e];
                ev.sort();
                assert_eq!(ev, [a.min(b), a.max(b)]);
            }
        }
    }

    #[test]
    fn vertex_degrees() {
        let mut tile_incidences = 0;
        for v in 0..NUM_VERTICES {
            let tiles = VERTEX_TILES[v].iter().filter(|&&t| t != NONE).count();
            let links = VERTEX_VERTICES[v].iter().filter(|&&n| n != NONE).count();
            assert!((1..=3).contains(&tiles));
            assert!((2..=3).contains(&links));
            assert_eq!(VERTEX_ADJ_MASK[v].count_ones() as usize, links);
            assert_eq!(VERTEX_EDGE_MASK[v].count_ones() as usize, links);
            for k in 0..3 {
                let (n, e) = (VERTEX_VERTICES[v][k], VERTEX_EDGES[v][k]);
                assert_eq!(n == NONE, e == NONE);
                if n != NONE {
                    let ev = EDGE_VERTICES[e as usize];
                    assert!(ev.contains(&(v as u8)) && ev.contains(&n));
                }
            }
            tile_incidences += tiles;
        }
        assert_eq!(tile_incidences, NUM_TILES * 6);
    }

    #[test]
    fn neighbors_are_symmetric() {
        for t in 0..NUM_TILES {
            for i in 0..6 {
                let n = TILE_NEIGHBORS[t][i];
                if n != NONE {
                    assert_eq!(TILE_NEIGHBORS[n as usize][(i + 3) % 6], t as u8);
                    // Shared side.
                    assert_eq!(TILE_EDGES[t][i], TILE_EDGES[n as usize][(i + 3) % 6]);
                }
            }
        }
        // Centre tile has six neighbours.
        let centre = TILE_AXIAL.iter().position(|&a| a == (0, 0)).unwrap();
        assert!(TILE_NEIGHBORS[centre].iter().all(|&n| n != NONE));
    }

    #[test]
    fn harbors_do_not_share_vertices() {
        let mut seen = 0u64;
        for &e in &HARBOR_EDGES {
            assert!(COAST_EDGES.contains(&e));
            let [a, b] = EDGE_VERTICES[e as usize];
            let m = (1u64 << a) | (1u64 << b);
            assert_eq!(seen & m, 0);
            seen |= m;
        }
    }

    #[test]
    fn bit_iterators() {
        let m = (1u64 << 3) | (1u64 << 53);
        assert_eq!(vertices_in(m).collect::<Vec<_>>(), vec![3, 53]);
        let e = (1u128 << 0) | (1u128 << 71);
        assert_eq!(edges_in(e).collect::<Vec<_>>(), vec![0, 71]);
    }
}
