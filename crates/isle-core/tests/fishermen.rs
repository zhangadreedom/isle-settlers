//! Tests for Track 1 of v1.0:
//! 1. 36-card balanced dice deck (`GameConfig::event_deck`, New Year reshuffle at `<= 5`).
//! 2. `Scenario::Fishermen`: coastal fishing grounds, inland trout lake on 2/12,
//!    fish hand cap (`MAX_FISH_HAND = 7`), the Old Boot (`+1` VP required to win,
//!    `PassOldBoot`), and all five fish spending actions (`28..=33` replay codec).

use isle_core::board::PIPS;
use isle_core::topology::{BASE, EXTENDED, vertices_in};
use isle_core::{
    Action, Board, BoardLayout, CANONICAL_DICE_DECK, CustomMap, DICE_DECK_RESHUFFLE_AT,
    DICE_DECK_SIZE, DevCard, Event, FishingGround, GameConfig, GameState, MAX_FISH_HAND, Phase,
    Resource, ResourceSet, Rng, RuleError, Scenario,
};

fn fishermen_game(n: u8) -> GameState {
    GameState::new(GameConfig::scenario(Scenario::Fishermen, n), 42).unwrap()
}

fn fishermen_main(n: u8) -> GameState {
    let mut g = fishermen_game(n);
    g.setup_step = 2 * n;
    g.phase = Phase::Main;
    g.dice = Some((3, 3));
    g
}

#[test]
fn types_remain_copy() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<GameState>();
    assert_copy::<Board>();
    assert_copy::<GameConfig>();
    assert_copy::<CustomMap>();
    assert_copy::<FishingGround>();
}

#[test]
fn event_deck_deals_balanced_rolls_and_reshuffles_at_five_left() {
    let config = GameConfig {
        event_deck: true,
        ..GameConfig::new(2)
    };
    let mut g = GameState::new(config, 123).unwrap();
    assert_eq!(g.dice_deck_len as usize, DICE_DECK_SIZE);

    // Initial deck is an exact permutation of all 36 ordered (d1, d2) pairs.
    let mut sorted = g.dice_deck;
    sorted.sort_unstable();
    assert_eq!(sorted, CANONICAL_DICE_DECK);
    let mut sum_counts = [0u8; 13];
    for (d1, d2) in g.dice_deck {
        sum_counts[(d1 + d2) as usize] += 1;
    }
    assert_eq!(sum_counts, PIPS);

    // Advance past setup into PreRoll.
    g.setup_step = 4;
    g.phase = Phase::PreRoll;

    let initial_deck = g.dice_deck;
    let mut drawn = Vec::new();
    // First 31 rolls draw down from 36 -> 5 without reshuffling.
    for step in 0..31usize {
        let p = g.current;
        let expected = initial_deck[35 - step];
        let mut events = Vec::new();
        g.apply_with(p, Action::RollDice, &mut events).unwrap();
        assert_eq!(g.dice, Some(expected));
        assert_eq!(g.dice_deck_len, (35 - step) as u8);
        assert!(events.contains(&Event::DiceRolled {
            player: p,
            dice: expected,
        }));
        drawn.push(expected);
        // Resolve MoveRobber if 7 was rolled, then end turn.
        if g.phase == Phase::MoveRobber {
            let mut legal = Vec::new();
            g.legal_actions(p, &mut legal);
            g.apply(p, legal[0]).unwrap();
        }
        g.apply(p, Action::EndTurn).unwrap();
        g.check_invariants().unwrap();
    }
    assert_eq!(g.dice_deck_len, DICE_DECK_RESHUFFLE_AT);

    // Roll 32 (`dice_deck_len == 5 <= 5`) triggers the New Year reshuffle (36 -> 35)!
    let p = g.current;
    g.apply(p, Action::RollDice).unwrap();
    assert_eq!(g.dice_deck_len, 35);
    let mut reshuffled = g.dice_deck;
    reshuffled.sort_unstable();
    assert_eq!(reshuffled, CANONICAL_DICE_DECK);
    g.check_invariants().unwrap();
}

