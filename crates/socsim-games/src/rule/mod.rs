mod book;
mod builtin;

pub use book::RuleBook;
pub use builtin::{Fixed, TitForTat, UniformRandom};

use socsim_core::{AgentId, SimRng};
use thiserror::Error;

use crate::{DecisionContext, ExperimentState, LearnContext, RuleKindId, StageGame};

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
}

#[derive(Clone, Copy, Debug)]
pub struct RuleInitContext {
    pub agent: AgentId,
    pub kind: RuleKindId,
    pub generation: u32,
    pub init_seed: u64,
}
