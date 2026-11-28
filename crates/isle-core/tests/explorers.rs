//! Tests for `Scenario::Explorers`（「探索者与海盗任务」):
//! * `Archipelago` (2–4p) / `ArchipelagoExtended` (5–6p) layouts, default `15` VP
//! * Deterministic `init_explorer_sites` (`pirate_lairs`, `deep_shoals`, `spice_islands`,
//!   plus single-island custom map fallback)
//! * Non-7 dry-roll compensation gold (`+1` coin) and `SpendCoinsResource` (`2` coins -> `1` resource)
//! * `RaidPirateLair`, `HaulFishShoal`, `TradeSpice` (codecs `37..=39`), Champion crowns (`+1` VP),
//!   Spice Lv1 (`+1` extra mission coin) and Spice Lv2 (`3:1` universal maritime ratio)

use isle_core::resources::cost;
use isle_core::topology::{ARCHIPELAGO, bits, edges_in, vertices_in};
use isle_core::{
    Action, Board, BoardLayout, CustomMap, Event, GameConfig, GameState, MAX_COINS, Phase,
    Resource, ResourceSet, RuleError, Scenario, Terrain,
};

fn explorers_game(n: u8) -> GameState {
    GameState::new(GameConfig::scenario(Scenario::Explorers, n), 42).unwrap()
}

fn explorers_main(n: u8) -> GameState {
    let mut g = explorers_game(n);
    g.setup_step = 2 * n;
    g.phase = Phase::Main;
    g.dice = Some((3, 3));
    g
}

#[test]
fn config_defaults_and_site_generation_including_custom_single_island() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<GameState>();
    assert_copy::<Board>();
    assert_copy::<GameConfig>();
    assert_copy::<CustomMap>();

    let c4 = GameConfig::scenario(Scenario::Explorers, 4);
    assert_eq!(c4.victory_points, 15);
    assert_eq!(c4.scenario.max_players(), 6);
    assert_eq!(c4.layout(), BoardLayout::Archipelago);

    let c6 = GameConfig::scenario(Scenario::Explorers, 6);
    assert_eq!(c6.victory_points, 15);
    assert_eq!(c6.layout(), BoardLayout::ArchipelagoExtended);

    let g4 = explorers_game(4);
    assert_eq!(g4.board.pirate_lairs.count_ones(), 3);
    assert_eq!(g4.board.deep_shoals.count_ones(), 3);
    assert!((g4.board.pirate_lairs & g4.board.deep_shoals).is_empty());
    assert!(!g4.board.pirate_lairs.contains(g4.board.pirate_start));
    assert!(!g4.board.deep_shoals.contains(g4.board.pirate_start));
    assert!(g4.board.spice_islands.count_ones() >= 3);
    for t in bits(g4.board.spice_islands) {
        assert!(g4.board.tile_island[t as usize] >= 2);
        assert!(g4.board.terrain[t as usize].is_land());
    }
    g4.check_invariants().unwrap();

    let g6 = explorers_game(6);
    assert_eq!(g6.board.pirate_lairs.count_ones(), 4);
    assert_eq!(g6.board.deep_shoals.count_ones(), 4);
    assert!((g6.board.pirate_lairs & g6.board.deep_shoals).is_empty());
    assert!(g6.board.spice_islands.count_ones() >= 3);
    g6.check_invariants().unwrap();

    // Non-Explorers boards keep empty mission masks.
    let arch = GameState::new(GameConfig::scenario(Scenario::Archipelago, 4), 42).unwrap();
    assert!(arch.board.pirate_lairs.is_empty());
    assert!(arch.board.deep_shoals.is_empty());
    assert!(arch.board.spice_islands.is_empty());

    // Custom single-island seafaring map: no outer islands (`tile_island >= 2`),
    // so `init_explorer_sites` marks 3 coastal land tiles farthest from `tile_pos[0]`.
    let mut single = CustomMap::from_board(&arch.board);
    for t in 0..ARCHIPELAGO.num_tiles {
        if arch.board.tile_island[t] >= 2 {
            single.terrain[t] = Terrain::Sea;
            single.numbers[t] = 0;
        }
    }
    let gs = GameState::new(
        GameConfig {
            custom_map: Some(single),
            ..GameConfig::scenario(Scenario::Explorers, 4)
        },
        9,
    )
    .unwrap();
    assert_eq!(gs.board.island_count(), 1);
    assert_eq!(gs.board.spice_islands.count_ones(), 3);
    for t in bits(gs.board.spice_islands) {
        assert!(gs.board.terrain[t as usize].is_land());
        assert!(!(ARCHIPELAGO.tile_edge_mask(t) & gs.board.sea_edges).is_empty());
    }
    gs.check_invariants().unwrap();
}

