//! Information-set Monte Carlo tree search (single observer, open loop).
//!
//! Every iteration samples a fresh determinization of the hidden information
//! (opponents' cards, deck order, future dice) consistent with what the bot
//! can see, then walks one shared tree keyed by action sequences. Children
//! are selected with UCB1 using availability counts (an action may be legal in
//! some determinizations only). Leaving the tree, a fast heuristic policy
//! plays a couple of turns and the static evaluation turns the position into
//! per-seat win estimates, which are backed up max^n style: each node keeps
//! the reward of the seat that chose its action.

use isle_core::resources::cost;
use isle_core::{Action, GameState, MAX_PLAYERS, Phase, ResourceSet, Rng};

use crate::Bot;
use crate::determinize::determinize;
use crate::eval::{Weights, rewards};
use crate::greedy::{missing, robber_value, spot_value, vertex_production};

#[derive(Clone, Copy, Debug)]
pub struct MctsConfig {
    /// Search iterations per decision.
    pub iterations: u32,
    /// UCB exploration constant (rewards are win estimates in [0, 1]).
    pub exploration: f32,
    /// Turns simulated after leaving the tree before evaluating.
    pub rollout_turns: u32,
    /// Wide action families (setup spots, discards, robber moves) are cut to
    /// their best few by heuristics.
    pub max_children: usize,
    /// Deepest tree level that is expanded.
    pub max_depth: usize,
    pub weights: Weights,
}

impl Default for MctsConfig {
    fn default() -> Self {
        // Tuned by tournaments (docs/adr/0001-tech-stack.md, "AI（P3 补充）").
        MctsConfig {
            iterations: 4000,
            exploration: 0.5,
            rollout_turns: 8,
            max_children: 8,
            max_depth: 24,
            weights: Weights::default(),
        }
    }
}

impl MctsConfig {
    /// Overrides one setting by short key (used by `bot_by_name` for
    /// tuning): `c` exploration, `r` rollout turns, `k` max children, `d` max
    /// depth, and evaluation weights `wi` income, `wd` diversity, `ws` spots,
    /// `wh` hand, `wo` overflow, `wc` dev card, `wk` knight, `wr` road, `wp`
    /// harbor, `wb` build potential, `wt` temperature.
    pub fn set(&mut self, key: &str, value: f32) -> Option<()> {
        let w = &mut self.weights;
        match key {
            "c" => self.exploration = value,
            "r" => self.rollout_turns = value as u32,
            "k" => self.max_children = value as usize,
            "d" => self.max_depth = value as usize,
            "wi" => w.income = value,
            "wd" => w.diversity = value,
            "ws" => w.spots = value,
            "wh" => w.hand = value,
            "wo" => w.overflow = value,
            "wc" => w.dev_card = value,
            "wk" => w.knight = value,
            "wr" => w.road = value,
            "wp" => w.harbor = value,
            "wb" => w.build = value,
            "wt" => w.temperature = value,
            _ => return None,
        }
        Some(())
    }
}

struct Node {
    action: Action,
    /// Seat that chose `action`.
    actor: u8,
    visits: u32,
    /// Times this action was available when its parent was visited.
    avail: u32,
    /// Sum of the actor's rewards.
    reward: f32,
    children: Vec<u32>,
}

/// Statistics of the last search (for tuning and debugging).
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchStats {
    pub iterations: u32,
    pub nodes: usize,
    /// Visits and mean reward of the chosen action.
    pub best_visits: u32,
    pub best_value: f32,
}

pub struct MctsBot {
    config: MctsConfig,
    rng: Rng,
    nodes: Vec<Node>,
    legal: Vec<Action>,
    cands: Vec<Action>,
    scratch: Vec<Action>,
    path: Vec<u32>,
    pub last: SearchStats,
}

impl MctsBot {
    pub fn new(seed: u64) -> Self {
        Self::with_config(seed, MctsConfig::default())
    }

    pub fn with_config(seed: u64, config: MctsConfig) -> Self {
        MctsBot {
            config,
            rng: Rng::new(seed),
            nodes: Vec::new(),
            legal: Vec::with_capacity(128),
            cands: Vec::with_capacity(64),
            scratch: Vec::with_capacity(64),
            path: Vec::with_capacity(32),
            last: SearchStats::default(),
        }
    }

    pub fn config(&self) -> &MctsConfig {
        &self.config
    }

