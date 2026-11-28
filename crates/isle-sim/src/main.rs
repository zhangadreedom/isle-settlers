//! Self-play simulator and engine benchmark.
//!
//! ```text
//! cargo run --release -p isle-sim -- --games 100000 --bot weighted --threads 4
//! cargo run --release -p isle-sim -- --games 2000 --bots greedy,weighted,weighted,weighted
//! cargo run --release -p isle-sim -- --games 500 --scenario archipelago --players 4 --bot greedy
//! ```

use std::time::Instant;

use isle_ai::{Bot, bot_by_name, play_game};
use isle_core::{GameConfig, GameState, MAX_PLAYERS, Scenario};

struct Args {
    games: u64,
    players: u8,
    scenario: Scenario,
    friendly_robber: bool,
    paired_turns: bool,
    event_deck: bool,
    seed: u64,
    threads: usize,
    /// Bot name per seat.
    bots: Vec<String>,
    max_turns: u32,
    check: bool,
    /// Rotate the lineup every game so no bot keeps the first seat.
    rotate: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        games: 20_000,
        players: 4,
        scenario: Scenario::Classic,
        friendly_robber: false,
        paired_turns: false,
        event_deck: false,
        seed: 1,
        threads: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        bots: Vec::new(),
        max_turns: 1000,
        check: false,
        rotate: false,
    };
    let mut bot = String::from("weighted");
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || {
            it.next()
                .unwrap_or_else(|| usage(&format!("missing value for {flag}")))
        };
        match flag.as_str() {
            "--games" => a.games = val().parse().unwrap_or_else(|_| usage("bad --games")),
            "--players" => a.players = val().parse().unwrap_or_else(|_| usage("bad --players")),
            "--seed" => a.seed = val().parse().unwrap_or_else(|_| usage("bad --seed")),
            "--threads" => a.threads = val().parse().unwrap_or_else(|_| usage("bad --threads")),
            "--bot" => bot = val(),
            "--bots" => a.bots = val().split(',').map(str::to_owned).collect(),
            "--max-turns" => {
                a.max_turns = val().parse().unwrap_or_else(|_| usage("bad --max-turns"))
            }
            "--scenario" => {
                a.scenario = match val().to_ascii_lowercase().as_str() {
                    "classic" => Scenario::Classic,
                    "archipelago" => Scenario::Archipelago,
                    "fog" | "fog-islands" | "fog_islands" | "fogislands" => Scenario::FogIslands,
                    "citadels" | "cities" | "ck" => Scenario::Citadels,
                    "fishermen" | "fish" => Scenario::Fishermen,
                    "caravans" | "camel" | "camels" => Scenario::Caravans,
                    _ => usage(
                        "--scenario must be classic, archipelago, fog, citadels, fishermen or caravans",
                    ),
                }
            }
            "--friendly-robber" => a.friendly_robber = true,
            "--paired-turns" => a.paired_turns = true,
            "--event-deck" => a.event_deck = true,
            "--check" => a.check = true,
            "--rotate" => a.rotate = true,
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown flag {other}")),
        }
    }
    if !(2..=a.scenario.max_players()).contains(&a.players) {
        usage("--players must be 2-6");
    }
    if a.bots.is_empty() {
        a.bots = vec![bot; a.players as usize];
    }
    if a.bots.len() != a.players as usize {
        usage("--bots needs one name per seat");
    }
    if let Some(bad) = a.bots.iter().find(|b| bot_by_name(b, 0).is_none()) {
        usage(&format!("unknown bot {bad}"));
    }
    a.threads = a.threads.max(1);
    a
}