#[test]
fn dry_roll_compensation_gold_and_spend_coins_without_wealthiest() {
    let mut g = explorers_game(3);
    g.setup_step = 6;
    g.phase = Phase::PreRoll;
    g.current = 0;

    // Pick a producing non-gold land tile and put seat 0's settlement on it, while seats 1 & 2
    // have no buildings at all. Force an event_deck roll matching that tile's number!
    let prod_tile = (0..ARCHIPELAGO.num_tiles as u8)
        .find(|&t| {
            t != g.robber
                && g.board.terrain[t as usize].resource().is_some()
                && g.board.numbers[t as usize] != 0
        })
        .unwrap();
    let target_sum = g.board.numbers[prod_tile as usize];
    let v0 = vertices_in(ARCHIPELAGO.tile_vertex_mask[prod_tile as usize])
        .next()
        .unwrap();
    g.settlements[0].insert(v0);
    g.players[0].settlements_left -= 1;

    let d1 = target_sum.saturating_sub(6).max(1);
    let d2 = target_sum - d1;
    g.config.event_deck = true;
    g.dice_deck = isle_core::CANONICAL_DICE_DECK;
    g.dice_deck[9] = (d1, d2);
    g.dice_deck_len = 10;

    let mut events = Vec::new();
    g.apply_with(0, Action::RollDice, &mut events).unwrap();
    assert_eq!(g.players[0].hand.total(), 1);
    assert_eq!(g.players[0].coins, 0, "producer gets 0 compensation gold");
    assert_eq!(g.players[1].coins, 1, "dry seat 1 gets +1 compensation gold");
    assert_eq!(g.players[2].coins, 1, "dry seat 2 gets +1 compensation gold");
    assert!(events.contains(&Event::CoinsGained {
        player: 1,
        amount: 1
    }));
    assert!(events.contains(&Event::CoinsGained {
        player: 2,
        amount: 1
    }));

    // Give seat 0 four coins: `wealthiest_owner` must remain `None` in Explorers,
    // and `SpendCoinsResource` converts 2 coins -> 1 chosen resource!
    g.players[0].coins = 4;
    events.clear();
    g.apply_with(0, Action::SpendCoinsResource(Resource::Ore), &mut events)
        .unwrap();
    assert_eq!(g.players[0].coins, 2);
    assert_eq!(g.wealthiest_owner, None);
    assert!(events.contains(&Event::CoinsSpent {
        player: 0,
        cost: 2,
        resource: Resource::Ore
    }));
    g.players[0].coins = MAX_COINS;
    g.check_invariants().unwrap();
}

