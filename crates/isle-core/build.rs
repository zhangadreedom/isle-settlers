//! Generates the board topology of the standard 19-tile island as constant
//! tables, so the engine never builds graphs at runtime.
//!
//! Geometry: pointy-top hexes in axial coordinates `(q, r)`, radius 2.
//! Positions use an exact integer lattice: a point `(x, y)` in lattice units
//! maps to world space as `(x * sqrt(3)/2, y / 2)` for unit hex size, with
//! `y` growing "down" (towards the bottom of the screen).
//!
//! Corner `i` of a tile goes clockwise starting at the top corner; edge `i`
//! joins corner `i` and corner `i + 1`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::{env, fs, path::Path};

const CORNER_OFFSETS: [(i32, i32); 6] = [(0, -2), (1, -1), (1, 1), (0, 2), (-1, 1), (-1, -1)];
/// Axial neighbour across edge `i` (NE, E, SE, SW, W, NW).
const EDGE_NEIGHBORS: [(i32, i32); 6] = [(1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1)];
/// Harbor slots around the coast: gaps of 3, 3, 4 coastal edges, repeated
/// three times, which gives the layout 3-fold rotational symmetry.
const HARBOR_COAST_SLOTS: [usize; 9] = [0, 3, 6, 10, 13, 16, 20, 23, 26];
const NONE: u8 = u8::MAX;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Tiles in reading order: top row first, left to right.
    let mut tiles: Vec<(i32, i32)> = Vec::new();
    for r in -2i32..=2 {
        for q in -2..=2 {
            if (q + r).abs() <= 2 {
                tiles.push((q, r));
            }
        }
    }
    assert_eq!(tiles.len(), 19);
    let tile_index = |q: i32, r: i32| tiles.iter().position(|&t| t == (q, r));
    let tile_center = |(q, r): (i32, i32)| (2 * q + r, 3 * r);

    // Collect unique corners, then index them in reading order (y, then x).
    let mut corner_set: BTreeMap<(i32, i32), ()> = BTreeMap::new();
    for &t in &tiles {
        let (cx, cy) = tile_center(t);
        for (dx, dy) in CORNER_OFFSETS {
            corner_set.insert((cy + dy, cx + dx), ());
        }
    }
    let vertices: Vec<(i32, i32)> = corner_set.keys().map(|&(y, x)| (x, y)).collect();
    assert_eq!(vertices.len(), 54);
    let vertex_index = |p: (i32, i32)| vertices.iter().position(|&v| v == p).unwrap();

    // Tile -> corner indices.
    let tile_vertices: Vec<[usize; 6]> = tiles
        .iter()
        .map(|&t| {
            let (cx, cy) = tile_center(t);
            let mut out = [0; 6];
            for (i, (dx, dy)) in CORNER_OFFSETS.iter().enumerate() {
                out[i] = vertex_index((cx + dx, cy + dy));
            }
            out
        })
        .collect();

    // Unique edges as sorted vertex pairs, indexed in sorted order.
    let mut edge_set: BTreeMap<(usize, usize), ()> = BTreeMap::new();
    for tv in &tile_vertices {
        for i in 0..6 {
            let (a, b) = (tv[i], tv[(i + 1) % 6]);
            edge_set.insert((a.min(b), a.max(b)), ());
        }
    }
    let edges: Vec<(usize, usize)> = edge_set.keys().copied().collect();
    assert_eq!(edges.len(), 72);
    let edge_index = |a: usize, b: usize| {
        edges
            .iter()
            .position(|&e| e == (a.min(b), a.max(b)))
            .unwrap()
    };

    let tile_edges: Vec<[usize; 6]> = tile_vertices
        .iter()
        .map(|tv| {
            let mut out = [0; 6];
            for i in 0..6 {
                out[i] = edge_index(tv[i], tv[(i + 1) % 6]);
            }
            out
        })
        .collect();

    let tile_neighbors: Vec<[u8; 6]> = tiles
        .iter()
        .map(|&(q, r)| {
            let mut out = [NONE; 6];
            for (i, (dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
                if let Some(n) = tile_index(q + dq, r + dr) {
                    out[i] = n as u8;
                }
            }
            out
        })
        .collect();

    // Vertex -> tiles, vertex -> (neighbour vertex, connecting edge).
    let mut vertex_tiles: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    for (t, tv) in tile_vertices.iter().enumerate() {
        for &v in tv {
            vertex_tiles[v].push(t);
        }
    }
    let mut vertex_links: Vec<Vec<(usize, usize)>> = vec![Vec::new(); vertices.len()];
    for (e, &(a, b)) in edges.iter().enumerate() {
        vertex_links[a].push((b, e));
        vertex_links[b].push((a, e));
    }

    // Edge -> tiles; coastal edges belong to exactly one tile.
    let mut edge_tile_count = vec![0usize; edges.len()];
    for te in &tile_edges {
        for &e in te {
            edge_tile_count[e] += 1;
        }
    }
    let mut coast: Vec<usize> = (0..edges.len())
        .filter(|&e| edge_tile_count[e] == 1)
        .collect();
    assert_eq!(coast.len(), 30);
    // Clockwise order starting just right of "12 o'clock".
    let angle = |e: usize| {
        let (a, b) = edges[e];
        let x = (vertices[a].0 + vertices[b].0) as f64 * 3f64.sqrt() / 2.0;
        let y = (vertices[a].1 + vertices[b].1) as f64 / 2.0;
        let mut ang = y.atan2(x) + std::f64::consts::FRAC_PI_2; // 0 at top, clockwise
        if ang < 0.0 {
            ang += std::f64::consts::TAU;
        }
        ang
    };
    coast.sort_by(|&a, &b| angle(a).partial_cmp(&angle(b)).unwrap());
    let harbor_edges: Vec<usize> = HARBOR_COAST_SLOTS.iter().map(|&s| coast[s]).collect();

    // ---- emit ----
    let mut s = String::new();
    s.push_str("// @generated by build.rs - do not edit.\n\n");
    let _ = writeln!(
        s,
        "/// Number of land tiles.\npub const NUM_TILES: usize = {};",
        tiles.len()
    );
    let _ = writeln!(
        s,
        "/// Number of intersections (building spots).\npub const NUM_VERTICES: usize = {};",
        vertices.len()
    );
    let _ = writeln!(
        s,
        "/// Number of paths (road spots).\npub const NUM_EDGES: usize = {};",
        edges.len()
    );
    let _ = writeln!(
        s,
        "/// Number of harbors.\npub const NUM_HARBORS: usize = {};",
        harbor_edges.len()
    );
    let _ = writeln!(
        s,
        "/// Number of coastal edges.\npub const NUM_COAST_EDGES: usize = {};\n",
        coast.len()
    );

    let list = |items: Vec<String>| items.join(", ");

    let _ = writeln!(
        s,
        "/// Axial `(q, r)` coordinates of each tile.\npub const TILE_AXIAL: [(i8, i8); NUM_TILES] = [{}];",
        list(tiles.iter().map(|(q, r)| format!("({q}, {r})")).collect())
    );
    let _ = writeln!(
        s,
        "/// Lattice position of each tile centre.\npub const TILE_POS: [(i16, i16); NUM_TILES] = [{}];",
        list(
            tiles
                .iter()
                .map(|&t| {
                    let (x, y) = tile_center(t);
                    format!("({x}, {y})")
                })
                .collect()
        )
    );
    let _ = writeln!(
        s,
        "/// Lattice position of each vertex.\npub const VERTEX_POS: [(i16, i16); NUM_VERTICES] = [{}];",
        list(
            vertices
                .iter()
                .map(|(x, y)| format!("({x}, {y})"))
                .collect()
        )
    );
    let arr6 = |a: &[usize; 6]| format!("[{}]", list(a.iter().map(|v| v.to_string()).collect()));
    let _ = writeln!(
        s,
        "/// Corners of each tile, clockwise from the top.\npub const TILE_VERTICES: [[u8; 6]; NUM_TILES] = [{}];",
        list(tile_vertices.iter().map(arr6).collect())
    );
    let _ = writeln!(
        s,
        "/// Sides of each tile; side `i` joins corners `i` and `i + 1`.\npub const TILE_EDGES: [[u8; 6]; NUM_TILES] = [{}];",
        list(tile_edges.iter().map(arr6).collect())
    );
    let _ = writeln!(
        s,
        "/// Neighbouring tile across each side, or `NONE`.\npub const TILE_NEIGHBORS: [[u8; 6]; NUM_TILES] = [{}];",
        list(
            tile_neighbors
                .iter()
                .map(|a| format!("[{}]", list(a.iter().map(|v| v.to_string()).collect())))
                .collect()
        )
    );
    let pad3 = |v: Vec<usize>| {
        let mut out = [NONE; 3];
        for (i, x) in v.into_iter().enumerate() {
            out[i] = x as u8;
        }
        format!("[{}]", list(out.iter().map(|x| x.to_string()).collect()))
    };
    let _ = writeln!(
        s,
        "/// Tiles touching each vertex (1-3), padded with `NONE`.\npub const VERTEX_TILES: [[u8; 3]; NUM_VERTICES] = [{}];",
        list(vertex_tiles.iter().map(|v| pad3(v.clone())).collect())
    );
    let _ = writeln!(
        s,
        "/// Adjacent vertices (2-3), padded with `NONE`. Index-aligned with `VERTEX_EDGES`.\npub const VERTEX_VERTICES: [[u8; 3]; NUM_VERTICES] = [{}];",
        list(
            vertex_links
                .iter()
                .map(|v| pad3(v.iter().map(|&(n, _)| n).collect()))
                .collect()
        )
    );
    let _ = writeln!(
        s,
        "/// Edges at each vertex (2-3), padded with `NONE`. `VERTEX_EDGES[v][k]` joins `v` and `VERTEX_VERTICES[v][k]`.\npub const VERTEX_EDGES: [[u8; 3]; NUM_VERTICES] = [{}];",
        list(
            vertex_links
                .iter()
                .map(|v| pad3(v.iter().map(|&(_, e)| e).collect()))
                .collect()
        )
    );
    let _ = writeln!(
        s,
        "/// Endpoints of each edge.\npub const EDGE_VERTICES: [[u8; 2]; NUM_EDGES] = [{}];",
        list(edges.iter().map(|(a, b)| format!("[{a}, {b}]")).collect())
    );
    let _ = writeln!(
        s,
        "/// Bitmask of the vertices adjacent to each vertex.\npub const VERTEX_ADJ_MASK: [u64; NUM_VERTICES] = [{}];",
        list(
            vertex_links
                .iter()
                .map(|v| format!("{:#x}", v.iter().fold(0u64, |m, &(n, _)| m | 1 << n)))
                .collect()
        )
    );
    let _ = writeln!(
        s,
        "/// Bitmask of the edges touching each vertex.\npub const VERTEX_EDGE_MASK: [u128; NUM_VERTICES] = [{}];",
        list(
            vertex_links
                .iter()
                .map(|v| format!("{:#x}", v.iter().fold(0u128, |m, &(_, e)| m | 1 << e)))
                .collect()
        )
    );
    let _ = writeln!(
        s,
        "/// Bitmask of the vertices of each tile.\npub const TILE_VERTEX_MASK: [u64; NUM_TILES] = [{}];",
        list(
            tile_vertices
                .iter()
                .map(|tv| format!("{:#x}", tv.iter().fold(0u64, |m, &v| m | 1 << v)))
                .collect()
        )
    );
    let _ = writeln!(
        s,
        "/// Coastal edges, clockwise from the top.\npub const COAST_EDGES: [u8; NUM_COAST_EDGES] = [{}];",
        list(coast.iter().map(|e| e.to_string()).collect())
    );
    let _ = writeln!(
        s,
        "/// Coastal edge occupied by each harbor slot.\npub const HARBOR_EDGES: [u8; NUM_HARBORS] = [{}];",
        list(harbor_edges.iter().map(|e| e.to_string()).collect())
    );

    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("topology_gen.rs");
    fs::write(out, s).unwrap();
}
