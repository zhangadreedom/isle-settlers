//! Generates board topologies as static tables, so the engine never builds
//! graphs at runtime:
//!
//! * `BASE`: the 19-tile island (rows of 3-4-5-4-3), 3–4 players.
//! * `EXTENDED`: the 30-tile island for 5–6 players (rows 3-4-5-6-5-4-3).
//! * `ARCHIPELAGO`: the seafaring map, drawn below as text. Its tiles include
//!   the sea between the islands; `ARCHIPELAGO_MAP` keeps one character per
//!   tile (`.` sea, `~` sea where the pirate starts, a letter per island,
//!   `M` being the home island) for the board to interpret.
//! * `FOG_ISLANDS`: a crescent home island in the west separated by a sea
//!   strait from thirteen unexplored fog tiles (`?`) in the east.
//!
//! Geometry: pointy-top hexes on an exact integer lattice. A point `(x, y)`
//! in lattice units maps to world space as `(x * sqrt(3)/2, y / 2)` for unit
//! hex size, with `y` growing "down". Neighbouring tile centres differ by
//! `(±2, 0)` or `(±1, ±3)`.
//!
//! Corner `i` of a tile goes clockwise starting at the top corner; edge `i`
//! joins corner `i` and corner `i + 1`. Tiles are numbered in reading order,
//! vertices by `(y, x)` and edges by their sorted vertex pair; the base board
//! keeps exactly the numbering it always had.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::{env, fs, path::Path};

const CORNER_OFFSETS: [(i32, i32); 6] = [(0, -2), (1, -1), (1, 1), (0, 2), (-1, 1), (-1, -1)];
/// Neighbouring tile centre across edge `i` (NE, E, SE, SW, W, NW).
const EDGE_NEIGHBORS: [(i32, i32); 6] = [(1, -3), (2, 0), (1, 3), (-1, 3), (-2, 0), (-1, -3)];
const NONE: u8 = u8::MAX;

enum Shape {
    /// Centred rows of land tiles with these lengths.
    Rows(&'static [i32]),
    /// One string per row; character `i` is a tile at lattice `x = i` (the
    /// map is centred afterwards), a space is no tile.
    Map(&'static [&'static str]),
}