#[test]
fn coastal_fishing_grounds_populate_base_extended_and_custom_maps() {
    // Non-Fishermen boards keep 0 fishing grounds.
    let classic = GameState::new(GameConfig::default(), 7).unwrap();
    assert_eq!(classic.board.fishing_ground_count, 0);
    assert!(classic.board.active_fishing_grounds().is_empty());

    // 4p Base Fishermen: 6 non-harbor coastal edges with [4, 5, 6, 8, 9, 10].
    let f4 = fishermen_game(4);
    assert_eq!(f4.config.victory_points, 10);
    assert_eq!(f4.board.layout, BoardLayout::Base);
    assert_eq!(f4.board.fishing_ground_count, 6);
    let nums4: Vec<u8> = f4
        .board
        .active_fishing_grounds()
        .iter()
        .map(|fg| fg.number)
        .collect();
    assert_eq!(nums4, vec![4, 5, 6, 8, 9, 10]);
    for fg in f4.board.active_fishing_grounds() {
        assert!(BASE.coast_edges.contains(&fg.edge));
        assert!(!BASE.harbor_edges.contains(&fg.edge));
    }

    // 6p Extended Fishermen: 8 non-harbor coastal edges with [4, 5, 5, 6, 8, 9, 9, 10].
    let f6 = fishermen_game(6);
    assert_eq!(f6.board.layout, BoardLayout::Extended);
    assert_eq!(f6.board.fishing_ground_count, 8);
    let nums6: Vec<u8> = f6
        .board
        .active_fishing_grounds()
        .iter()
        .map(|fg| fg.number)
        .collect();
    assert_eq!(nums6, vec![4, 5, 5, 6, 8, 9, 9, 10]);
    for fg in f6.board.active_fishing_grounds() {
        assert!(EXTENDED.coast_edges.contains(&fg.edge));
        assert!(!EXTENDED.harbor_edges.contains(&fg.edge));
    }

    // CustomMap in Fishermen also receives deterministic fishing grounds.
    let cm = CustomMap::from_board(&classic.board);
    let custom_f = GameState::new(
        GameConfig {
            custom_map: Some(cm),
            ..GameConfig::scenario(Scenario::Fishermen, 4)
        },
        99,
    )
    .unwrap();
    assert_eq!(
        custom_f.board.active_fishing_grounds(),
        f4.board.active_fishing_grounds()
    );
}

