//! Hosts a match between one human seat (or none) and bots. Pure Rust so it
//! can be tested natively; `lib.rs` wraps it for JavaScript.

use isle_ai::{Bot, bot_by_name};
use isle_core::{
    Action, Event, GameConfig, GameState, Phase, ResourceSet, Scenario, TradeResponse,
};
use isle_proto::net::BotLevel;
use isle_proto::{ReplayPlayer, ReplayRecord};

#[derive(Debug, PartialEq, Eq)]
pub enum HostError {
    /// The action index refers to an outdated `legal_actions` result.
    Stale,
    NoSuchAction,
    Spectator,
    Config(String),
    /// The rules rejected the action (`RuleError` name).
    Rule(String),
}

impl HostError {
    pub fn code(&self) -> String {
        match self {
            HostError::Stale => "Stale".into(),
            HostError::NoSuchAction => "NoSuchAction".into(),
            HostError::Spectator => "Spectator".into(),
            HostError::Config(m) => format!("Config: {m}"),
            HostError::Rule(r) => r.clone(),
        }
    }
}

/// One bot move.
#[derive(Debug, serde::Serialize)]
pub struct BotStep {
    pub player: u8,
    pub action: Action,
    pub events: Vec<Event>,
}

pub struct MatchHost {
    pub(crate) state: GameState,
    pub(crate) human: Option<u8>,
    bots: Vec<Option<Box<dyn Bot>>>,
    legal: Vec<Action>,
    events: Vec<Event>,
    /// Incremented on every state change; guards against stale action indices.
    pub(crate) version: u32,
    seed: u64,
    bot_level: BotLevel,
    /// Every action applied, for replays.
    log: Vec<(u8, Action)>,
}

impl MatchHost {
    pub fn new(
        num_players: u8,
        seed: u64,
        human: Option<u8>,
        bot: &str,
        scenario: Scenario,
    ) -> Result<MatchHost, HostError> {
        Self::with_config(GameConfig::scenario(scenario, num_players), seed, human, bot)
    }