#[test]
fn missions_spice_perks_champions_and_action_codecs() {
    let mut g = explorers_main(4);
    let lair = bits(g.board.pirate_lairs).next().unwrap();
    let shoal = bits(g.board.deep_shoals).next().unwrap();
    let spice = bits(g.board.spice_islands).next().unwrap();

    // Without ships or buildings, all 3 mission actions fail with NotConnected (when funded).
    let fund = ResourceSet::new(0, 4, 8, 8, 4);
    g.bank -= fund;
    g.players[0].hand += fund;
    assert_eq!(g.check(0, Action::RaidPirateLair(lair)), Err(RuleError::NotConnected));
    assert_eq!(g.check(0, Action::HaulFishShoal(shoal)), Err(RuleError::NotConnected));
    assert_eq!(g.check(0, Action::TradeSpice(spice)), Err(RuleError::NotConnected));

    // Place ships for seat 0 on `lair` and `shoal`, and a settlement on `spice`.
    let lair_edge = edges_in(ARCHIPELAGO.tile_edge_mask(lair)).next().unwrap();
    let shoal_edge = edges_in(ARCHIPELAGO.tile_edge_mask(shoal) & ! isle_core::BitSet::bit(lair_edge))
        .next()
        .unwrap();
    let spice_vert = vertices_in(ARCHIPELAGO.tile_vertex_mask[spice as usize])
        .next()
        .unwrap();
    g.ships[0].insert(lair_edge);
    g.ships[0].insert(shoal_edge);
    g.players[0].ships_left -= 2;
    g.settlements[0].insert(spice_vert);
    g.players[0].settlements_left -= 1;
    g.players[0].longest_road = g.compute_longest_road(0);

    // Before Spice Lv1, RaidPirateLair grants 2 coins and +1 VP.
    let mut events = Vec::new();
    g.apply_with(0, Action::RaidPirateLair(lair), &mut events).unwrap();
    assert_eq!(g.players[0].explorer_missions, [1, 0, 0]);
    assert_eq!(g.players[0].coins, 2);
    assert!(events.contains(&Event::ExplorerMission {
        player: 0,
        mission: 0,
        step: 1,
        champion_claimed: false,
    }));

    // Blocked while an open trade offer is pending in Main!
    g.apply(
        0,
        Action::ProposeTrade {
            give: ResourceSet::single(Resource::Wool, 1),
            get: ResourceSet::single(Resource::Brick, 1),
        },
    )
    .unwrap();
    assert_eq!(g.check(0, Action::TradeSpice(spice)), Err(RuleError::WrongPhase));
    g.apply(0, Action::CancelTrade).unwrap();

    // TradeSpice step 1 -> grants 3 coins and unlocks Spice Lv1 (+1 extra coin on ALL missions!).
    assert_eq!(g.trade_ratio(0, Resource::Brick), 4);
    g.apply(0, Action::TradeSpice(spice)).unwrap();
    assert_eq!(g.players[0].explorer_missions, [1, 0, 1]);
    assert_eq!(g.players[0].coins, 5);

    // HaulFishShoal now grants 3 coins instead of 2 thanks to Spice Lv1!
    g.apply(0, Action::HaulFishShoal(shoal)).unwrap();
    assert_eq!(g.players[0].explorer_missions, [1, 1, 1]);
    assert_eq!(g.players[0].coins, 8);

    // TradeSpice step 2 -> unlocks Spice Lv2 (3:1 universal maritime ratio!).
    g.apply(0, Action::TradeSpice(spice)).unwrap();
    assert_eq!(g.players[0].explorer_missions, [1, 1, 2]);
    for r in Resource::ALL {
        assert_eq!(g.trade_ratio(0, r), 3);
    }

    // TradeSpice step 3 -> claims Champion crown on track 2 (+1 extra VP)!
    events.clear();
    g.apply_with(0, Action::TradeSpice(spice), &mut events).unwrap();
    assert_eq!(g.players[0].explorer_missions, [1, 1, 3]);
    assert_eq!(g.explorer_champions[2], Some(0));
    assert!(events.contains(&Event::ExplorerMission {
        player: 0,
        mission: 2,
        step: 3,
        champion_claimed: true,
    }));
    // Step 4 is rejected with NoPiecesLeft.
    assert_eq!(g.check(0, Action::TradeSpice(spice)), Err(RuleError::NoPiecesLeft));

    // Seat 1 reaching step 3 on track 2 later does NOT steal seat 0's Champion crown!
    let spice2 = bits(g.board.spice_islands).find(|&t| t != spice).unwrap();
    let sv2 = vertices_in(ARCHIPELAGO.tile_vertex_mask[spice2 as usize])
        .find(|&v| g.settlement_spots(1, false).contains(v))
        .unwrap();
    g.settlements[1].insert(sv2);
    g.players[1].settlements_left -= 1;
    let p1_fund = ResourceSet::new(0, 0, 3, 3, 0);
    g.bank -= p1_fund;
    g.players[1].hand += p1_fund;
    g.current = 1;
    for expected_step in 1..=3 {
        events.clear();
        g.apply_with(1, Action::TradeSpice(spice2), &mut events).unwrap();
        assert!(events.contains(&Event::ExplorerMission {
            player: 1,
            mission: 2,
            step: expected_step,
            champion_claimed: false,
        }));
    }
    assert_eq!(g.explorer_champions[2], Some(0));
    // Seat 0 VP: 1 settlement (1) + 5 mission steps (5) + 1 champion crown (1) = 7 VP.
    assert_eq!(g.public_victory_points(0), 7);
    // Seat 1 VP: 1 settlement (1) + 3 mission steps (3) + 0 champion crowns = 4 VP.
    assert_eq!(g.public_victory_points(1), 4);
    g.check_invariants().unwrap();

    // PairedTurn also allows Explorer missions!
    let mut g6 = explorers_game(6);
    g6.config.paired_turns = true;
    g6.setup_step = 12;
    g6.current = 0;
    g6.in_paired_turn = Some(3);
    g6.phase = Phase::PairedTurn { partner: 3 };
    let lair6 = bits(g6.board.pirate_lairs).next().unwrap();
    let le6 = edges_in(g6.topo().tile_edge_mask(lair6)).next().unwrap();
    g6.ships[3].insert(le6);
    g6.players[3].ships_left -= 1;
    g6.players[3].longest_road = g6.compute_longest_road(3);
    g6.bank -= cost::PIRATE_LAIR;
    g6.players[3].hand += cost::PIRATE_LAIR;
    g6.apply(3, Action::RaidPirateLair(lair6)).unwrap();
    assert_eq!(g6.players[3].explorer_missions[0], 1);
    g6.check_invariants().unwrap();

    // Codec tags 37, 38, 39 round-trip cleanly.
    for (tag, a) in [
        (37u8, Action::RaidPirateLair(5)),
        (38u8, Action::HaulFishShoal(12)),
        (39u8, Action::TradeSpice(29)),
    ] {
        let mut buf = Vec::new();
        a.encode(&mut buf);
        assert_eq!(buf[0], tag);
        assert_eq!(Action::decode(&buf), Some((a, 2)));
    }
}