#[test]
fn coastal_shoals_trout_lake_cap_and_old_boot_production() {
    let mut g = fishermen_main(2);
    let fg = g.board.active_fishing_grounds()[0];
    let [v_settle, v_city] = BASE.edge_vertices[fg.edge as usize];
    g.settlements[0].insert(v_settle);
    g.players[0].settlements_left -= 1;
    g.cities[1].insert(v_city);
    g.players[1].cities_left -= 1;

    // Force an exact roll of `fg.number` via event_deck with a non-zero rng.below(12).
    g.config.event_deck = true;
    g.dice_deck = [(1, 1); DICE_DECK_SIZE];
    let d1 = (fg.number / 2).clamp(1, 6);
    let d2 = fg.number - d1;
    g.dice_deck[9] = (d1, d2);
    g.dice_deck_len = 10;
    // Find a seed where neither seat draws the Old Boot (`below(12) != 0` twice).
    let no_boot_seed = (0u64..)
        .find(|&s| {
            let mut r = Rng::new(s);
            r.below(12) != 0 && r.below(12) != 0
        })
        .unwrap();
    g.rng = Rng::new(no_boot_seed);
    g.phase = Phase::PreRoll;
    let mut events = Vec::new();
    g.apply_with(0, Action::RollDice, &mut events).unwrap();
    assert_eq!(g.players[0].fish, 1);
    assert_eq!(g.players[1].fish, 2);
    assert!(events.contains(&Event::FishCaught {
        player: 0,
        count: 1,
    }));
    assert!(events.contains(&Event::FishCaught {
        player: 1,
        count: 2,
    }));
    assert_eq!(g.old_boot_owner, None);

    // Cap at MAX_FISH_HAND = 7: starting with 6 fish and catching 2 adds only 1.
    g.players[1].fish = 6;
    g.dice_deck[8] = (d1, d2);
    g.rng = Rng::new(no_boot_seed);
    g.phase = Phase::PreRoll;
    events.clear();
    g.apply_with(0, Action::RollDice, &mut events).unwrap();
    assert_eq!(g.players[1].fish, MAX_FISH_HAND);
    assert!(events.contains(&Event::FishCaught {
        player: 1,
        count: 1,
    }));

    // Trout Lake (`board.desert`) on roll 2 or 12: blocked when robber is on desert,
    // yields +1/+2 fish when robber has moved off desert!
    let mut lake = fishermen_main(2);
    lake.config.event_deck = true;
    lake.dice_deck = [(1, 1); DICE_DECK_SIZE];
    let desert = lake.board.desert;
    let lv = vertices_in(
        BASE.tile_vertex_mask[desert as usize]
            & !lake
                .board
                .active_fishing_grounds()
                .iter()
                .fold(isle_core::BitSet::EMPTY, |m, fg| {
                    let [a, b] = BASE.edge_vertices[fg.edge as usize];
                    m | isle_core::BitSet::bit(a) | isle_core::BitSet::bit(b)
                }),
    )
    .next()
    .unwrap();
    lake.cities[0].insert(lv);
    lake.players[0].cities_left -= 1;

    // Robber starts on desert -> rolling 2 (1, 1) yields 0 lake fish.
    lake.dice_deck[9] = (1, 1);
    lake.dice_deck_len = 10;
    lake.phase = Phase::PreRoll;
    lake.apply(0, Action::RollDice).unwrap();
    assert_eq!(lake.players[0].fish, 0);

    // Move robber off desert -> rolling 12 (6, 6) yields 2 fish from the city, and can draw Old Boot!
    lake.robber = (0..BASE.num_tiles as u8).find(|&t| t != desert).unwrap();
    lake.dice_deck[8] = (6, 6);
    let boot_seed = (0u64..).find(|&s| Rng::new(s).below(12) == 0).unwrap();
    lake.rng = Rng::new(boot_seed);
    lake.phase = Phase::PreRoll;
    events.clear();
    lake.apply_with(0, Action::RollDice, &mut events).unwrap();
    assert_eq!(lake.players[0].fish, 2);
    assert_eq!(lake.old_boot_owner, Some(0));
    assert!(events.contains(&Event::OldBootDrawn { player: 0 }));
    lake.check_invariants().unwrap();
}

