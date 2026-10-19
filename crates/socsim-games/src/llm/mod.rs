//! Two-stage LLM decisions with parsing, retry, replay, and manifest support.
//!
//! See the [LLM-decisions guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#llm-decisions-feature-llm).

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
pub use render::{
    CountsRenderer, DescribeGame, DescribedRenderer, GameRenderer, MarkdownTableRenderer,
};
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
    /// Creates an empty rule book.
    pub fn new() -> Self {
        Self {
            rules: BTreeMap::new(),
        }
    }

    /// Registers `rule` under its canonical policy-kind name.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError::DuplicateKind`] if `kind` is already registered.
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
/// Backend-independent prompt prepared for one LLM attempt.
pub struct DecisionRequest {
    /// Optional system instruction passed separately to the backend.
    pub system: Option<String>,
    /// User prompt whose exact contents participate in the replay-cache key.
    pub prompt: String,
    /// Zero-based attempt number; retries increment it.
    pub attempt: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Backend response normalized for rule resolution and recording.
pub struct LlmReply {
    /// Completion text presented to the action parser.
    pub text: String,
    /// Optional backend reasoning text, retained for records but not parsing.
    pub thinking: Option<String>,
    /// Optional token counts reported by the backend.
    pub usage: Option<socsim_llm::TokenUsage>,
    /// Whether the response came from [`ResponseStore`] rather than a backend call.
    pub cache_hit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
/// Category assigned to a response that cannot be accepted as an action.
pub enum ViolationKind {
    /// The response does not satisfy the parser's required syntax.
    Format,
    /// A parsed label does not name an available action.
    Label,
    /// A parsed numeric value lies outside the role's action bounds.
    Range,
    /// The parsed value is rejected by [`StageGame::check_action`].
    InvalidAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Structured reason why an LLM response was rejected.
pub struct Violation {
    /// Machine-readable rejection category.
    pub kind: ViolationKind,
    /// Human-readable detail suitable for retry feedback and logs.
    pub message: String,
}

/// Splits an LLM decision into prompt preparation and response resolution.
///
/// Policy authors implement this trait and register shared instances in an
/// [`LlmRuleBook`]. [`LlmDriver`] calls [`prepare`](Self::prepare), batches and
/// replays the resulting requests through [`LlmExecutor`], then calls
/// [`resolve`](Self::resolve). Implementations keep conversation state in
/// `X::PolicyState`; they must not perform backend I/O themselves and must use
/// only [`ResolveContext::fallback_rng`] for randomized fallbacks.
///
/// See the [two-stage decision guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#two-stage-decisions-prepare--batch--resolve).
pub trait LlmDecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    /// Returns the stable human-readable policy name used in records.
    fn name(&self) -> &str;

    /// Creates conversation state for one assigned agent.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError`] for invalid configuration or an incompatible
    /// experiment policy-state type.
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;

    /// Builds the first request for one decision without mutating policy state.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError`] when required state or observation data is absent.
    fn prepare(
        &self,
        state: &X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
    ) -> Result<DecisionRequest, RuleError>;

    /// Parses one reply and accepts, retries, or falls back to an action.
    ///
    /// Implementations update durable conversation state here. An accepted or
    /// fallback action must be valid for every role represented by the decision
    /// context; retry requests must advance their attempt counter.
    fn resolve(
        &self,
        state: &mut X::PolicyState,
        req: &DecisionRequest,
        reply: &LlmReply,
        rctx: &mut ResolveContext<'_, G>,
    ) -> Resolution<G::Action>;

    /// Updates LLM policy state once after the step's encounters complete.
    ///
    /// The default performs no learning.
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
    /// Stage game used to validate parsed or fallback actions.
    pub game: &'a G,
    /// Role for this decision, or `None` when one action serves several roles.
    pub role: Option<usize>,
    /// The agent's held action from the previous step.
    pub previous: &'a G::Action,
    /// Decision-local random stream used only by randomized fallback policies.
    pub fallback_rng: &'a mut SimRng,
    /// The current step.
    pub t: u64,
}

/// Result of interpreting one LLM reply.
pub enum Resolution<A> {
    /// The reply parsed to a valid action.
    Accept {
        /// Action to commit to the world.
        action: A,
        /// Non-fatal parser properties retained in the decision record.
        flags: ParseFlags,
    },
    /// The reply was invalid and another backend attempt should be made.
    Retry(DecisionRequest),
    /// Retries are exhausted and the configured fallback action is used.
    GiveUp {
        /// Valid action selected by the fallback policy.
        action: A,
        /// Fallback policy used for diagnostics.
        fallback: FallbackKind,
        /// Last response violation that caused the fallback.
        violation: Violation,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
/// Terminal action policy after response-level retries are exhausted.
pub enum Fallback<A> {
    /// Reuse the action held before this decision; this is the default.
    #[default]
    Previous,
    /// Draw uniformly from the finite action space or continuous bounds.
    UniformRandom,
    /// Use a caller-provided action after validating it against the game.
    Specified(A),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
/// Recorded non-generic identity of the fallback that produced an action.
pub enum FallbackKind {
    /// The previous held action was reused.
    Previous,
    /// A valid action was sampled uniformly.
    UniformRandom,
    /// A configured constant action was used.
    Specified,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Retry limit and terminal fallback for an [`LlmPolicy`].
pub struct RetryProtocol<A> {
    /// Number of additional requests allowed after the initial attempt.
    pub max_retries: u32,
    /// Action-selection policy used after all permitted retries fail.
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
