//! Decision rules and rule registries used during the Decision and Reward phases.
//!
//! See the [decision-rules guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#decision-rules).

mod book;
mod builtin;
mod catalog;
mod continuous;
mod learning;

pub use book::RuleBook;
pub use builtin::{Fixed, TitForTat, UniformRandom};
pub use catalog::KindCatalog;
pub use continuous::{DampedBestResponse, GradientAscent};
pub use learning::{Automaton, FictitiousPlay, LogitChoice, Mixed, MyopicBestResponse, RothErev};

use socsim_core::{AgentId, SimRng};
use thiserror::Error;

use crate::{
    DecisionContext, EncounterOutcome, ExperimentState, LearnContext, RuleKindId, StageGame,
};

#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
/// Failure to configure, initialize, execute, or update a decision rule.
pub enum RuleError {
    /// The experiment's policy-state enum lacks the state required by a rule.
    #[error("policy state does not contain slot {0}")]
    MissingSlot(&'static str),
    /// An assignment or lookup named a rule kind absent from its rule book.
    #[error("unknown rule kind: {0}")]
    UnknownKind(String),
    /// A rule book or combined kind catalog contains the same kind twice.
    #[error("duplicate rule kind: {0}")]
    DuplicateKind(String),
    /// A rule produced an action rejected by the stage game.
    #[error("rule returned an invalid action: {0}")]
    InvalidAction(String),
    /// A rule's prerequisites are incompatible with the selected game or mode.
    #[error("rule cannot be used with this game: {0}")]
    Unsupported(String),
    /// An LLM-backed rule could not obtain or process a backend response.
    #[error("LLM backend error: {0}")]
    Backend(String),
    /// One per-step decision would have to serve different roles for an agent.
    #[error("agent {agent} holds more than one role in a per-step decision")]
    AmbiguousRole {
        /// Numeric identifier of the ambiguously seated agent.
        agent: u64,
    },
    /// A component of a payoff gradient is NaN or infinite.
    #[error("component {dim} of the payoff gradient is not finite")]
    NonFiniteGradient {
        /// Zero-based component of the invalid gradient.
        dim: usize,
    },
    /// No converged best response could be obtained during a decision.
    #[error("best response unavailable: {0}")]
    BestResponse(String),
}

/// Chooses actions for one named policy kind and optionally learns from play.
///
/// Rule authors implement this trait and register shared instances in a
/// [`RuleBook`]. The decision mechanism calls [`init_state`](Self::init_state)
/// once per assigned agent, [`decide`](Self::decide) for each decision unit,
/// and the learning callbacks after outcomes are available. Implementations
/// must keep per-agent mutable data in `X::PolicyState`; the rule object is
/// shared and must not make results depend on call order beyond the supplied
/// context and random stream.
///
/// See the [decision-rules guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#decision-rules).
pub trait DecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    /// Returns the stable human-readable name recorded for this rule.
    fn name(&self) -> &str;
    /// Creates policy state for one assigned agent.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError`] if the game is unsupported, configuration is
    /// invalid, or `X::PolicyState` cannot represent the required state.
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;
    /// Chooses one action from the information allowed by `ctx`.
    ///
    /// The implementation must consume randomness only through `rng` and must
    /// return an action accepted for the role or roles represented by `ctx`.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError`] when required state is absent or a valid action
    /// cannot be produced.
    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<G::Action, RuleError>;
    /// Updates policy state once after all encounters in a step.
    ///
    /// The default performs no update. This callback is used for rules whose
    /// learning signal is the agent's step-level record.
    fn learn_step(
        &self,
        _state: &mut X::PolicyState,
        _ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        Ok(())
    }
    /// Updates policy state after one encounter involving the agent.
    ///
    /// The default performs no update. Implementations returning `true` from
    /// [`needs_encounter_learning`](Self::needs_encounter_learning) receive
    /// calls in encounter order.
    fn learn_encounter(
        &self,
        _state: &mut X::PolicyState,
        _outcome: &EncounterOutcome<'_, G::Action>,
        _ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        Ok(())
    }
    /// Reports whether the driver must invoke the per-encounter callback.
    ///
    /// The default is `false`.
    fn needs_encounter_learning(&self) -> bool {
        false
    }
    /// Reports whether decisions require pair-specific action history.
    ///
    /// The default is `false`; returning `true` makes assembly allocate and
    /// maintain pair history.
    fn needs_pair_history(&self) -> bool {
        false
    }
    /// Returns the current mixed strategy for `role`, when the rule can report
    /// one without mutating policy state or consuming randomness.
    fn mixed(&self, _state: &X::PolicyState, _role: usize, _game: &G) -> Option<Vec<f64>> {
        None
    }
}

fn pure_strategy(n_actions: usize, action: usize) -> Option<Vec<f64>> {
    if action >= n_actions {
        return None;
    }
    let mut probabilities = vec![0.0; n_actions];
    probabilities[action] = 1.0;
    Some(probabilities)
}

#[derive(Clone, Copy, Debug)]
/// Deterministic metadata supplied when a policy state is first created.
pub struct RuleInitContext {
    /// Agent that will own the initialized state.
    pub agent: AgentId,
    /// Interned identifier of the assigned policy kind.
    pub kind: RuleKindId,
    /// Generation counter, initially zero and incremented by replacement dynamics.
    pub generation: u32,
    /// Agent- and generation-specific seed for deterministic initialization.
    pub init_seed: u64,
}