    pub fn with_config(
        config: GameConfig,
        seed: u64,
        human: Option<u8>,
        bot_name: &str,
    ) -> Result<MatchHost, HostError> {
        let num_players = config.num_players;
        let state =
            GameState::new(config, seed).map_err(|e| HostError::Config(format!("{e:?}")))?;
        let human = human.filter(|&h| h < num_players);
        let bots = (0..num_players)
            .map(|p| {
                if Some(p) == human {
                    Ok(None)
                } else {
                    bot_by_name(bot_name, (seed << 8) | p as u64)
                        .map(|mut b| {
                            if human.is_some() {
                                b.set_propose_trades(true);
                            }
                            Some(b as Box<dyn Bot>)
                        })
                        .ok_or_else(|| HostError::Config(format!("unknown bot {bot_name}")))
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(MatchHost {
            state,
            human,
            bots,
            legal: Vec::with_capacity(128),
            events: Vec::new(),
            version: 0,
            seed,
            bot_level: BotLevel::of_bot_name(bot_name),
            log: Vec::new(),
        })
    }

    /// The game so far as a replay; the human seat is called `human_name`.
    pub fn replay(&self, human_name: &str) -> ReplayRecord {
        let players = (0..self.state.config.num_players)
            .map(|p| {
                if Some(p) == self.human {
                    ReplayPlayer {
                        name: human_name.into(),
                        bot: None,
                    }
                } else {
                    ReplayPlayer {
                        name: String::new(),
                        bot: Some(self.bot_level),
                    }
                }
            })
            .collect();
        ReplayRecord::new(
            self.state.config,
            self.seed,
            players,
            &self.log,
            self.state.winner,
        )
    }

    /// Recomputes and returns the human's legal actions.
    pub fn legal_actions(&mut self) -> &[Action] {
        match self.human {
            Some(h) => self.state.legal_actions(h, &mut self.legal),
            None => self.legal.clear(),
        }
        &self.legal
    }

    /// Plays `legal_actions()[index]` computed at `version`.
    pub fn apply_index(&mut self, version: u32, index: usize) -> Result<&[Event], HostError> {
        if version != self.version {
            return Err(HostError::Stale);
        }
        let action = *self
            .legal_actions()
            .get(index)
            .ok_or(HostError::NoSuchAction)?;
        self.apply_human(action)
    }

    pub fn propose_trade(
        &mut self,
        give: ResourceSet,
        get: ResourceSet,
    ) -> Result<&[Event], HostError> {
        self.apply_human(Action::ProposeTrade { give, get })
    }

    fn apply_human(&mut self, action: Action) -> Result<&[Event], HostError> {
        let human = self.human.ok_or(HostError::Spectator)?;
        self.events.clear();
        self.state
            .apply_with(human, action, &mut self.events)
            .map_err(|e| HostError::Rule(format!("{e:?}")))?;
        self.log.push((human, action));
        self.version += 1;
        Ok(&self.events)
    }

    /// The bot that should act now, if any. `None` means the game waits for
    /// the human (or is over).
    pub fn next_bot(&self) -> Option<u8> {
        let s = &self.state;
        let n = s.num_players() as u8;
        let is_bot = |p: u8| Some(p) != self.human;
        match s.phase {
            Phase::GameOver => None,
            // Bots discard/pick gold first, in seat order; the human may act any time.
            Phase::Discard => (0..n).find(|&p| s.pending_discard[p as usize] > 0 && is_bot(p)),
            Phase::Gold { .. } => (0..n).find(|&p| s.pending_gold[p as usize] > 0 && is_bot(p)),
            Phase::Main if s.trade.is_some() => {
                let offer = s.trade.expect("checked");
                (0..n)
                    .find(|&q| {
                        q != s.current
                            && offer.responses[q as usize] == TradeResponse::Pending
                            && is_bot(q)
                    })
                    .or_else(|| {
                        let any_accepted = (0..n).any(|q| {
                            q != s.current
                                && offer.responses[q as usize] == TradeResponse::Accepted
                        });
                        let all_replied = (0..n).all(|q| {
                            q == s.current || offer.responses[q as usize] != TradeResponse::Pending
                        });
                        (is_bot(s.current) && (any_accepted || all_replied)).then_some(s.current)
                    })
            }
            _ => s.to_move().filter(|&p| is_bot(p)),
        }
    }

    /// Lets the next bot act once. Events are redacted for the human seat.
    pub fn step_bot(&mut self) -> Option<BotStep> {
        let p = self.next_bot()?;
        self.state.legal_actions(p, &mut self.legal);
        let bot = self.bots[p as usize].as_mut().expect("bot seat");
        let action = bot.choose(&self.state, p, &self.legal);
        self.events.clear();
        self.state
            .apply_with(p, action, &mut self.events)
            .expect("bots play legal actions");
        self.log.push((p, action));
        self.version += 1;
        let viewer = self.human;
        Some(BotStep {
            player: p,
            action,
            events: self.events.iter().map(|e| e.redacted_for(viewer)).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isle_ai::GreedyBot;
    use isle_core::Resource;

    /// Drives the human seat with a bot through the host API only.
    fn play_through_host(seed: u64, human: u8) -> MatchHost {
        let mut host = MatchHost::new(4, seed, Some(human), "greedy", Scenario::Classic).unwrap();
        let mut stand_in = GreedyBot::new(seed);
        let mut proposed_this_turn = false;
        for _ in 0..20_000 {
            if host.state.phase == Phase::GameOver {
                break;
            }
            if host.step_bot().is_some() {
                continue;
            }
            // The human's turn: occasionally offer a trade to exercise that path.
            let s = host.state;
            let hand = s.players[human as usize].hand;
            if s.phase == Phase::Main
                && s.current == human
                && s.trade.is_none()
                && !proposed_this_turn
            {
                proposed_this_turn = true;
                if let Some(give) = Resource::ALL.into_iter().find(|&r| hand[r] >= 2) {
                    let get = Resource::ALL.into_iter().find(|&r| r != give).unwrap();
                    host.propose_trade(ResourceSet::single(give, 1), ResourceSet::single(get, 1))
                        .unwrap();
                    continue;
                }
            }
            if s.current != human {
                proposed_this_turn = false;
            }
            let version = host.version;
            let legal = host.legal_actions().to_vec();
            assert!(!legal.is_empty(), "human has no move in {:?}", s.phase);
            let choice = stand_in.choose(&host.state, human, &legal);
            let index = legal.iter().position(|a| *a == choice).unwrap();
            host.apply_index(version, index).unwrap();
        }
        host
    }

    #[test]
    fn full_games_through_the_host_api() {
        for seed in 0..12 {
            let host = play_through_host(seed, (seed % 4) as u8);
            assert_eq!(
                host.state.phase,
                Phase::GameOver,
                "seed {seed} did not finish"
            );
            host.state.check_invariants().unwrap();
            // The recorded log replays to the same final state.
            let replay = host.replay("Me");
            assert_eq!(replay.players[(seed % 4) as usize].name, "Me");
            assert_eq!(replay.play(|_, _, _, _| {}).unwrap(), host.state);
        }
    }

    #[test]
    fn stale_indices_are_rejected() {
        let mut host = MatchHost::new(3, 5, Some(1), "greedy", Scenario::Classic).unwrap();
        assert!(host.step_bot().is_some()); // seat 0 places
        let old = host.version - 1;
        host.legal_actions();
        assert!(matches!(host.apply_index(old, 0), Err(HostError::Stale)));
        // Seat 0 still has to place its road before the human may act.
        while host.next_bot().is_some() {
            host.step_bot();
        }
        let v = host.version;
        assert!(!host.legal_actions().is_empty());
        host.apply_index(v, 0).unwrap();
        assert!(matches!(host.apply_index(v, 0), Err(HostError::Stale)));
    }

    #[test]
    fn bot_events_hide_private_details() {
        let mut host = MatchHost::new(4, 9, Some(0), "greedy", Scenario::Classic).unwrap();
        let mut stand_in = GreedyBot::new(1);
        for _ in 0..5_000 {
            if host.state.phase == Phase::GameOver {
                break;
            }
            if let Some(step) = host.step_bot() {
                for e in &step.events {
                    match e {
                        Event::DevCardBought { card, .. } => assert!(card.is_none()),
                        Event::Stolen {
                            victim, resource, ..
                        } if *victim != 0 => assert!(resource.is_none()),
                        _ => {}
                    }
                }
                continue;
            }
            let v = host.version;
            let legal = host.legal_actions().to_vec();
            let choice = stand_in.choose(&host.state, 0, &legal);
            let i = legal.iter().position(|a| *a == choice).unwrap();
            host.apply_index(v, i).unwrap();
        }
    }

    #[test]
    fn spectator_matches_run_bots_only() {
        let mut host = MatchHost::new(4, 3, None, "greedy", Scenario::Classic).unwrap();
        assert!(host.legal_actions().is_empty());
        let mut steps = 0;
        while host.step_bot().is_some() {
            steps += 1;
        }
        assert_eq!(host.state.phase, Phase::GameOver);
        assert!(steps > 50);
    }
}