    fn search(&mut self, root: &GameState, me: u8, root_cands: &[Action]) -> Action {
        self.nodes.clear();
        self.nodes.push(Node {
            action: Action::EndTurn,
            actor: me,
            visits: 0,
            avail: 0,
            reward: 0.0,
            children: Vec::new(),
        });
        for _ in 0..self.config.iterations {
            self.iterate(root, me, root_cands);
        }
        let root_children = &self.nodes[0].children;
        let best = root_children
            .iter()
            .map(|&c| &self.nodes[c as usize])
            .max_by(|a, b| a.visits.cmp(&b.visits).then(mean(a).total_cmp(&mean(b))))
            .expect("at least one candidate");
        self.last = SearchStats {
            iterations: self.config.iterations,
            nodes: self.nodes.len(),
            best_visits: best.visits,
            best_value: mean(best),
        };
        best.action
    }

    fn iterate(&mut self, root: &GameState, me: u8, root_cands: &[Action]) {
        let mut s = determinize(root, me, &mut self.rng);
        let mut node = 0usize;
        self.path.clear();
        let mut depth = 0;
        while s.phase != Phase::GameOver {
            let actor = if node == 0 {
                me
            } else {
                s.to_move().expect("game running")
            };
            if node == 0 {
                self.cands.clear();
                self.cands.extend_from_slice(root_cands);
            } else {
                s.legal_actions(actor, &mut self.legal);
                candidates(
                    &s,
                    actor,
                    &self.legal,
                    self.config.max_children,
                    &mut self.cands,
                );
            }
            if self.cands.is_empty() {
                break;
            }
            // Count availability, find untried actions and the best child.
            let explore = self.config.exploration;
            let mut best: Option<(f32, usize)> = None;
            self.scratch.clear();
            for &a in &self.cands {
                let found = self.nodes[node]
                    .children
                    .iter()
                    .copied()
                    .find(|&c| self.nodes[c as usize].action == a);
                match found {
                    Some(c) => {
                        let child = &mut self.nodes[c as usize];
                        child.avail += 1;
                        let ucb = child.reward / child.visits as f32
                            + explore * ((child.avail as f32).ln() / child.visits as f32).sqrt();
                        if best.is_none_or(|(b, _)| ucb > b) {
                            best = Some((ucb, c as usize));
                        }
                    }
                    None => self.scratch.push(a),
                }
            }
            if !self.scratch.is_empty() {
                if depth >= self.config.max_depth {
                    break;
                }
                let a = self.scratch[self.rng.below(self.scratch.len() as u32) as usize];
                let id = self.nodes.len() as u32;
                self.nodes.push(Node {
                    action: a,
                    actor,
                    visits: 0,
                    avail: 1,
                    reward: 0.0,
                    children: Vec::new(),
                });
                self.nodes[node].children.push(id);
                s.apply(actor, a).expect("candidates are legal");
                self.path.push(id);
                break;
            }
            let (_, c) = best.expect("some child is available");
            s.apply(actor, self.nodes[c].action)
                .expect("available children are legal");
            self.path.push(c as u32);
            node = c;
            depth += 1;
        }

        rollout(
            &mut s,
            self.config.rollout_turns,
            &mut self.rng,
            &mut self.legal,
        );
        let mut r = [0.0f32; MAX_PLAYERS];
        rewards(&s, &self.config.weights, &mut r);
        for &id in &self.path {
            let n = &mut self.nodes[id as usize];
            n.visits += 1;
            n.reward += r[n.actor as usize];
        }
    }
}

fn mean(n: &Node) -> f32 {
    if n.visits == 0 {
        0.0
    } else {
        n.reward / n.visits as f32
    }
}

impl Bot for MctsBot {
    fn name(&self) -> &'static str {
        "mcts"
    }

    fn choose(&mut self, state: &GameState, player: u8, legal: &[Action]) -> Action {
        if legal.len() == 1 {
            return legal[0];
        }
        let mut cands = Vec::with_capacity(legal.len());
        candidates(state, player, legal, self.config.max_children, &mut cands);
        if cands.len() == 1 {
            return cands[0];
        }
        self.search(state, player, &cands)
    }
}

// ------------------------------------------------------------ pruning

