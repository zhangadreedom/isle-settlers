//! # isle-core
//!
//! Deterministic rules engine for Isle Settlers, a hex-island game of
//! settling, trading and building.
//!
//! * Board topologies are compile-time tables ([`topology`]): the base
//!   island for 2–4 players and the extended island for 5–6.
//! * [`GameState`] is a heap-free `Copy` struct: cloning it is a `memcpy`,
//!   which is what tree search needs.
//! * All randomness comes from the state's own seeded [`Rng`], so a game is
//!   reproducible from `(config, seed, actions)` on every target, including
//!   WebAssembly.
//! * [`GameState::apply`] validates and applies an [`Action`];
//!   [`GameState::apply_with`] also reports [`Event`]s. Errors leave the
//!   state untouched.
//!
//! ```
//! use isle_core::{GameConfig, GameState};
//!
//! let mut game = GameState::new(GameConfig::default(), 42).unwrap();
//! let mut moves = Vec::new();
//! let player = game.to_move().unwrap();
//! game.legal_actions(player, &mut moves);
//! game.apply(player, moves[0]).unwrap();
//! ```

pub mod action;
pub mod bitset;
pub mod board;
mod legal;
pub mod resources;
pub mod rng;
mod rules;
pub mod state;
pub mod topology;

pub use action::{Action, Event, EventSink, RuleError, TradeGood};
pub use bitset::BitSet;
pub use board::{Board, BoardError, CustomMap, FishingGround, MAX_FISHING_GROUNDS};
pub use resources::{CityTrack, Commodity, CommoditySet, HarborKind, Resource, ResourceSet, Terrain};
pub use rng::Rng;
pub use state::{
    BARBARIAN_STEPS, CANONICAL_DICE_DECK, ConfigError, DICE_DECK_RESHUFFLE_AT, DICE_DECK_SIZE,
    DevCard, GameConfig, GameState, MAX_BRIDGES, MAX_CAMELS, MAX_COINS, MAX_FISH_HAND,
    MAX_KNIGHTS_PER_LEVEL, MAX_PLAYERS, MAX_SHIPS, Phase, PlayerState, Scenario, TradeOffer,
    TradeResponse, WEALTHIEST_MIN_COINS,
};
pub use topology::{BoardLayout, Topology};
