use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

use crate::learn::{
    automaton::AutomatonState, fictitious::FictitiousBelief, logit::LogitState,
    roth_erev::RothErevState,
};

/// Experiment-specific state and per-agent policy-state representation.
///
/// Experiment authors implement this trait. [`crate::GameWorld`] owns one `X`,
/// decision and learning contexts call [`view_for`](Self::view_for), and rule
/// initialization constructs `PolicyState` values through [`Slot`]. The view
/// must enforce the intended information boundary for the requesting agent.
pub trait ExperimentState: Send + Sync + 'static {
    /// Snapshot-resident state held separately for every agent.
    type PolicyState: Clone + Send + Sync + Debug + 'static;
    /// Read-only experiment view exposed to one agent for the duration of a borrow.
    type View<'a>
    where
        Self: 'a;

    /// Produces the private experiment view visible to `me`.
    fn view_for(&self, me: AgentId) -> Self::View<'_>;
}

impl ExperimentState for () {
    type PolicyState = BuiltinPolicyState;
    type View<'a> = ();

    fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Zero-sized policy state for rules that retain no per-agent data.
pub struct Stateless;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Per-agent memory used by Tit-for-Tat.
pub struct TftState {
    /// Most recently observed opponent action, or `None` before any encounter.
    pub last_opponent: Option<usize>,
}

/// State of the continuous-game rules: the action this agent last played.
///
/// `last` is written after each step from the agent's recorded action, so an
/// action replaced by a fallback is the one remembered. It is `None` until the
/// agent has played once.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContinuousState {
    /// The action this agent played in its last played step, or `None`.
    pub last: Option<Vec<f64>>,
}

/// Fictitious-play beliefs shared across decisions or separated by opponent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeliefState {
    pub(crate) global: FictitiousBelief,
    pub(crate) by_opponent: Vec<(AgentId, FictitiousBelief)>,
}

impl BeliefState {
    pub(crate) fn new(global: FictitiousBelief) -> Self {
        Self {
            global,
            by_opponent: Vec::new(),
        }
    }

    /// Returns the belief used outside per-encounter decisions.
    pub fn global(&self) -> &FictitiousBelief {
        &self.global
    }

    /// Returns opponent-specific beliefs in ascending agent-ID order.
    pub fn by_opponent(&self) -> &[(AgentId, FictitiousBelief)] {
        &self.by_opponent
    }
}

/// Roth--Erev propensities shared across decisions or separated by opponent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropensityState {
    pub(crate) global: RothErevState,
    pub(crate) by_opponent: Vec<(AgentId, RothErevState)>,
}

impl PropensityState {
    pub(crate) fn new(global: RothErevState) -> Self {
        Self {
            global,
            by_opponent: Vec::new(),
        }
    }

    /// Returns the propensity state used outside per-encounter decisions.
    pub fn global(&self) -> &RothErevState {
        &self.global
    }

    /// Returns opponent-specific propensities in ascending agent-ID order.
    pub fn by_opponent(&self) -> &[(AgentId, RothErevState)] {
        &self.by_opponent
    }
}

/// Logit estimates shared across decisions or separated by opponent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EstimateState {
    pub(crate) global: LogitState,
    pub(crate) by_opponent: Vec<(AgentId, LogitState)>,
}

impl EstimateState {
    pub(crate) fn new(global: LogitState) -> Self {
        Self {
            global,
            by_opponent: Vec::new(),
        }
    }

    /// Returns the estimate used outside per-encounter decisions.
    pub fn global(&self) -> &LogitState {
        &self.global
    }

    /// Returns opponent-specific estimates in ascending agent-ID order.
    pub fn by_opponent(&self) -> &[(AgentId, LogitState)] {
        &self.by_opponent
    }
}

/// Repeated-game automata shared across decisions or separated by opponent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomatonPolicyState {
    pub(crate) global: AutomatonState,
    pub(crate) by_opponent: Vec<(AgentId, AutomatonState)>,
}

impl AutomatonPolicyState {
    pub(crate) fn new(global: AutomatonState) -> Self {
        Self {
            global,
            by_opponent: Vec::new(),
        }
    }

    /// Returns the automaton used outside per-encounter decisions.
    pub fn global(&self) -> &AutomatonState {
        &self.global
    }

