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
use isle_core::topology::NONE;
use isle_core::{
    Action, CommoditySet, GameState, MAX_PLAYERS, Phase, ResourceSet, Rng, TradeGood,
};

use crate::determinize::determinize;
use crate::eval::{Weights, rewards};
use crate::greedy::{
    bridge_value, camel_spot_value, citadel_city_value, fishermen_city_value, knight_spot_value,
    missing, propose_trade_offer, robber_hurts_me, robber_value, setup_spot_value, spot_value,
    track_for_commodity, upgrade_city_value, vertex_production,
};
use crate::{Bot, BotPersona};

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
    /// Default MCTS settings with `persona` evaluation weights.
    pub fn for_persona(persona: BotPersona) -> Self {
        MctsConfig {
            weights: Weights::for_persona(persona),
            ..MctsConfig::default()
        }
    }

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
            "wa" => w.sea = value,
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
    propose_trades: bool,
    last_offer: Option<(u32, u8)>,
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

    pub fn with_persona(seed: u64, persona: BotPersona) -> Self {
        let mut bot = Self::with_config(seed, MctsConfig::for_persona(persona));
        if persona == BotPersona::Merchant {
            bot.propose_trades = true;
        }
        bot
    }

    pub fn with_config(seed: u64, config: MctsConfig) -> Self {
        MctsBot {
            config,
            rng: Rng::new(seed),
            propose_trades: false,
            last_offer: None,
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
        match self.config.weights.persona {
            BotPersona::Balanced => "mcts",
            BotPersona::RoadBuilder => "mcts@road",
            BotPersona::CityBaron => "mcts@city",
            BotPersona::Warlord => "mcts@warlord",
            BotPersona::Merchant => "mcts@merchant",
        }
    }

    fn set_propose_trades(&mut self, enabled: bool) {
        self.propose_trades = enabled;
    }

    fn choose(&mut self, state: &GameState, player: u8, legal: &[Action]) -> Action {
        if legal.len() == 1
            && !(self.propose_trades
                && state.phase == Phase::Main
                && player == state.current
                && state.trade.is_none())
        {
            return legal[0];
        }
        if state.trade.is_some()
            && player == state.current
            && !legal.iter().any(|a| matches!(a, Action::ConfirmTrade { .. }))
            && legal.contains(&Action::CancelTrade)
        {
            return Action::CancelTrade;
        }
        let mut cands = Vec::with_capacity(legal.len());
        candidates(state, player, legal, self.config.max_children, &mut cands);
        let chosen = if cands.len() == 1 {
            cands[0]
        } else {
            self.search(state, player, &cands)
        };
        if self.propose_trades
            && chosen == Action::EndTurn
            && state.phase == Phase::Main
            && player == state.current
            && state.trade.is_none()
            && self.last_offer != Some((state.turn, player))
        {
            let ps = &state.players[player as usize];
            for (ok, target) in [
                (
                    ps.cities_left > 0 && !state.settlements[player as usize].is_empty(),
                    cost::CITY,
                ),
                (
                    ps.settlements_left > 0 && !state.settlement_spots(player, true).is_empty(),
                    cost::SETTLEMENT,
                ),
            ] {
                if ok && let Some(offer) = propose_trade_offer(state, player, target) {
                    self.last_offer = Some((state.turn, player));
                    return offer;
                }
            }
        }
        chosen
    }
}

// ------------------------------------------------------------ pruning

/// Actions worth searching: everything, except that wide families (setup
/// spots, discards, robber moves, and wide Citadels knight/commodity lists)
/// are cut to their best few by heuristics.
/// Uses only information the actor has (the state is a determinization).
fn candidates(s: &GameState, actor: u8, legal: &[Action], k: usize, out: &mut Vec<Action>) {
    out.clear();
    let citadels = s.config.scenario.is_citadels();
    if citadels && matches!(s.phase, Phase::Main | Phase::PairedTurn { .. }) && legal.len() > k {
        citadel_main_candidates(s, actor, legal, out);
        return;
    }
    let wide = matches!(
        s.phase,
        Phase::SetupSettlement | Phase::Discard | Phase::MoveRobber | Phase::PlaceCamel
    );
    if legal.len() <= k || !wide {
        out.extend_from_slice(legal);
        return;
    }
    let hand = s.players[actor as usize].hand;
    let comm = s.players[actor as usize].commodities;
    let score = |a: &Action| match a {
        Action::BuildSettlement(v) if citadels => setup_spot_value(s, actor, *v),
        Action::BuildSettlement(v) => spot_value(s, actor, *v),
        Action::Discard(d) => discard_score(hand - *d),
        Action::DiscardCitadel {
            resources,
            commodities,
        } => citadel_discard_score(s, actor, hand - *resources, comm - *commodities),
        Action::MoveRobber { tile, victim } => robber_value(s, actor, *tile, *victim),
        Action::PlaceCamel(e) => camel_spot_value(s, actor, *e, BotPersona::Balanced),
        _ => 0.0,
    };
    let mut scored: Vec<(f32, Action)> = legal.iter().map(|a| (score(a), *a)).collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.extend(scored.iter().take(k).map(|&(_, a)| a));
}

