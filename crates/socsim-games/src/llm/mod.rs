#[cfg(feature = "engine")]
mod driver;
mod executor;
mod manifest;
mod obs;
mod parse;
mod policy;
mod render;
mod store;

#[cfg(feature = "engine")]
pub use driver::{assemble_with_llm, LlmDriver, LlmSetup};
#[cfg(feature = "live")]
pub use executor::live_backend_factory;
pub use executor::{BackendFactory, ExecutorOptions, LlmExecutor};
pub use manifest::{HasLlmManifest, LlmReplayManifest, WithLlm};
pub use obs::{ActionCounts, CountsObs, ObservationProvider, OwnHistory, OwnHistoryObs};
pub use parse::{ActionParser, LabelParser, NumberParser, OutOfRange, ParseFlags, ParseOutcome};
pub use policy::{ConversationTurn, LlmConversation, LlmPolicy};
pub use render::{CountsRenderer, DescribeGame, GameRenderer, MarkdownTableRenderer};
pub use store::{
    BackendIdentity, CacheKeyV1, MergeReport, ReplayMode, ResponseRecord, ResponseStore, StoreError,
};

#[cfg(feature = "engine")]
pub use manifest::verify_snapshot;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use socsim_core::SimRng;

use crate::{
    DecisionContext, ExperimentState, LearnContext, RuleError, RuleInitContext, StageGame,
};

/// LLM-backed rules indexed by their canonical policy-kind name.
pub struct LlmRuleBook<G: StageGame, X: ExperimentState> {
    pub(crate) rules: BTreeMap<String, Arc<dyn LlmDecisionRule<G, X>>>,
}

impl<G: StageGame, X: ExperimentState> Clone for LlmRuleBook<G, X> {
    fn clone(&self) -> Self {
        Self {
            rules: self.rules.clone(),
        }
    }
}

impl<G: StageGame, X: ExperimentState> Default for LlmRuleBook<G, X> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: StageGame, X: ExperimentState> LlmRuleBook<G, X> {
    pub fn new() -> Self {
        Self {
            rules: BTreeMap::new(),
        }
    }

    pub fn register(
        &mut self,
        kind: &str,
        rule: Arc<dyn LlmDecisionRule<G, X>>,
    ) -> Result<(), RuleError> {
        if self.rules.contains_key(kind) {
            return Err(RuleError::DuplicateKind(kind.to_owned()));
        }
        self.rules.insert(kind.to_owned(), rule);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DecisionRequest {
    pub system: Option<String>,
    pub prompt: String,
    pub attempt: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LlmReply {
    pub text: String,
    pub thinking: Option<String>,
    pub usage: Option<socsim_llm::TokenUsage>,
    pub cache_hit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ViolationKind {
    Format,
    Label,
    Range,
    InvalidAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    pub kind: ViolationKind,
    pub message: String,
}

pub trait LlmDecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;

    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;

    fn prepare(
        &self,
        state: &X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
    ) -> Result<DecisionRequest, RuleError>;

    fn resolve(
        &self,
        state: &mut X::PolicyState,
        req: &DecisionRequest,
        reply: &LlmReply,
        rctx: &mut ResolveContext<'_, G>,
    ) -> Resolution<G::Action>;

    fn learn_step(
        &self,
        _state: &mut X::PolicyState,
        _ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        Ok(())
    }
}

/// Context passed to [`LlmDecisionRule::resolve`].
///
/// `fallback_rng` is derived from the decision unit and never consumes the
/// engine's step stream.
pub struct ResolveContext<'a, G: StageGame> {
    pub game: &'a G,
    pub role: Option<usize>,
    /// The agent's held action from the previous step.
    pub previous: &'a G::Action,
    pub fallback_rng: &'a mut SimRng,
    /// The current step.
    pub t: u64,
}

pub enum Resolution<A> {
    Accept {
        action: A,
        flags: ParseFlags,
    },
    Retry(DecisionRequest),
    GiveUp {
        action: A,
        fallback: FallbackKind,
        violation: Violation,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Fallback<A> {
    #[default]
    Previous,
    UniformRandom,
    Specified(A),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FallbackKind {
    Previous,
    UniformRandom,
    Specified,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetryProtocol<A> {
    pub max_retries: u32,
    pub fallback: Fallback<A>,
}

impl<A> Default for RetryProtocol<A> {
    fn default() -> Self {
        Self {
            max_retries: 1,
            fallback: Fallback::Previous,
        }
    }
}
