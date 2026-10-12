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
pub enum RuleError {
    #[error("policy state does not contain slot {0}")]
    MissingSlot(&'static str),
    #[error("unknown rule kind: {0}")]
    UnknownKind(String),
    #[error("duplicate rule kind: {0}")]
    DuplicateKind(String),
    #[error("rule returned an invalid action: {0}")]
    InvalidAction(String),
    #[error("rule cannot be used with this game: {0}")]
    Unsupported(String),
    #[error("LLM backend error: {0}")]
    Backend(String),
    #[error("agent {agent} holds more than one role in a per-step decision")]
    AmbiguousRole { agent: u64 },
    /// A component of a payoff gradient is NaN or infinite.
    #[error("component {dim} of the payoff gradient is not finite")]
    NonFiniteGradient { dim: usize },
    /// No converged best response could be obtained during a decision.
    #[error("best response unavailable: {0}")]
    BestResponse(String),
}

pub trait DecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;
    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<G::Action, RuleError>;
    fn learn_step(
        &self,
        _state: &mut X::PolicyState,
        _ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        Ok(())
    }
    fn learn_encounter(
        &self,
        _state: &mut X::PolicyState,
        _outcome: &EncounterOutcome<'_, G::Action>,
        _ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        Ok(())
    }
    fn needs_encounter_learning(&self) -> bool {
        false
    }
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
pub struct RuleInitContext {
    pub agent: AgentId,
    pub kind: RuleKindId,
    pub generation: u32,
    pub init_seed: u64,
}
