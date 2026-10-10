pub mod game;
pub mod learn;
pub mod mixed;
pub mod solve;

pub use game::catalog;
pub use mixed::MixedStrategy;

#[cfg(feature = "llm")]
pub mod llm;

pub use game::{ActionError, FiniteGame, MatrixError, MatrixGame, NormalFormGame, StageGame};
pub use learn::{automaton::AutomatonKind, logit::EstimateMode, TieBreak};

#[cfg(feature = "core")]
pub mod assemble;
#[cfg(feature = "core")]
pub mod error;
#[cfg(feature = "core")]
pub mod matching;
#[cfg(feature = "core")]
pub mod mechanism;
#[cfg(feature = "core")]
pub mod outcome;
#[cfg(feature = "core")]
pub mod rule;
#[cfg(feature = "core")]
pub mod view;
#[cfg(feature = "core")]
pub mod world;

#[cfg(feature = "engine")]
pub use assemble::assemble;
#[cfg(feature = "core")]
pub use assemble::{
    build_world, build_world_with, AssignGroup, AssignKind, Assignment, InitialActions,
    OnInvalidAction, PayoffAggregation, PlayOptions,
};
#[cfg(feature = "core")]
pub use error::GameError;
#[cfg(feature = "core")]
pub use matching::{FixedPlayers, MatchContext, Matching, MatchingState, RoundRobin};
#[cfg(feature = "core")]
pub use mechanism::RuleDriver;
#[cfg(feature = "core")]
pub use outcome::{NormalFormOutcome, OutcomeContext, OutcomeRule};
#[cfg(feature = "core")]
pub use rule::{
    Automaton, DecisionRule, FictitiousPlay, Fixed, KindCatalog, LogitChoice, Mixed,
    MyopicBestResponse, RothErev, RuleBook, RuleError, RuleInitContext, TitForTat, UniformRandom,
};
#[cfg(feature = "core")]
pub use view::{
    DecisionContext, DecisionScope, DecisionUnit, EncounterOutcome, LearnContext, OmniscientContext,
};
#[cfg(feature = "core")]
pub use world::{
    ArenaWriter, AutomatonPolicyState, BeliefState, BuiltinPolicyState, Credit, EncounterId,
    EncounterRef, EstimateState, ExperimentState, GameWorld, HistoryView, PairView, PeerHistory,
    PlayArena, PolicyTable, PropensityState, RuleKindId, Slot, Stateless, StepRecord, TftState,
    Topology,
};

#[cfg(feature = "core")]
pub(crate) use world::InternalPairHistory;
#[cfg(feature = "core")]
pub(crate) use world::InternalStepHistory;
