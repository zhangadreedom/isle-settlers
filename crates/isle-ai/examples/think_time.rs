//! Measures how long a bot takes per decision (native build).
//!
//! ```text
//! cargo run --release -p isle-ai --example think_time -- mcts 4 [players] [archipelago]
//! ```

use std::time::Instant;

use isle_ai::bot_by_name;
use isle_core::{GameConfig, GameState, Scenario};

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "mcts".into());
    let games: u64 = args.next().and_then(|g| g.parse().ok()).unwrap_or(4);
    let players: u8 = args.next().and_then(|p| p.parse().ok()).unwrap_or(4);
    let scenario = match args.next().as_deref() {
        Some("archipelago") => Scenario::Archipelago,
        _ => Scenario::Classic,
    };
    let mut times = Vec::new();
    let mut legal = Vec::new();
    for seed in 0..games {
        let mut g = GameState::new(GameConfig::scenario(scenario, players), seed).unwrap();
        let mut bots: Vec<_> = (0..players as u64)
            .map(|k| bot_by_name(&name, seed * 8 + k).expect("known bot"))
            .collect();
        while let Some(p) = g.to_move() {
            g.legal_actions(p, &mut legal);
            let t = Instant::now();
            let a = bots[p as usize].choose(&g, p, &legal);
            if legal.len() > 1 {
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            g.apply(p, a).unwrap();
        }
    }
    times.sort_by(f64::total_cmp);
    let q = |p: f64| times[((times.len() - 1) as f64 * p) as usize];
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    println!(
        "{name} ({players} players, {scenario:?}): {} searched decisions; mean {mean:.1} ms, median {:.1} ms, p90 {:.1} ms, max {:.1} ms",
        times.len(),
        q(0.5),
        q(0.9),
        q(1.0)
    );
}