fn citadel_discard_score(
    s: &GameState,
    actor: u8,
    keep_hand: ResourceSet,
    keep_comm: CommoditySet,
) -> f32 {
    let mut score = discard_score(keep_hand);
    let ps = &s.players[actor as usize];
    let has_city = !s.cities[actor as usize].is_empty();
    let ratio = s.commodity_trade_ratio(actor);
    for track in isle_core::CityTrack::ALL {
        let lvl = ps.city_progress[track.index()];
        let c = track.commodity();
        let kept = keep_comm[c];
        if has_city && lvl < 5 {
            let need = lvl + 1;
            let frac = kept.min(need) as f32 / need as f32;
            let weight = if s.upgrade_claims_metropolis(actor, track) {
                6.0
            } else if need == 3 {
                4.5
            } else {
                2.5
            };
            score += frac * weight + if kept >= need { 2.5 } else { 0.0 };
            score += (kept.saturating_sub(need) / ratio) as f32 * 0.8;
        } else {
            score += (kept / ratio) as f32 * 0.9 + kept as f32 * 0.15;
        }
    }
    score
}

fn citadel_main_candidates(s: &GameState, actor: u8, legal: &[Action], out: &mut Vec<Action>) {
    let ps = &s.players[actor as usize];
    let hand = ps.hand;
    let comm = ps.commodities;
    let has_city = !s.cities[actor as usize].is_empty();
    let mut chase: Vec<(f32, Action)> = Vec::new();
    let mut recruit: Vec<(f32, Action)> = Vec::new();
    let mut move_k: Vec<(f32, Action)> = Vec::new();
    let mut trades: Vec<(f32, Action)> = Vec::new();

    for &a in legal {
        match a {
            Action::ChaseRobber { tile, victim, .. } => {
                chase.push((robber_value(s, actor, tile, victim), a));
            }
            Action::RecruitKnight(v) => {
                recruit.push((knight_spot_value(s, actor, v), a));
            }
            Action::MoveKnight { from, to } => {
                if s.knight_owner[to as usize] != NONE && s.knight_owner[to as usize] != actor {
                    let opp = s.knight_owner[to as usize];
                    let score = if s.knight_retreat_spot(opp, to, from).is_none() {
                        10.0
                    } else {
                        4.0
                    };
                    move_k.push((score, a));
                }
            }
            Action::MaritimeTradeGood { give, get } => {
                let score = match get {
                    TradeGood::Commodity(c) if has_city => {
                        let track = track_for_commodity(c);
                        let lvl = ps.city_progress[track.index()];
                        if lvl < 5 && comm[c] + 1 == lvl + 1 {
                            Some(upgrade_city_value(s, actor, track, None))
                        } else {
                            None
                        }
                    }
                    TradeGood::Resource(r) => {
                        let wants_r = cost::CITY[r] > hand[r]
                            || cost::SETTLEMENT[r] > hand[r]
                            || cost::KNIGHT[r] > hand[r]
                            || cost::ACTIVATE_KNIGHT[r] > hand[r];
                        match give {
                            TradeGood::Commodity(c) => {
                                let lvl = ps.city_progress[track_for_commodity(c).index()];
                                ((wants_r && (lvl >= 3 || !has_city || ps.total_cards() > 7))
                                    || ps.total_cards() > 7)
                                    .then_some(if cost::CITY[r] > hand[r] { 2.0 } else { 1.0 })
                            }
                            TradeGood::Resource(_) => None,
                        }
                    }
                    _ => None,
                };
                if let Some(sc) = score {
                    trades.push((sc, a));
                }
            }
            _ => out.push(a),
        }
    }
    for (list, cap) in [(&mut chase, 3), (&mut recruit, 3), (&mut move_k, 2), (&mut trades, 4)] {
        list.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.extend(list.iter().take(cap).map(|&(_, a)| a));
    }
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
    let citadels = s.config.scenario.is_citadels();
    let pick = |rng: &mut Rng| legal[rng.below(legal.len() as u32) as usize];
    match s.phase {
        Phase::SetupSettlement => best_of(legal, |a| match a {
            Action::BuildSettlement(v) if citadels => {
                Some(setup_spot_value(s, p, *v) + rng.below(100) as f32 * 0.01)
            }
            Action::BuildSettlement(v)
                if s.config.scenario.is_fishermen()
                    || s.config.scenario.is_caravans()
                    || s.config.scenario.is_explorers() =>
            {
                Some(spot_value(s, p, *v) + rng.below(100) as f32 * 0.01)
            }
            Action::BuildSettlement(v) => {
                Some(vertex_production(s, *v) + rng.below(100) as f32 * 0.01)
            }
            _ => None,
        })
        .unwrap_or_else(|| pick(rng)),
        Phase::SetupRoad { .. } | Phase::RoadBuilding { .. } => pick(rng),
        Phase::PlaceCamel => best_of(legal, |a| match a {
            Action::PlaceCamel(e) => {
                Some(camel_spot_value(s, p, *e, BotPersona::Balanced) + rng.below(20) as f32 * 0.01)
            }
            _ => None,
        })
        .unwrap_or_else(|| pick(rng)),
        Phase::Discard if citadels => {
            let ps = &s.players[p as usize];
            let mut hand = ps.hand;
            let mut comm = ps.commodities;
            let mut drop_r = ResourceSet::EMPTY;
            let mut drop_c = CommoditySet::EMPTY;
            let has_city = !s.cities[p as usize].is_empty();
            for _ in 0..s.pending_discard[p as usize] {
                // Drop from the pile with highest disposable count.
                let mut best_idx = 0usize;
                let mut best_score = i32::MIN;
                for i in 0..5 {
                    if hand.0[i] > 0 {
                        let sc = hand.0[i] as i32 * 2;
                        if sc > best_score {
                            best_score = sc;
                            best_idx = i;
                        }
                    }
                }
                for i in 0..3 {
                    if comm.0[i] > 0 {
                        let c = isle_core::Commodity::ALL[i];
                        let lvl = ps.city_progress[track_for_commodity(c).index()];
                        let saving = has_city && lvl < 5 && comm.0[i] <= lvl + 1;
                        let sc = comm.0[i] as i32 * 2 - if saving { 5 } else { -1 };
                        if sc > best_score {
                            best_score = sc;
                            best_idx = 5 + i;
                        }
                    }
                }
                if best_idx < 5 {
                    hand.0[best_idx] -= 1;
                    drop_r.0[best_idx] += 1;
                } else {
                    comm.0[best_idx - 5] -= 1;
                    drop_c.0[best_idx - 5] += 1;
                }
            }
            let target = Action::DiscardCitadel {
                resources: drop_r,
                commodities: drop_c,
            };
            legal
                .iter()
                .copied()
                .find(|a| *a == target)
                .unwrap_or_else(|| pick(rng))
        }
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
            let robbed = !(s.buildings_of(p) & s.topo().tile_vertex_mask[s.robber as usize]).is_empty();
            if robbed && legal.contains(&Action::PlayKnight) {
                Action::PlayKnight
            } else {
                Action::RollDice
            }
        }
        // Pick what the next city or settlement still lacks.
        Phase::Gold { .. } => {
            let hand = s.players[p as usize].hand;
            best_of(legal, |a| match a {
                Action::ChooseGold(set) => {
                    let after = hand + *set;
                    Some(
                        -(missing(after, cost::CITY) as f32)
                            - missing(after, cost::SETTLEMENT) as f32
                            + rng.below(10) as f32 * 0.01,
                    )
                }
                _ => None,
            })
            .unwrap_or_else(|| pick(rng))
        }
        Phase::Main if p != s.current => Action::RespondTrade { accept: false },
        Phase::Main | Phase::SpecialBuild { .. } | Phase::PairedTurn { .. } => {
            main_policy(s, p, legal, rng)
        }
        Phase::GameOver => legal[0],
    }
}

