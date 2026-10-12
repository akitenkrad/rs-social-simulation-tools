pub mod game;
pub mod learn;
pub mod mixed;
pub mod solve;

pub use game::catalog;
pub use game::continuous;
pub use mixed::MixedStrategy;

#[cfg(feature = "llm")]
pub mod llm;

pub use game::{
    check_continuous_action, metric_action_ids, ActionError, AggregateGame, AggregateReduce,
    AggregateSpec, AggregateSpecError, AnonymousGame, CommonActionSpace, ContinuousGame,
    ContinuousSpec, DescribeGame, FiniteGame, FiniteSpec, MatrixError, MatrixGame, MetricAction,
    MetricActionIds, MetricScheme, NormalFormGame, NumericBr, StageGame,
};
pub use learn::{automaton::AutomatonKind, logit::EstimateMode, TieBreak};
pub use solve::{
    best_response_iteration, best_response_step, ess_verdict, golden_section_best_response,
    pure_nash_counts, replicator_fixed_points, replicator_integrate, AffineComponent,
    BestResponseOracle, BrIteration, BrIterationOptions, BrResult, BrStatus, CountProfile,
    EssVerdict, FixedPoint, FixedPointSet, GoldenSectionOptions, ReplicatorOptions, ReplicatorRun,
};

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
pub use assemble::{
    assemble, assemble_with, assemble_with_aggregates, counting_play, Counting, NoDynamics,
    Pairwise, PlayPath, PopulationDynamics, WithAggregates,
};
#[cfg(feature = "core")]
pub use assemble::{
    build_world, build_world_with, uniform_initial_actions, AssignGroup, AssignKind, Assignment,
    InitialActions, OnInvalidAction, PayoffAggregation, PlayOptions,
};
#[cfg(feature = "core")]
pub use error::GameError;
#[cfg(feature = "grid")]
pub use matching::GridNeighborhood;
#[cfg(feature = "net")]
pub use matching::NetworkEdges;
#[cfg(feature = "core")]
pub use matching::{
    EncounterBounds, FixedPlayers, ImplicitPattern, MatchContext, Matching, MatchingState,
    RandomGroups, RandomPairs, RoundRobin, WholePopulation,
};
#[cfg(feature = "core")]
pub use mechanism::{
    ContinuousEquilibrium, ContinuousMetrics, ContinuousMetricsOptions, EquilibriumTarget,
    FermiUpdate, Fitness, GameMetrics, ImitateBest, ImitationTie, MetricsOptions, MixedSource,
    MoranUpdate, ProportionalImitation, RuleDriver, Update,
};
#[cfg(feature = "core")]
pub use outcome::{NormalFormOutcome, OutcomeContext, OutcomeRule};
#[cfg(feature = "core")]
pub use rule::{
    Automaton, DampedBestResponse, DecisionRule, FictitiousPlay, Fixed, GradientAscent,
    KindCatalog, LogitChoice, Mixed, MyopicBestResponse, RothErev, RuleBook, RuleError,
    RuleInitContext, TitForTat, UniformRandom,
};
#[cfg(feature = "core")]
pub use view::{
    AggregateView, DecisionContext, DecisionScope, DecisionUnit, EncounterOutcome, LearnContext,
    OmniscientContext,
};
#[cfg(feature = "net")]
pub use world::NetworkTopology;
#[cfg(feature = "core")]
pub use world::{
    ArenaWriter, AutomatonPolicyState, BeliefState, BuiltinPolicyState, ContinuousState, Credit,
    EncounterId, EncounterRef, EstimateState, ExperimentState, GameWorld, HistoryView, PairView,
    PeerHistory, PlayArena, PolicyTable, PropensityState, RuleKindId, Slot, Stateless, StepRecord,
    TftState, Topology,
};
#[cfg(feature = "grid")]
pub use world::{GridBoundary, GridNeighbors, GridSpec, GridTopology};

#[cfg(feature = "core")]
pub(crate) use world::InternalAggregateTable;
#[cfg(feature = "core")]
pub(crate) use world::InternalPairHistory;
#[cfg(feature = "core")]
pub(crate) use world::InternalPlayedActionCounts;
#[cfg(feature = "core")]
pub(crate) use world::InternalPolicyReports;
#[cfg(feature = "core")]
pub(crate) use world::InternalStepHistory;