    /// Returns opponent-specific automata in ascending agent-ID order.
    pub fn by_opponent(&self) -> &[(AgentId, AutomatonState)] {
        &self.by_opponent
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
/// Built-in tagged policy-state representation used by `ExperimentState for ()`.
pub enum BuiltinPolicyState {
    /// State for fixed and uniformly random rules.
    Stateless(Stateless),
    /// State for Tit-for-Tat.
    TitForTat(TftState),
    /// State for fictitious play.
    Belief(BeliefState),
    /// State for Roth--Erev reinforcement learning.
    Propensity(PropensityState),
    /// State for logit choice.
    Estimate(EstimateState),
    /// State for a repeated-game automaton.
    Automaton(AutomatonPolicyState),
    #[cfg(feature = "llm")]
    /// Persistent conversation state for an LLM policy.
    Llm(crate::llm::LlmConversation),
    /// State of `DampedBestResponse` and `GradientAscent`.
    Continuous(ContinuousState),
}

/// Embeds one rule-specific state type in an experiment's policy-state type.
///
/// Experiment authors implement `Slot<T>` for every decision-rule state `T`
/// they intend to use. Rules call [`slot`](Self::slot) and
/// [`slot_mut`](Self::slot_mut) while deciding or learning, and call
/// [`from_slot`](Self::from_slot) during agent initialization. Implementations
/// must make `from_slot(value).slot()` return that same value and return `None`
/// only when the current variant holds another rule's state.
///
/// See the [policy-state guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md#policystate-and-slot).
pub trait Slot<T> {
    /// Borrows the embedded state when the current variant contains `T`.
    fn slot(&self) -> Option<&T>;
    /// Mutably borrows the embedded state when the current variant contains `T`.
    fn slot_mut(&mut self) -> Option<&mut T>;
    /// Wraps freshly initialized rule state in the experiment policy-state type.
    fn from_slot(value: T) -> Self;
}

impl Slot<Stateless> for BuiltinPolicyState {
    fn slot(&self) -> Option<&Stateless> {
        match self {
            Self::Stateless(value) => Some(value),
            Self::TitForTat(_)
            | Self::Belief(_)
            | Self::Propensity(_)
            | Self::Estimate(_)
            | Self::Automaton(_)
            | Self::Continuous(_) => None,
            #[cfg(feature = "llm")]
            Self::Llm(_) => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut Stateless> {
        match self {
            Self::Stateless(value) => Some(value),
            Self::TitForTat(_)
            | Self::Belief(_)
            | Self::Propensity(_)
            | Self::Estimate(_)
            | Self::Automaton(_)
            | Self::Continuous(_) => None,
            #[cfg(feature = "llm")]
            Self::Llm(_) => None,
        }
    }

    fn from_slot(value: Stateless) -> Self {
        Self::Stateless(value)
    }
}

impl Slot<TftState> for BuiltinPolicyState {
    fn slot(&self) -> Option<&TftState> {
        match self {
            Self::TitForTat(value) => Some(value),
            Self::Stateless(_)
            | Self::Belief(_)
            | Self::Propensity(_)
            | Self::Estimate(_)
            | Self::Automaton(_)
            | Self::Continuous(_) => None,
            #[cfg(feature = "llm")]
            Self::Llm(_) => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut TftState> {
        match self {
            Self::TitForTat(value) => Some(value),
            Self::Stateless(_)
            | Self::Belief(_)
            | Self::Propensity(_)
            | Self::Estimate(_)
            | Self::Automaton(_)
            | Self::Continuous(_) => None,
            #[cfg(feature = "llm")]
            Self::Llm(_) => None,
        }
    }

    fn from_slot(value: TftState) -> Self {
        Self::TitForTat(value)
    }
}

#[diagnostic::do_not_recommend]
impl Slot<BeliefState> for BuiltinPolicyState {
    fn slot(&self) -> Option<&BeliefState> {
        match self {
            Self::Belief(value) => Some(value),
            _ => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut BeliefState> {
        match self {
            Self::Belief(value) => Some(value),
            _ => None,
        }
    }

    fn from_slot(value: BeliefState) -> Self {
        Self::Belief(value)
    }
}

#[diagnostic::do_not_recommend]
impl Slot<PropensityState> for BuiltinPolicyState {
    fn slot(&self) -> Option<&PropensityState> {
        match self {
            Self::Propensity(value) => Some(value),
            _ => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut PropensityState> {
        match self {
            Self::Propensity(value) => Some(value),
            _ => None,
        }
    }

    fn from_slot(value: PropensityState) -> Self {
        Self::Propensity(value)
    }
}

#[diagnostic::do_not_recommend]
impl Slot<EstimateState> for BuiltinPolicyState {
    fn slot(&self) -> Option<&EstimateState> {
        match self {
            Self::Estimate(value) => Some(value),
            _ => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut EstimateState> {
        match self {
            Self::Estimate(value) => Some(value),
            _ => None,
        }
    }

    fn from_slot(value: EstimateState) -> Self {
        Self::Estimate(value)
    }
}

#[diagnostic::do_not_recommend]
impl Slot<AutomatonPolicyState> for BuiltinPolicyState {
    fn slot(&self) -> Option<&AutomatonPolicyState> {
        match self {
            Self::Automaton(value) => Some(value),
            _ => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut AutomatonPolicyState> {
        match self {
            Self::Automaton(value) => Some(value),
            _ => None,
        }
    }

    fn from_slot(value: AutomatonPolicyState) -> Self {
        Self::Automaton(value)
    }
}

#[cfg(feature = "llm")]
#[diagnostic::do_not_recommend]
impl Slot<crate::llm::LlmConversation> for BuiltinPolicyState {
    fn slot(&self) -> Option<&crate::llm::LlmConversation> {
        match self {
            Self::Llm(value) => Some(value),
            Self::Stateless(_)
            | Self::TitForTat(_)
            | Self::Belief(_)
            | Self::Propensity(_)
            | Self::Estimate(_)
            | Self::Automaton(_)
            | Self::Continuous(_) => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut crate::llm::LlmConversation> {
        match self {
            Self::Llm(value) => Some(value),
            Self::Stateless(_)
            | Self::TitForTat(_)
            | Self::Belief(_)
            | Self::Propensity(_)
            | Self::Estimate(_)
            | Self::Automaton(_)
            | Self::Continuous(_) => None,
        }
    }

    fn from_slot(value: crate::llm::LlmConversation) -> Self {
        Self::Llm(value)
    }
}

#[diagnostic::do_not_recommend]
impl Slot<ContinuousState> for BuiltinPolicyState {
    fn slot(&self) -> Option<&ContinuousState> {
        match self {
            Self::Continuous(value) => Some(value),
            _ => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut ContinuousState> {
        match self {
            Self::Continuous(value) => Some(value),
            _ => None,
        }
    }

    fn from_slot(value: ContinuousState) -> Self {
        Self::Continuous(value)
    }
}