#[test]
fn old_boot_requires_extra_vp_and_passing_it_can_win_immediately() {
    let mut g = fishermen_main(3);
    g.config.victory_points = 4;

    // Give seat 0 two cities = 4 public VP, seat 1 one city = 2 public VP, seat 2 two cities = 4 public VP.
    let spots: Vec<u8> = vertices_in(g.setup_spots()).collect();
    let v0 = spots[0];
    let v1 = *spots
        .iter()
        .find(|&&v| v != v0 && !BASE.vertex_adj_mask[v0 as usize].contains(v))
        .unwrap();
    g.cities[0].insert(v0);
    g.cities[0].insert(v1);
    g.players[0].cities_left -= 2;

    let p1_v = *spots
        .iter()
        .find(|&&v| {
            v != v0
                && v != v1
                && !BASE.vertex_adj_mask[v0 as usize].contains(v)
                && !BASE.vertex_adj_mask[v1 as usize].contains(v)
        })
        .unwrap();
    g.cities[1].insert(p1_v);
    g.players[1].cities_left -= 1;

    let p2_a = *spots
        .iter()
        .find(|&&v| {
            ![v0, v1, p1_v].contains(&v)
                && ![v0, v1, p1_v]
                    .iter()
                    .any(|&u| BASE.vertex_adj_mask[u as usize].contains(v))
        })
        .unwrap();
    let p2_b = *spots
        .iter()
        .find(|&&v| {
            ![v0, v1, p1_v, p2_a].contains(&v)
                && ![v0, v1, p1_v, p2_a]
                    .iter()
                    .any(|&u| BASE.vertex_adj_mask[u as usize].contains(v))
        })
        .unwrap();
    g.cities[2].insert(p2_a);
    g.cities[2].insert(p2_b);
    g.players[2].cities_left -= 2;

    // Seat 0 holds the Old Boot -> target is 5 VP, so 4 VP does NOT win on EndTurn / action!
    g.old_boot_owner = Some(0);
    assert_eq!(g.target_victory_points(0), 5);
    assert_eq!(g.target_victory_points(2), 4);

    // Cannot pass to self or to seat 1 (2 VP < 4 VP).
    assert_eq!(
        g.apply(0, Action::PassOldBoot(0)),
        Err(RuleError::InvalidPlayer)
    );
    assert_eq!(
        g.apply(0, Action::PassOldBoot(1)),
        Err(RuleError::InvalidVictim)
    );

    // Passing Old Boot to seat 2 (4 VP >= 4 VP) lowers seat 0's target to 4 VP and wins immediately!
    let mut events = Vec::new();
    g.apply_with(0, Action::PassOldBoot(2), &mut events)
        .unwrap();
    assert_eq!(g.old_boot_owner, Some(2));
    assert_eq!(g.winner, Some(0));
    assert_eq!(g.phase, Phase::GameOver);
    assert!(events.contains(&Event::OldBootPassed { from: 0, to: 2 }));
    assert!(events.contains(&Event::GameWon {
        player: 0,
        points: 4,
    }));
    g.check_invariants().unwrap();
}

