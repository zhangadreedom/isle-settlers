//! Determinization: replaces everything `me` cannot know with a random guess
//! consistent with what `me` can see.
//!
//! Hidden information is: other players' hand composition, other players'
//! development cards, the order of the development deck and the RNG (future
//! dice and steals). The result depends only on public information, `me`'s
//! own cards and the random generator — never on the true hidden values — so
//! a bot that searches determinized states cannot cheat.

use isle_core::{DevCard, GameState, Resource, ResourceSet, Rng};

/// A random full state consistent with `me`'s knowledge of `s`.
pub fn determinize(s: &GameState, me: u8, rng: &mut Rng) -> GameState {
    let mut d = *s;
    let n = s.num_players();
    let me = me as usize;
    let layout = s.board.layout;

    // Resources. The cards held by opponents, taken together, are public:
    // whatever is neither in the bank nor in my hand. Only who holds which
    // card is unknown, so deal that multiset out by the public hand sizes.
    let mut pool: Vec<Resource> = Vec::with_capacity(5 * layout.bank_per_resource() as usize);
    for r in Resource::ALL {
        let elsewhere = layout.bank_per_resource() - s.bank[r] - s.players[me].hand[r];
        pool.extend(std::iter::repeat_n(r, elsewhere as usize));
    }
    rng.shuffle(&mut pool);
    let mut cards = pool.into_iter();
    for q in 0..n {
        if q == me {
            continue;
        }
        let count = s.players[q].hand.total();
        let mut hand = ResourceSet::EMPTY;
        for _ in 0..count {
            hand[cards.next().expect("hand sizes match the pool")] += 1;
        }
        d.players[q].hand = hand;
    }

    // Development cards. Unseen = the full deck minus my cards and every card
    // played face up. Opponents keep their public card counts; the deck keeps
    // its length. (Cards an opponent bought this turn are treated as
    // playable: which of their cards are new is not tracked publicly.)
    let mut unseen: Vec<DevCard> = Vec::with_capacity(layout.dev_deck_size());
    for card in DevCard::ALL {
        let c = card.index();
        let mine = s.players[me].dev_cards[c] + s.players[me].new_dev_cards[c];
        let played: u8 = (0..n).map(|q| s.players[q].played_dev_cards[c]).sum();
        let left = card.count_in_deck(layout) - mine - played;
        unseen.extend(std::iter::repeat_n(card, left as usize));
    }
    rng.shuffle(&mut unseen);
    let mut unseen = unseen.into_iter();
    for q in 0..n {
        if q == me {
            continue;
        }
        let count = s.players[q].dev_card_count();
        let mut held = [0u8; 5];
        for _ in 0..count {
            held[unseen.next().expect("dev card counts match").index()] += 1;
        }
        d.players[q].dev_cards = held;
        d.players[q].new_dev_cards = [0; 5];
    }
    let deck: Vec<DevCard> = unseen.collect();
    debug_assert_eq!(deck.len(), s.dev_deck_len as usize);
    d.dev_deck[..deck.len()].copy_from_slice(&deck);

    // Unexplored fog stacks. The multiset of remaining tiles and number
    // tokens is public (initial pool minus revealed tiles); only their order
    // is hidden, so sort into canonical order first and shuffle.
    if layout.has_fog() && s.fog_terrain_len > 0 {
        let tlen = s.fog_terrain_len as usize;
        d.fog_terrain[..tlen].sort_unstable_by_key(|&t| t as u8);
        rng.shuffle(&mut d.fog_terrain[..tlen]);
        let nlen = s.fog_numbers_len as usize;
        d.fog_numbers[..nlen].sort_unstable();
        rng.shuffle(&mut d.fog_numbers[..nlen]);
    }

    // Future dice and steals.
    d.rng = Rng::new(rng.next_u64());
    d
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{Bot, WeightedRandomBot};
    use isle_core::GameConfig;

    pub(crate) fn sample_states() -> Vec<GameState> {
        let mut out = Vec::new();
        for (scenario, seeds) in [
            (isle_core::Scenario::Classic, 0..14u64),
            (isle_core::Scenario::FogIslands, 14..24u64),
        ] {
            for seed in seeds {
                let mut g = GameState::new(GameConfig::scenario(scenario, 4), seed).unwrap();
                let mut bots: Vec<WeightedRandomBot> = (0..4)
                    .map(|k| WeightedRandomBot::new(seed * 9 + k))
                    .collect();
                let mut legal = Vec::new();
                for step in 0..500 {
                    let Some(p) = g.to_move() else { break };
                    g.legal_actions(p, &mut legal);
                    let a = bots[p as usize].choose(&g, p, &legal);
                    g.apply(p, a).unwrap();
                    if step % 60 == 59 {
                        out.push(g);
                    }
                }
            }
        }
        out
    }

    /// Changes only what player `me` cannot observe.
    fn scramble(g: &GameState, me: u8, seed: u64) -> GameState {
        let mut rng = Rng::new(seed);
        let mut h = determinize(g, me, &mut rng);
        h.rng = Rng::new(seed ^ 77);
        h
    }

    #[test]
    fn keeps_public_and_own_information_and_conservation() {
        for g in sample_states() {
            for me in 0..4u8 {
                let d = determinize(&g, me, &mut Rng::new(5));
                d.check_invariants().unwrap();
                assert_eq!(d.players[me as usize].hand, g.players[me as usize].hand);
                assert_eq!(
                    d.players[me as usize].dev_cards,
                    g.players[me as usize].dev_cards
                );
                for q in 0..4 {
                    assert_eq!(d.players[q].hand.total(), g.players[q].hand.total());
                    assert_eq!(d.players[q].dev_card_count(), g.players[q].dev_card_count());
                    assert_eq!(d.players[q].played_dev_cards, g.players[q].played_dev_cards);
                }
                assert_eq!(d.dev_deck_len, g.dev_deck_len);
                assert_eq!(
                    (d.bank, d.settlements, d.cities, d.roads, d.phase),
                    (g.bank, g.settlements, g.cities, g.roads, g.phase)
                );
            }
        }
    }

    #[test]
    fn result_does_not_depend_on_hidden_information() {
        for (i, g) in sample_states().iter().enumerate() {
            for me in 0..4u8 {
                // Two states that look the same to `me`...
                let other = scramble(g, me, i as u64 * 31 + me as u64);
                // ...determinize to exactly the same state with the same RNG.
                let a = determinize(g, me, &mut Rng::new(99));
                let b = determinize(&other, me, &mut Rng::new(99));
                assert_eq!(a, b);
            }
        }
    }
}