fn usage(err: &str) -> ! {
    if !err.is_empty() {
        eprintln!("error: {err}\n");
    }
    eprintln!(
        "usage: isle-sim [--games N] [--players 2-6] [--scenario classic|archipelago|fog|citadels|fishermen|caravans]\n\
         \x20               [--friendly-robber] [--paired-turns] [--event-deck] [--seed S] [--threads T]\n\
         \x20               [--bot NAME | --bots NAME,NAME,...] [--rotate] [--max-turns N] [--check]\n\
         bots: random, weighted, greedy, greedy:trade, mcts, mcts:N (with optional @road|@city|@warlord|@merchant)"
    );
    std::process::exit(if err.is_empty() { 0 } else { 2 });
}

#[derive(Default, Clone, Copy)]
struct Stats {
    games: u64,
    finished: u64,
    turns: u64,
    actions: u64,
    wins: [u64; MAX_PLAYERS],
    /// Wins per lineup entry (equals `wins` unless rotating).
    wins_by_bot: [u64; MAX_PLAYERS],
    /// Seafaring: island bonuses earned and ships on the board at the end,
    /// summed over players.
    islands: u64,
    ships: u64,
    /// Fog maps: tiles uncovered across all games.
    revealed: u64,
    /// Citadels: metropolises claimed, defender points earned and knights on board at the end.
    metropolises: u64,
    defender_points: u64,
    knights: u64,
    /// Caravans: camel segments placed, stone bridges built and final gold coins.
    camels: u64,
    bridges: u64,
    coins: u64,
}

fn run_shard(args: &Args, shard: u64, count: u64) -> Stats {
    let mut s = Stats::default();
    let config = GameConfig {
        friendly_robber: args.friendly_robber,
        paired_turns: args.paired_turns,
        event_deck: args.event_deck,
        ..GameConfig::scenario(args.scenario, args.players)
    };
    let fog_pool_len = config.layout().fog_terrain_pool().len() as u64;
    for i in 0..count {
        let game_seed = args
            .seed
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add(shard << 40 | i);
        let mut g = GameState::new(config, game_seed).expect("valid config");
        let n = args.players as usize;
        let shift = if args.rotate {
            (shard as usize + i as usize * 7) % n
        } else {
            0
        };
        let lineup = |seat: usize| (seat + shift) % n;
        let mut b: Vec<Box<dyn Bot + Send>> = (0..n)
            .map(|k| {
                bot_by_name(&args.bots[lineup(k)], game_seed ^ (k as u64 + 1)).expect("validated")
            })
            .collect();
        let mut bots: Vec<&mut dyn Bot> = b.iter_mut().map(|b| &mut **b as &mut dyn Bot).collect();
        let outcome = play_game(&mut g, &mut bots, args.max_turns, args.check);
        for p in 0..n {
            s.islands += g.players[p].islands.count_ones() as u64;
            s.ships += g.ships[p].count_ones() as u64;
            s.defender_points += g.players[p].defender_points as u64;
            s.bridges += g.bridges[p].count_ones() as u64;
            s.coins += g.players[p].coins as u64;
        }
        s.camels += g.camels.count_ones() as u64;
        if g.board.layout.has_fog() {
            s.revealed += fog_pool_len - g.fog_terrain_len as u64;
        }
        if args.scenario == Scenario::Citadels {
            s.metropolises += g
                .metropolis_owners
                .iter()
                .filter(|o| o.is_some())
                .count() as u64;
            s.knights += g.all_knights().count_ones() as u64;
        }
        s.games += 1;
        s.turns += outcome.turns as u64;
        s.actions += outcome.actions;
        if let Some(w) = outcome.winner {
            s.finished += 1;
            s.wins[w as usize] += 1;
            s.wins_by_bot[lineup(w as usize)] += 1;
        }
    }
    s
}