/// Actions worth searching: everything, except that wide families (setup
/// spots, discards, robber moves) are cut to their `k` best by heuristics.
/// Uses only information the actor has (the state is a determinization).
fn candidates(s: &GameState, actor: u8, legal: &[Action], k: usize, out: &mut Vec<Action>) {
    out.clear();
    let wide = matches!(
        s.phase,
        Phase::SetupSettlement | Phase::Discard | Phase::MoveRobber
    );
    if legal.len() <= k || !wide {
        out.extend_from_slice(legal);
        return;
    }
    let hand = s.players[actor as usize].hand;
    let score = |a: &Action| match a {
        Action::BuildSettlement(v) => spot_value(s, actor, *v),
        Action::Discard(d) => discard_score(hand - *d),
        Action::MoveRobber { tile, victim } => robber_value(s, actor, *tile, *victim),
        _ => 0.0,
    };
    let mut scored: Vec<(f32, Action)> = legal.iter().map(|a| (score(a), *a)).collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.extend(scored.iter().take(k).map(|&(_, a)| a));
}

/// Prefer keeping cards that complete something, then a varied hand.
fn discard_score(keep: ResourceSet) -> f32 {
    let builds = [cost::CITY, cost::SETTLEMENT, cost::DEV_CARD, cost::ROAD];
    let progress: f32 = builds
        .iter()
        .map(|b| 1.0 / (1.0 + missing(keep, *b) as f32))
        .sum();
    let spread: f32 = keep.0.iter().map(|&c| (c.min(2)) as f32).sum();
    progress * 2.0 + spread * 0.3
}

// ------------------------------------------------------------ rollouts

/// Plays `turns` turns (or to the end) with the fast policy.
fn rollout(s: &mut GameState, turns: u32, rng: &mut Rng, legal: &mut Vec<Action>) {
    let end = s.turn + turns;
    let mut steps = 0;
    while let Some(p) = s.to_move() {
        if s.turn >= end || steps > 600 {
            break;
        }
        s.legal_actions(p, legal);
        let a = rollout_policy(s, p, legal, rng);
        s.apply(p, a).expect("policy plays legal actions");
        steps += 1;
    }
}

fn best_of(legal: &[Action], mut score: impl FnMut(&Action) -> Option<f32>) -> Option<Action> {
    let mut best: Option<(f32, Action)> = None;
    for a in legal {
        if let Some(v) = score(a)
            && best.is_none_or(|(b, _)| v > b)
        {
            best = Some((v, *a));
        }
    }
    best.map(|(_, a)| a)
}

/// Cheap, reasonable play: build when possible, trade towards a build,
/// rob the strongest neighbour, discard the most plentiful cards.
fn rollout_policy(s: &GameState, p: u8, legal: &[Action], rng: &mut Rng) -> Action {
    if legal.len() == 1 {
        return legal[0];
    }
    let pick = |rng: &mut Rng| legal[rng.below(legal.len() as u32) as usize];
    match s.phase {
        Phase::SetupSettlement => best_of(legal, |a| match a {
            Action::BuildSettlement(v) => {
                Some(vertex_production(s, *v) + rng.below(100) as f32 * 0.01)
            }
            _ => None,
        })
        .unwrap_or_else(|| pick(rng)),
        Phase::SetupRoad { .. } | Phase::RoadBuilding { .. } => pick(rng),
        Phase::Discard => {
            // Drop from the largest piles.
            let mut hand = s.players[p as usize].hand;
            let mut drop = ResourceSet::EMPTY;
            for _ in 0..s.pending_discard[p as usize] {
                let r = (0..5).max_by_key(|&i| hand.0[i]).expect("five resources");
                hand.0[r] -= 1;
                drop.0[r] += 1;
            }
            legal
                .iter()
                .copied()
                .find(|a| *a == Action::Discard(drop))
                .unwrap_or_else(|| pick(rng))
        }
        Phase::MoveRobber => best_of(legal, |a| match a {
            Action::MoveRobber { tile, victim } => Some(robber_value(s, p, *tile, *victim)),
            _ => None,
        })
        .unwrap_or_else(|| pick(rng)),
        Phase::PreRoll => {
            let robbed = s.buildings_of(p) & s.topo().tile_vertex_mask[s.robber as usize] != 0;
            if robbed && legal.contains(&Action::PlayKnight) {
                Action::PlayKnight
            } else {
                Action::RollDice
            }
        }
        Phase::Main if p != s.current => Action::RespondTrade { accept: false },
        Phase::Main | Phase::SpecialBuild { .. } => main_policy(s, p, legal, rng),
        Phase::GameOver => legal[0],
    }
}