enum Harbors {
    /// `n` harbors spread evenly along the clockwise coast.
    Spread(usize),
    /// Indices into the clockwise coast.
    Coast(&'static [usize]),
    /// `(tile, side)` pairs: side `i` joins corners `i` and `i + 1`
    /// (0 = north-east, 1 = east, ... 5 = north-west).
    Sides(&'static [(usize, usize)]),
}

struct Spec {
    name: &'static str,
    shape: Shape,
    harbors: Harbors,
}

/// The archipelago: a home island in the west and two small islands in the
/// east, two sea tiles away (at least three edges between home and either
/// island), the pirate between the small ones.
const ARCHIPELAGO_ROWS: &[&str] = &[
    "M M M . . b b",
    " M M M . . b",
    "M M M . . . ~",
    " M M M . . c",
    "M M M . . c c",
];

/// Fog islands: a crescent home island in the west around a sheltered bay,
/// a sea channel with the pirate at its mouth, and thirteen unexplored fog
/// tiles (`?`) to the east.
const FOG_ISLANDS_ROWS: &[&str] = &[
    "M M M . ? ? ?",
    " M M M . ? ?",
    "M M . ~ ? ? ?",
    " M M M . ? ?",
    "M M M . ? ? ?",
];

const SPECS: [Spec; 4] = [
    // Gaps of 3, 3, 4 coastal edges, repeated three times.
    Spec {
        name: "BASE",
        shape: Shape::Rows(&[3, 4, 5, 4, 3]),
        harbors: Harbors::Coast(&[0, 3, 6, 10, 13, 16, 20, 23, 26]),
    },
    Spec {
        name: "EXTENDED",
        shape: Shape::Rows(&[3, 4, 5, 6, 5, 4, 3]),
        harbors: Harbors::Spread(11),
    },
    // Seven around the home island, one on each small island.
    Spec {
        name: "ARCHIPELAGO",
        shape: Shape::Map(ARCHIPELAGO_ROWS),
        harbors: Harbors::Sides(&[
            (1, 0),
            (9, 1),
            (22, 1),
            (28, 2),
            (26, 3),
            (13, 4),
            (0, 5),
            (6, 0),
            (32, 2),
        ]),
    },
    // Nine symmetric harbors around the crescent home island.
    Spec {
        name: "FOG_ISLANDS",
        shape: Shape::Map(FOG_ISLANDS_ROWS),
        harbors: Harbors::Sides(&[
            (0, 0),
            (2, 0),
            (9, 1),
            (14, 1),
            (22, 1),
            (28, 2),
            (26, 2),
            (20, 4),
            (7, 4),
        ]),
    },
];

/// Tile centres in reading order, with their map characters.
fn tiles_of(shape: &Shape) -> (Vec<(i32, i32)>, Vec<u8>) {
    match shape {
        Shape::Rows(rows) => {
            let mid = (rows.len() as i32 - 1) / 2;
            let mut tiles = Vec::new();
            for (i, &count) in rows.iter().enumerate() {
                for k in 0..count {
                    tiles.push((-(count - 1) + 2 * k, 3 * (i as i32 - mid)));
                }
            }
            let chars = vec![b'L'; tiles.len()];
            (tiles, chars)
        }
        Shape::Map(rows) => {
            let mid = (rows.len() as i32 - 1) / 2;
            let mut tiles = Vec::new();
            let mut chars = Vec::new();
            for (r, line) in rows.iter().enumerate() {
                for (i, ch) in line.bytes().enumerate() {
                    if ch != b' ' {
                        tiles.push((i as i32, 3 * (r as i32 - mid)));
                        chars.push(ch);
                    }
                }
            }
            let (lo, hi) = tiles.iter().fold((i32::MAX, i32::MIN), |(lo, hi), t| {
                (lo.min(t.0), hi.max(t.0))
            });
            let shift = (lo + hi).div_euclid(2);
            for t in &mut tiles {
                t.0 -= shift;
            }
            // Neighbouring rows interleave.
            let parity = |t: &(i32, i32)| (t.0 + t.1 / 3).rem_euclid(2);
            assert!(
                tiles.iter().all(|t| parity(t) == parity(&tiles[0])),
                "map rows must interleave"
            );
            (tiles, chars)
        }
    }
}

fn is_sea(ch: u8) -> bool {
    ch == b'.' || ch == b'~'
}

fn is_fog(ch: u8) -> bool {
    ch == b'?'
}

fn is_land(ch: u8) -> bool {
    !is_sea(ch) && !is_fog(ch)
}

fn generate(spec: &Spec, out: &mut String) -> (usize, usize, usize, usize) {
    let (tiles, chars) = tiles_of(&spec.shape);
    let tile_at = |p: (i32, i32)| tiles.iter().position(|&t| t == p);

    let corners: BTreeSet<(i32, i32)> = tiles
        .iter()
        .flat_map(|&(cx, cy)| {
            CORNER_OFFSETS
                .iter()
                .map(move |(dx, dy)| (cy + dy, cx + dx))
        })
        .collect();
    let vertices: Vec<(i32, i32)> = corners.iter().map(|&(y, x)| (x, y)).collect();
    let vertex_at = |p: (i32, i32)| vertices.iter().position(|&v| v == p).unwrap();

    let tile_vertices: Vec<[usize; 6]> = tiles
        .iter()
        .map(|&(cx, cy)| CORNER_OFFSETS.map(|(dx, dy)| vertex_at((cx + dx, cy + dy))))
        .collect();
    let edge_set: BTreeSet<(usize, usize)> = tile_vertices
        .iter()
        .flat_map(|tv| {
            (0..6).map(move |i| (tv[i].min(tv[(i + 1) % 6]), tv[i].max(tv[(i + 1) % 6])))
        })
        .collect();
    let edges: Vec<(usize, usize)> = edge_set.into_iter().collect();
    let edge_at = |a: usize, b: usize| {
        edges
            .iter()
            .position(|&e| e == (a.min(b), a.max(b)))
            .unwrap()
    };
    let tile_edges: Vec<[usize; 6]> = tile_vertices
        .iter()
        .map(|tv| std::array::from_fn(|i| edge_at(tv[i], tv[(i + 1) % 6])))
        .collect();
    let tile_neighbors: Vec<[u8; 6]> = tiles
        .iter()
        .map(|&(x, y)| {
            EDGE_NEIGHBORS.map(|(dx, dy)| tile_at((x + dx, y + dy)).map_or(NONE, |t| t as u8))
        })
        .collect();

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

    // Coastal edges belong to one tile; order them clockwise from 12 o'clock.
    let mut edge_tiles = vec![0usize; edges.len()];
    for te in &tile_edges {
        for &e in te {
            edge_tiles[e] += 1;
        }
    }
    let mut coast: Vec<usize> = (0..edges.len()).filter(|&e| edge_tiles[e] == 1).collect();
    let angle = |e: usize| {
        let (a, b) = edges[e];
        let x = (vertices[a].0 + vertices[b].0) as f64 * 3f64.sqrt() / 2.0;
        let y = (vertices[a].1 + vertices[b].1) as f64 / 2.0;
        (y.atan2(x) + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU)
    };
    coast.sort_by(|&a, &b| angle(a).total_cmp(&angle(b)));
    let harbor_edges: Vec<usize> = match spec.harbors {
        Harbors::Coast(slots) => slots.iter().map(|&s| coast[s]).collect(),
        Harbors::Spread(n) => (0..n).map(|i| coast[i * coast.len() / n]).collect(),
        Harbors::Sides(sides) => sides
            .iter()
            .map(|&(t, side)| {
                // A harbor sits on known land and faces open water.
                assert!(is_land(chars[t]), "{}: harbor not on known land", spec.name);
                let n = tile_neighbors[t][side];
                assert!(
                    n == NONE || is_sea(chars[n as usize]),
                    "{}: harbor not facing open sea",
                    spec.name
                );
                tile_edges[t][side]
            })
            .collect(),
    };
    // Harbors never share an intersection.
    let mut used = BTreeSet::new();
    for &e in &harbor_edges {
        assert!(
            used.insert(edges[e].0) && used.insert(edges[e].1),
            "{}: harbors touch",
            spec.name
        );
    }

    let list = |items: Vec<String>| items.join(", ");
    let pad3 = |v: Vec<usize>| {
        let mut a = [NONE; 3];
        for (i, x) in v.into_iter().enumerate() {
            a[i] = x as u8;
        }
        format!("[{}, {}, {}]", a[0], a[1], a[2])
    };
    let arr6 = |a: &[usize; 6]| format!("[{}]", list(a.iter().map(|v| v.to_string()).collect()));
    let mask = |bits: Vec<usize>| format!("{:#x}", bits.into_iter().fold(0u128, |m, b| m | 1 << b));

    let _ = writeln!(out, "pub static {}: Topology = Topology {{", spec.name);
    let _ = writeln!(
        out,
        "    num_tiles: {},\n    num_vertices: {},\n    num_edges: {},",
        tiles.len(),
        vertices.len(),
        edges.len()
    );
    let _ = writeln!(
        out,
        "    tile_pos: &[{}],",
        list(tiles.iter().map(|(x, y)| format!("({x}, {y})")).collect())
    );
    let _ = writeln!(
        out,
        "    vertex_pos: &[{}],",
        list(
            vertices
                .iter()
                .map(|(x, y)| format!("({x}, {y})"))
                .collect()
        )
    );
    let _ = writeln!(
        out,
        "    tile_vertices: &[{}],",
        list(tile_vertices.iter().map(arr6).collect())
    );
    let _ = writeln!(
        out,
        "    tile_edges: &[{}],",
        list(tile_edges.iter().map(arr6).collect())
    );
    let _ = writeln!(
        out,
        "    tile_neighbors: &[{}],",
        list(
            tile_neighbors
                .iter()
                .map(|a| format!("[{}]", list(a.iter().map(|v| v.to_string()).collect())))
                .collect()
        )
    );
    let _ = writeln!(
        out,
        "    vertex_tiles: &[{}],",
        list(vertex_tiles.iter().map(|v| pad3(v.clone())).collect())
    );
    let _ = writeln!(
        out,
        "    vertex_vertices: &[{}],",
        list(
            vertex_links
                .iter()
                .map(|v| pad3(v.iter().map(|&(n, _)| n).collect()))
                .collect()
        )
    );
    let _ = writeln!(
        out,
        "    vertex_edges: &[{}],",
        list(
            vertex_links
                .iter()
                .map(|v| pad3(v.iter().map(|&(_, e)| e).collect()))
                .collect()
        )
    );
    let _ = writeln!(
        out,
        "    edge_vertices: &[{}],",
        list(edges.iter().map(|(a, b)| format!("[{a}, {b}]")).collect())
    );
    let _ = writeln!(
        out,
        "    vertex_adj_mask: &[{}],",
        list(
            vertex_links
                .iter()
                .map(|v| mask(v.iter().map(|&(n, _)| n).collect()))
                .collect()
        )
    );
    let _ = writeln!(
        out,
        "    vertex_edge_mask: &[{}],",
        list(
            vertex_links
                .iter()
                .map(|v| mask(v.iter().map(|&(_, e)| e).collect()))
                .collect()
        )
    );
    let _ = writeln!(
        out,
        "    tile_vertex_mask: &[{}],",
        list(tile_vertices.iter().map(|tv| mask(tv.to_vec())).collect())
    );
    let _ = writeln!(
        out,
        "    coast_edges: &[{}],",
        list(coast.iter().map(|e| e.to_string()).collect())
    );
    let _ = writeln!(
        out,
        "    harbor_edges: &[{}],",
        list(harbor_edges.iter().map(|e| e.to_string()).collect())
    );
    let _ = writeln!(
        out,
        "    all_vertices: {},",
        mask((0..vertices.len()).collect())
    );
    let _ = writeln!(out, "    all_edges: {},", mask((0..edges.len()).collect()));
    let _ = writeln!(out, "}};\n");
    if let Shape::Map(_) = spec.shape {
        check_islands(spec.name, &tiles, &chars, &tile_neighbors, &tile_vertices);
        let _ = writeln!(
            out,
            "/// Map characters of `{0}`, one per tile.\npub static {0}_MAP: &[u8] = b\"{1}\";\n",
            spec.name,
            String::from_utf8(chars).unwrap()
        );
    }
    (tiles.len(), vertices.len(), edges.len(), harbor_edges.len())
}

/// Each island letter is one connected piece of land, and different
/// islands (as well as initial land and fog) share no intersection.
fn check_islands(
    name: &str,
    tiles: &[(i32, i32)],
    chars: &[u8],
    neighbors: &[[u8; 6]],
    tile_vertices: &[[usize; 6]],
) {
    let land: Vec<usize> = (0..tiles.len()).filter(|&t| is_land(chars[t])).collect();
    for &t in &land {
        for &n in &neighbors[t] {
            if n != NONE {
                assert!(!is_fog(chars[n as usize]), "{name}: land touches fog");
                if is_land(chars[n as usize]) {
                    assert_eq!(chars[t], chars[n as usize], "{name}: islands touch");
                }
            }
        }
    }
    let letters: BTreeSet<u8> = land.iter().map(|&t| chars[t]).collect();
    assert!(letters.contains(&b'M'), "{name}: no home island");
    for &letter in &letters {
        let group: Vec<usize> = land
            .iter()
            .copied()
            .filter(|&t| chars[t] == letter)
            .collect();
        let mut seen = vec![group[0]];
        let mut i = 0;
        while i < seen.len() {
            for &n in &neighbors[seen[i]] {
                let n = n as usize;
                if n != NONE as usize && chars[n] == letter && !seen.contains(&n) {
                    seen.push(n);
                }
            }
            i += 1;
        }
        assert_eq!(
            seen.len(),
            group.len(),
            "{name}: island {} is split",
            letter as char
        );
    }
    for &a in &land {
        for &b in &land {
            if chars[a] != chars[b] {
                assert!(
                    tile_vertices[a]
                        .iter()
                        .all(|v| !tile_vertices[b].contains(v)),
                    "{name}: islands share an intersection"
                );
            }
        }
    }
    assert_eq!(
        chars.iter().filter(|&&c| c == b'~').count(),
        1,
        "{name}: one pirate start"
    );
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let mut out = String::from("// @generated by build.rs - do not edit.\n\n");
    let mut max = (0, 0, 0, 0);
    for spec in &SPECS {
        let (t, v, e, h) = generate(spec, &mut out);
        if spec.name == "BASE" {
            assert_eq!(
                (t, v, e),
                (19, 54, 72),
                "the base board must keep its shape"
            );
        }
        assert!(v <= 128 && e <= 128, "{}: masks are u128", spec.name);
        max = (max.0.max(t), max.1.max(v), max.2.max(e), max.3.max(h));
    }
    let _ = writeln!(
        out,
        "/// Largest tile count of any board.\npub const MAX_TILES: usize = {};",
        max.0
    );
    let _ = writeln!(
        out,
        "/// Largest vertex count of any board.\npub const MAX_VERTICES: usize = {};",
        max.1
    );
    let _ = writeln!(
        out,
        "/// Largest edge count of any board.\npub const MAX_EDGES: usize = {};",
        max.2
    );
    let _ = writeln!(
        out,
        "/// Largest harbor count of any board.\npub const MAX_HARBORS: usize = {};",
        max.3
    );
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("topology_gen.rs"),
        out,
    )
    .unwrap();
}