fn main() {
    let args = parse_args();
    let threads = args.threads as u64;
    let start = Instant::now();
    let shards: Vec<Stats> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let count = args.games / threads + u64::from(t < args.games % threads);
                let args = &args;
                scope.spawn(move || run_shard(args, t, count))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("shard panicked"))
            .collect()
    });
    let secs = start.elapsed().as_secs_f64();

    let mut total = Stats::default();
    for s in &shards {
        total.games += s.games;
        total.finished += s.finished;
        total.turns += s.turns;
        total.actions += s.actions;
        total.islands += s.islands;
        total.ships += s.ships;
        total.revealed += s.revealed;
        total.metropolises += s.metropolises;
        total.defender_points += s.defender_points;
        total.knights += s.knights;
        total.camels += s.camels;
        total.bridges += s.bridges;
        total.coins += s.coins;
        for p in 0..MAX_PLAYERS {
            total.wins[p] += s.wins[p];
            total.wins_by_bot[p] += s.wins_by_bot[p];
        }
    }
    let g = total.games.max(1) as f64;
    let layout = GameConfig::scenario(args.scenario, args.players).layout();
    println!("bots            {}", args.bots.join(", "));
    println!("players         {}", args.players);
    println!("scenario        {:?}", args.scenario);
    if args.friendly_robber || args.paired_turns || args.event_deck {
        let mut rules = Vec::new();
        if args.friendly_robber {
            rules.push("friendly-robber");
        }
        if args.paired_turns {
            rules.push("paired-turns");
        }
        if args.event_deck {
            rules.push("event-deck");
        }
        println!("house rules     {}", rules.join(", "));
    }
    println!("threads         {}", args.threads);
    println!(
        "invariants      {}",
        if args.check {
            "checked every action"
        } else {
            "off"
        }
    );
    println!("games           {}", total.games);
    println!(
        "finished        {} ({:.1}%)",
        total.finished,
        100.0 * total.finished as f64 / g
    );
    println!("wall time       {secs:.3} s");
    println!("games/s         {:.0}", total.games as f64 / secs);
    println!(
        "games/s/thread  {:.0}",
        total.games as f64 / secs / args.threads as f64
    );
    println!("actions/s       {:.0}", total.actions as f64 / secs);
    println!("avg turns       {:.1}", total.turns as f64 / g);
    println!("avg actions     {:.1}", total.actions as f64 / g);
    let per_player = g * args.players as f64;
    if layout.has_ships() {
        println!(
            "island bonuses  {:.2} per player",
            total.islands as f64 / per_player
        );
        println!(
            "ships at end    {:.2} per player",
            total.ships as f64 / per_player
        );
        if layout.has_fog() {
            println!(
                "fog revealed    {:.2} / {} tiles",
                total.revealed as f64 / g,
                layout.fog_terrain_pool().len()
            );
        }
    }
    if args.scenario == Scenario::Citadels {
        println!(
            "metropolises    {:.2} / 3 per game",
            total.metropolises as f64 / g
        );
        println!(
            "defender pts    {:.2} per player",
            total.defender_points as f64 / per_player
        );
        println!(
            "knights at end  {:.2} per player",
            total.knights as f64 / per_player
        );
    }
    if args.scenario == Scenario::Caravans {
        println!(
            "camels placed   {:.2} / {} per game",
            total.camels as f64 / g,
            isle_core::MAX_CAMELS
        );
        println!(
            "bridges built   {:.2} / {} per player",
            total.bridges as f64 / per_player,
            isle_core::MAX_BRIDGES
        );
        println!(
            "coins at end    {:.2} per player",
            total.coins as f64 / per_player
        );
    }
    let wins: Vec<String> = (0..args.players as usize)
        .map(|p| {
            format!(
                "{:.1}%",
                100.0 * total.wins[p] as f64 / total.finished.max(1) as f64
            )
        })
        .collect();
    println!("wins by seat    {}", wins.join("  "));
    // Win rate of each lineup entry with a 95% confidence interval.
    let f = total.finished.max(1) as f64;
    for (i, name) in args.bots.iter().enumerate() {
        let p = total.wins_by_bot[i] as f64 / f;
        let ci = 1.96 * (p * (1.0 - p) / f).sqrt();
        println!("  [{i}] {name:<28} {:5.1}% ± {:.1}", 100.0 * p, 100.0 * ci);
    }
}