fn main_policy(s: &GameState, p: u8, legal: &[Action], rng: &mut Rng) -> Action {
    if let Some(a) = best_of(legal, |a| match a {
        Action::BuildCity(v) => Some(vertex_production(s, *v)),
        _ => None,
    }) {
        return a;
    }
    if let Some(a) = best_of(legal, |a| match a {
        Action::BuildSettlement(v) => Some(vertex_production(s, *v)),
        _ => None,
    }) {
        return a;
    }
    if let Some(a) = legal
        .iter()
        .find(|a| matches!(a, Action::ConfirmTrade { .. }))
    {
        return *a;
    }
    let hand = s.players[p as usize].hand;
    // Trade surplus with the bank towards a city or settlement.
    for target in [cost::CITY, cost::SETTLEMENT] {
        if missing(hand, target) == 1 {
            let trade = legal.iter().find(|a| match a {
                Action::MaritimeTrade { give, get } => {
                    hand[*get] < target[*get]
                        && hand[*give] as i32 - target[*give] as i32
                            >= s.trade_ratio(p, *give) as i32
                }
                _ => false,
            });
            if let Some(t) = trade {
                return *t;
            }
        }
    }
    if legal.contains(&Action::BuyDevCard) && rng.below(2) == 0 {
        return Action::BuyDevCard;
    }
    let robbed = s.buildings_of(p) & s.topo().tile_vertex_mask[s.robber as usize] != 0;
    if robbed && legal.contains(&Action::PlayKnight) {
        return Action::PlayKnight;
    }
    if rng.below(3) == 0 {
        let roads: Vec<&Action> = legal
            .iter()
            .filter(|a| matches!(a, Action::BuildRoad(_)))
            .collect();
        if !roads.is_empty() {
            return *roads[rng.below(roads.len() as u32) as usize];
        }
    }
    if rng.below(5) == 0 {
        let plays: Vec<&Action> = legal
            .iter()
            .filter(|a| {
                matches!(
                    a,
                    Action::PlayYearOfPlenty(_)
                        | Action::PlayMonopoly(_)
                        | Action::PlayRoadBuilding
                )
            })
            .collect();
        if !plays.is_empty() {
            return *plays[rng.below(plays.len() as u32) as usize];
        }
    }
    Action::EndTurn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::determinize::tests::sample_states;
    use crate::{GreedyBot, play_game};
    use isle_core::GameConfig;

    fn small() -> MctsConfig {
        MctsConfig {
            iterations: 120,
            ..MctsConfig::default()
        }
    }

    #[test]
    fn plays_legal_moves_through_whole_games() {
        for seed in 0..3 {
            let mut g = GameState::new(GameConfig::default(), seed).unwrap();
            let mut m = MctsBot::with_config(seed, small());
            let mut b: Vec<GreedyBot> = (0..3).map(|k| GreedyBot::new(seed * 3 + k)).collect();
            let [b1, b2, b3] = &mut b[..] else {
                unreachable!()
            };
            let mut bots: Vec<&mut dyn Bot> = vec![&mut m, b1, b2, b3];
            let out = play_game(&mut g, &mut bots, 1000, true);
            assert!(out.winner.is_some(), "seed {seed} did not finish");
        }
    }

    #[test]
    fn decisions_do_not_depend_on_hidden_information() {
        for (i, g) in sample_states().iter().enumerate().take(12) {
            let Some(me) = g.to_move() else { continue };
            let mut legal = Vec::new();
            g.legal_actions(me, &mut legal);
            if legal.len() < 2 {
                continue;
            }
            // A state that looks identical to `me` but differs in hidden cards.
            let mut other = determinize(g, me, &mut Rng::new(i as u64 + 1000));
            other.rng = Rng::new(4242);
            let a = MctsBot::with_config(7, small()).choose(g, me, &legal);
            let b = MctsBot::with_config(7, small()).choose(&other, me, &legal);
            assert_eq!(a, b, "state {i}: decision changed with hidden information");
        }
    }

    #[test]
    fn search_is_reproducible() {
        let g = sample_states()[3];
        let me = g.to_move().unwrap();
        let mut legal = Vec::new();
        g.legal_actions(me, &mut legal);
        let a = MctsBot::with_config(1, small()).choose(&g, me, &legal);
        let b = MctsBot::with_config(1, small()).choose(&g, me, &legal);
        assert_eq!(a, b);
    }
}