#[test]
fn all_five_fish_spending_actions_and_codec_work() {
    // 0. Action binary codec round-trip for tags 28..=33.
    let samples = [
        (Action::PassOldBoot(2), 28u8),
        (Action::FishDriveOffRobber, 29),
        (Action::FishSteal(1), 30),
        (Action::FishTakeResource(Resource::Ore), 31),
        (Action::FishBuildRoad(14), 32),
        (Action::FishBuyDevCard, 33),
    ];
    for (act, expected_tag) in samples {
        let mut buf = Vec::new();
        act.encode(&mut buf);
        assert_eq!(buf[0], expected_tag);
        assert_eq!(Action::decode(&buf), Some((act, buf.len())));
    }

    let mut g = fishermen_main(2);
    let v0 = 0u8;
    g.settlements[0].insert(v0);
    g.players[0].settlements_left -= 1;
    let e0 = BASE.vertex_edges[v0 as usize][0];

    // 1. FishDriveOffRobber (2 fish): rejected while robber is already on desert.
    g.players[0].fish = 2;
    assert_eq!(
        g.apply(0, Action::FishDriveOffRobber),
        Err(RuleError::RobberMustMove)
    );
    let non_desert = (0..BASE.num_tiles as u8)
        .find(|&t| t != g.board.desert)
        .unwrap();
    g.robber = non_desert;

    // Disallowed while an open domestic trade offer is pending in Main!
    let brick = ResourceSet::single(Resource::Brick, 1);
    let wool = ResourceSet::single(Resource::Wool, 1);
    g.bank -= brick + wool;
    g.players[0].hand += brick;
    g.players[1].hand += wool;
    g.apply(0, Action::ProposeTrade { give: brick, get: wool })
        .unwrap();
    assert_eq!(
        g.check(0, Action::FishDriveOffRobber),
        Err(RuleError::WrongPhase)
    );
    let mut legal = Vec::new();
    g.legal_actions(0, &mut legal);
    assert!(!legal.contains(&Action::FishDriveOffRobber));
    g.apply(0, Action::CancelTrade).unwrap();

    let mut events = Vec::new();
    g.apply_with(0, Action::FishDriveOffRobber, &mut events)
        .unwrap();
    assert_eq!(g.players[0].fish, 0);
    assert_eq!(g.robber, g.board.desert);
    assert_eq!(
        events,
        vec![
            Event::FishSpent { player: 0, cost: 2 },
            Event::RobberMoved {
                player: 0,
                tile: g.board.desert,
            },
        ]
    );

    // 2. FishSteal (3 fish): steals 1 card from victim.
    g.players[0].fish = 3;
    events.clear();
    g.apply_with(0, Action::FishSteal(1), &mut events).unwrap();
    assert_eq!(g.players[0].fish, 0);
    assert_eq!(g.players[1].hand[Resource::Wool], 0);
    assert_eq!(g.players[0].hand[Resource::Wool], 1);
    assert!(events.contains(&Event::FishSpent { player: 0, cost: 3 }));
    assert!(events.contains(&Event::Stolen {
        thief: 0,
        victim: 1,
        resource: Some(Resource::Wool),
    }));
    // Victim now has 0 cards -> InvalidVictim.
    g.players[0].fish = 3;
    assert_eq!(
        g.apply(0, Action::FishSteal(1)),
        Err(RuleError::InvalidVictim)
    );

    // 3. FishTakeResource (4 fish): takes 1 chosen resource from bank.
    g.players[0].fish = 4;
    events.clear();
    g.apply_with(0, Action::FishTakeResource(Resource::Ore), &mut events)
        .unwrap();
    assert_eq!(g.players[0].fish, 0);
    assert_eq!(g.players[0].hand[Resource::Ore], 1);
    assert_eq!(
        events,
        vec![
            Event::FishSpent { player: 0, cost: 4 },
            Event::FishResourceTaken {
                player: 0,
                resource: Resource::Ore,
            },
        ]
    );

    // 4. FishBuildRoad (5 fish): places connected road and updates longest road.
    g.players[0].fish = 5;
    events.clear();
    g.apply_with(0, Action::FishBuildRoad(e0), &mut events)
        .unwrap();
    assert_eq!(g.players[0].fish, 0);
    assert!(g.roads[0].contains(e0));
    assert_eq!(g.players[0].longest_road, 1);
    assert!(events.contains(&Event::FishSpent { player: 0, cost: 5 }));
    assert!(events.contains(&Event::RoadBuilt {
        player: 0,
        edge: e0,
    }));

    // 5. FishBuyDevCard (7 fish): VP card enters `dev_cards` immediately, others `new_dev_cards`.
    let top = g.dev_deck_len as usize - 1;
    let knight_pos = g.dev_deck[..=top]
        .iter()
        .position(|&c| c == DevCard::Knight)
        .unwrap();
    g.dev_deck.swap(knight_pos, top);
    let vp_pos = g.dev_deck[..top]
        .iter()
        .position(|&c| c == DevCard::VictoryPoint)
        .unwrap();
    g.dev_deck.swap(vp_pos, top - 1);

    g.players[0].fish = 7;
    g.apply(0, Action::FishBuyDevCard).unwrap();
    assert_eq!(g.players[0].fish, 0);
    assert_eq!(g.players[0].new_dev_cards[DevCard::Knight.index()], 1);

    g.players[0].fish = 7;
    g.apply(0, Action::FishBuyDevCard).unwrap();
    assert_eq!(g.players[0].dev_cards[DevCard::VictoryPoint.index()], 1);
    assert_eq!(g.victory_points(0), 2);
    g.check_invariants().unwrap();
}