fn main_policy(s: &GameState, p: u8, legal: &[Action], rng: &mut Rng) -> Action {
    let citadels = s.config.scenario.is_citadels();
    let fishermen = s.config.scenario.is_fishermen();
    let caravans = s.config.scenario.is_caravans();
    let explorers = s.config.scenario.is_explorers();
    if fishermen
        && let Some(a) = best_of(legal, |a| match a {
            Action::PassOldBoot(q) => Some(s.public_victory_points(*q) as f32),
            _ => None,
        })
    {
        return a;
    }
    if let Some(a) = best_of(legal, |a| match a {
        Action::BuildCity(v) if citadels => Some(citadel_city_value(s, p, *v)),
        Action::BuildCity(v) if fishermen => Some(fishermen_city_value(s, *v)),
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
    if explorers
        && let Some(a) = best_of(legal, |a| match a {
            Action::RaidPirateLair(_) => {
                Some(crate::greedy::explorer_mission_value(s, p, 0, BotPersona::Balanced))
            }
            Action::HaulFishShoal(_) => {
                Some(crate::greedy::explorer_mission_value(s, p, 1, BotPersona::Balanced))
            }
            Action::TradeSpice(_) => {
                Some(crate::greedy::explorer_mission_value(s, p, 2, BotPersona::Balanced))
            }
            _ => None,
        })
    {
        return a;
    }
    if citadels
        && let Some(a) = best_of(legal, |a| match a {
            Action::UpgradeCity { track, metropolis } => {
                Some(upgrade_city_value(s, p, *track, *metropolis))
            }
            _ => None,
        })
    {
        return a;
    }
    if let Some(a) = legal
        .iter()
        .find(|a| matches!(a, Action::ConfirmTrade { .. }))
    {
        return *a;
    }
    if citadels {
        let n = s.num_players();
        let my_vuln = s.non_metropolis_cities(p).count_ones() as u8;
        let cities_total: u8 = (0..n)
            .map(|q| s.non_metropolis_cities(q as u8).count_ones() as u8)
            .sum();
        let defense_total: u8 = (0..n).map(|q| s.active_knight_strength(q as u8)).sum();
        let my_str = s.active_knight_strength(p);
        if my_vuln > 0 && (defense_total < cities_total || my_str == 0) {
            if let Some(a) = best_of(legal, |a| match a {
                Action::ActivateKnight(v) => Some(s.knight_level[*v as usize] as f32),
                _ => None,
            }) {
                return a;
            }
            if let Some(a) = best_of(legal, |a| match a {
                Action::PromoteKnight(v) if s.knight_active.contains(*v) => {
                    Some(10.0 + s.knight_level[*v as usize] as f32)
                }
                Action::RecruitKnight(v) => Some(knight_spot_value(s, p, *v)),
                _ => None,
            }) {
                return a;
            }
        }
        if robber_hurts_me(s, p)
            && let Some(a) = best_of(legal, |a| match a {
                Action::ChaseRobber { tile, victim, .. } => Some(robber_value(s, p, *tile, *victim)),
                _ => None,
            })
        {
            return a;
        }
    }
    let hand = s.players[p as usize].hand;
    // Trade surplus with the bank towards a city or settlement (or connected Explorer mission).
    if explorers && s.players[p as usize].coins >= 2 {
        for m in [2u8, 0, 1] {
            if s.players[p as usize].explorer_missions[m as usize] < 3
                && crate::sea::explorer_track_reached(s, p, m as usize)
            {
                let target = crate::greedy::explorer_mission_cost(m);
                if missing(hand, target) == 1
                    && let Some(t) = legal.iter().find(|a| match a {
                        Action::SpendCoinsResource(get) => hand[*get] < target[*get],
                        _ => false,
                    })
                {
                    return *t;
                }
            }
        }
    }
    for target in [cost::CITY, cost::SETTLEMENT] {
        if missing(hand, target) == 1 {
            let trade = legal.iter().find(|a| match a {
                Action::SpendCoinsResource(get) if caravans || explorers => {
                    hand[*get] < target[*get]
                        && (explorers
                            || s.wealthiest_owner != Some(p)
                            || s.players[p as usize].coins >= 6)
                }
                Action::FishTakeResource(get) if fishermen => hand[*get] < target[*get],
                Action::MaritimeTrade { give, get } => {
                    hand[*get] < target[*get]
                        && hand[*give] as i32 - target[*give] as i32
                            >= s.trade_ratio(p, *give) as i32
                }
                Action::MaritimeTradeGood {
                    give: TradeGood::Commodity(c),
                    get: TradeGood::Resource(get),
                } if citadels => {
                    let ps = &s.players[p as usize];
                    let lvl = ps.city_progress[track_for_commodity(*c).index()];
                    hand[*get] < target[*get] && (lvl >= 3 || s.cities[p as usize].is_empty())
                }
                _ => false,
            });
            if let Some(t) = trade {
                return *t;
            }
        }
    }
    if fishermen {
        if robber_hurts_me(s, p) && legal.contains(&Action::FishDriveOffRobber) {
            return Action::FishDriveOffRobber;
        }
        if legal.contains(&Action::FishBuyDevCard) {
            return Action::FishBuyDevCard;
        }
        if rng.below(2) == 0 {
            let f_roads: Vec<&Action> = legal
                .iter()
                .filter(|a| matches!(a, Action::FishBuildRoad(_)))
                .collect();
            if !f_roads.is_empty() {
                return *f_roads[rng.below(f_roads.len() as u32) as usize];
            }
        }
        if s.players[p as usize].fish >= 5 || rng.below(3) == 0 {
            if let Some(a) = best_of(legal, |a| match a {
                Action::FishSteal(q) => Some(
                    s.public_victory_points(*q) as f32 * 2.0
                        + s.players[*q as usize].hand.total() as f32,
                ),
                _ => None,
            }) {
                return a;
            }
        }
    }
    if citadels {
        let ps = &s.players[p as usize];
        if !s.cities[p as usize].is_empty()
            && let Some(t) = legal.iter().find(|a| match a {
                Action::MaritimeTradeGood {
                    give: TradeGood::Commodity(give_c),
                    get: TradeGood::Commodity(get_c),
                } => {
                    let get_lvl = ps.city_progress[track_for_commodity(*get_c).index()];
                    let give_lvl = ps.city_progress[track_for_commodity(*give_c).index()];
                    get_lvl < 5
                        && ps.commodities[*get_c] + 1 == get_lvl + 1
                        && (give_lvl >= 5
                            || ps.commodities[*give_c]
                                >= give_lvl + 1 + s.commodity_trade_ratio(p))
                }
                _ => false,
            })
        {
            return *t;
        }
        if rng.below(2) == 0
            && let Some(a) = best_of(legal, |a| match a {
                Action::ActivateKnight(v) => Some(s.knight_level[*v as usize] as f32),
                _ => None,
            })
        {
            return a;
        }
        if rng.below(3) == 0
            && let Some(a) = best_of(legal, |a| match a {
                Action::PromoteKnight(v) => Some(s.knight_level[*v as usize] as f32 + 5.0),
                Action::RecruitKnight(v) => Some(knight_spot_value(s, p, *v)),
                _ => None,
            })
        {
            return a;
        }
    }
    if caravans
        && rng.below(2) == 0
        && let Some(a) = best_of(legal, |a| match a {
            Action::BuildBridge(e) => Some(bridge_value(s, p, *e, BotPersona::Balanced)),
            _ => None,
        })
    {
        return a;
    }
    // Seafaring: sail towards islands when ships lead somewhere.
    if s.board.layout.has_ships()
        && rng.below(2) == 0
        && let Some(a) = best_of(legal, |a| match a {
            Action::BuildShip(e) => {
                let v = crate::sea::ship_value(s, p, *e);
                (v > 1.0).then_some(v)
            }
            _ => None,
        })
    {
        return a;
    }
    let dev_denom = if citadels { 5 } else { 2 };
    if legal.contains(&Action::BuyDevCard) && rng.below(dev_denom) == 0 {
        return Action::BuyDevCard;
    }
    let robbed = !(s.buildings_of(p) & s.topo().tile_vertex_mask[s.robber as usize]).is_empty();
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

    #[test]
    fn plays_citadels_games_cleanly() {
        let mut g =
            GameState::new(GameConfig::scenario(isle_core::Scenario::Citadels, 4), 7).unwrap();
        let mut m = MctsBot::with_config(7, small());
        let mut b: Vec<GreedyBot> = (0..3).map(|k| GreedyBot::new(k + 1)).collect();
        let [b1, b2, b3] = &mut b[..] else {
            unreachable!()
        };
        let mut bots: Vec<&mut dyn Bot> = vec![&mut m, b1, b2, b3];
        let out = play_game(&mut g, &mut bots, 1000, true);
        assert!(out.winner.is_some(), "citadels game with MctsBot did not finish");
    }

    #[test]
    fn plays_fishermen_and_event_deck_games_cleanly() {
        let config = GameConfig {
            event_deck: true,
            ..GameConfig::scenario(isle_core::Scenario::Fishermen, 4)
        };
        let mut g = GameState::new(config, 13).unwrap();
        let mut m = MctsBot::with_config(13, small());
        let mut b: Vec<GreedyBot> = (0..3).map(|k| GreedyBot::new(k + 10)).collect();
        let [b1, b2, b3] = &mut b[..] else {
            unreachable!()
        };
        let mut bots: Vec<&mut dyn Bot> = vec![&mut m, b1, b2, b3];
        let out = play_game(&mut g, &mut bots, 1000, true);
        assert!(out.winner.is_some(), "fishermen game with MctsBot did not finish");
    }
}
