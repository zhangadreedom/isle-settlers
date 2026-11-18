//! Prints a hash of every action and final score over many bot games on the
//! base board. Used to check that engine refactors keep the base game
//! byte-for-byte identical (saved rooms and replays depend on it).
//!
//! ```text
//! cargo run --release -p isle-ai --example fingerprint
//! ```
//!
//! Expected output since P3: `b862d3b3842bae2d`. Change it only together
//! with a deliberate rule change (and a new replay/room format version).

use isle_ai::bot_by_name;
use isle_core::{GameConfig, GameState};

fn main() {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |s: &str| {
        for b in s.bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    };
    let mut legal = Vec::new();
    for (bot, games) in [("weighted", 300u64), ("greedy", 60), ("mcts:60", 6)] {
        for seed in 0..games {
            let mut g = GameState::new(GameConfig::default(), seed).unwrap();
            let mut bots: Vec<_> = (0..4)
                .map(|k| bot_by_name(bot, seed * 4 + k).unwrap())
                .collect();
            while let Some(p) = g.to_move() {
                if g.turn > 1000 {
                    break;
                }
                g.legal_actions(p, &mut legal);
                let a = bots[p as usize].choose(&g, p, &legal);
                feed(&format!("{p}{a:?}"));
                g.apply(p, a).unwrap();
            }
            let points: Vec<u8> = (0..4).map(|p| g.victory_points(p)).collect();
            feed(&format!("{points:?}{:?}", g.winner));
        }
    }
    println!("{hash:016x}");
}
