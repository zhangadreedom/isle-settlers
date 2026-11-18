//! Generates board topologies as static tables, so the engine never builds
//! graphs at runtime:
//!
//! * `BASE`: the 19-tile island (rows of 3-4-5-4-3), 3–4 players.
//! * `EXTENDED`: the 30-tile island for 5–6 players (rows 3-4-5-6-5-4-3).
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

struct Spec {
    name: &'static str,
    rows: &'static [i32],
    harbors: usize,
    /// Fixed harbor positions (indices into the clockwise coast); `None`
    /// spreads `harbors` evenly.
    harbor_slots: Option<&'static [usize]>,
}

const SPECS: [Spec; 2] = [
    // Gaps of 3, 3, 4 coastal edges, repeated three times.
    Spec {
        name: "BASE",
        rows: &[3, 4, 5, 4, 3],
        harbors: 9,
        harbor_slots: Some(&[0, 3, 6, 10, 13, 16, 20, 23, 26]),
    },
    Spec {
        name: "EXTENDED",
        rows: &[3, 4, 5, 6, 5, 4, 3],
        harbors: 11,
        harbor_slots: None,
    },
];

fn generate(spec: &Spec, out: &mut String) -> (usize, usize, usize) {
    // Tile centres in reading order.
    let mid = (spec.rows.len() as i32 - 1) / 2;
    let mut tiles: Vec<(i32, i32)> = Vec::new();
    for (i, &count) in spec.rows.iter().enumerate() {
        for k in 0..count {
            tiles.push((-(count - 1) + 2 * k, 3 * (i as i32 - mid)));
        }
    }
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
    let slots: Vec<usize> = match spec.harbor_slots {
        Some(s) => s.to_vec(),
        None => (0..spec.harbors)
            .map(|i| i * coast.len() / spec.harbors)
            .collect(),
    };
    assert_eq!(slots.len(), spec.harbors);
    let harbor_edges: Vec<usize> = slots.iter().map(|&s| coast[s]).collect();
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
    (tiles.len(), vertices.len(), edges.len())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let mut out = String::from("// @generated by build.rs - do not edit.\n\n");
    let mut max = (0, 0, 0);
    for spec in &SPECS {
        let (t, v, e) = generate(spec, &mut out);
        if spec.name == "BASE" {
            assert_eq!(
                (t, v, e),
                (19, 54, 72),
                "the base board must keep its shape"
            );
        }
        assert!(v <= 128 && e <= 128, "{}: masks are u128", spec.name);
        max = (max.0.max(t), max.1.max(v), max.2.max(e));
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
        SPECS.iter().map(|s| s.harbors).max().unwrap()
    );
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("topology_gen.rs"),
        out,
    )
    .unwrap();
}
