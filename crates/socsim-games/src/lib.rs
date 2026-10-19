//! Game-theoretic models, simulation components, and solvers for socsim.
//!
//! A [`StageGame`] defines roles and valid actions; [`NormalFormGame`] adds
//! payoffs, while [`FiniteGame`] and [`ContinuousGame`] describe discrete and
//! box-constrained action spaces. In engine-backed simulations,
//! [`DecisionRule`] implementations choose actions, [`Matching`] creates
//! encounters, and an [`OutcomeRule`] computes their payoffs. [`GameWorld`]
//! stores the game, policy state, histories, and topology; [`build_world`] and
//! [`assemble()`] connect those pieces to the socsim engine.
//!
//! The crate also provides synchronous and asynchronous population dynamics,
//! per-step game metrics, finite- and continuous-game solvers, built-in classic
//! games, and learning-rule cores. Optional APIs add the type-checked
//! [`GameSimFactory`], scenario-driven [`GameTrialFactory`], LLM decision rules,
//! and versioned JSON exports.
//!
//! # Cargo features
//!
//! - The default build provides game traits and implementations, learning-rule
//!   cores, mixed strategies, finite-game solvers, and continuous-game solvers.
//! - `core` adds worlds, rules, matching, outcomes, views, metrics
//!   configuration, and [`build_world`].
//! - `engine` implies `core` and adds [`assemble()`], play mechanisms, and
//!   population-dynamics mechanisms.
//! - `metrics` adds action entropy to [`GameMetrics`].
//! - `net` and `grid` add network and lattice topologies and matchings.
//! - `llm` adds LLM-backed decisions, response storage, and replay manifests;
//!   `live` additionally enables the live backend factory.
//! - `factory` adds [`GameSimFactory`]; `config` adds scenario parsing and
//!   [`GameTrialFactory`].
//! - `export` adds validated game and solution artifacts.
//!
//! # Example
//!
//! The example is compiled with every feature combination. Its simulation body
//! runs when the `engine` feature is enabled and remains a valid empty program
//! for the default, solver-only build.
//!
//! ```
//! # #[cfg(feature = "engine")]
//! # {
//! use std::sync::Arc;
//! use socsim_games::{
//!     assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed,
//!     FixedPlayers, InitialActions, MatrixGame, NormalFormOutcome, PlayOptions,
//!     RuleBook, TitForTat,
//! };
//!
//! let game = MatrixGame::bimatrix(
//!     &[vec![3.0, 0.0], vec![5.0, 1.0]],
//!     &[vec![3.0, 5.0], vec![0.0, 1.0]],
//! )?;
//! let mut rules = RuleBook::new();
//! rules.register("tit_for_tat", Arc::new(TitForTat::new(0)))?;
//! rules.register("defect", Arc::new(Fixed::new(1)))?;
//! let world = build_world(
//!     game,
//!     2,
//!     (),
//!     InitialActions::Same(0),
//!     &rules,
//!     &Assignment::Ordered(vec![
//!         AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 1 },
//!         AssignGroup { kind: AssignKind::Rule("defect".into()), count: 1 },
//!     ]),
//!     2,
//!     2,
//!     1,
//! )?;
//! let mut simulation = assemble(
//!     world,
//!     FixedPlayers::pairs(&[(0, 1)]),
//!     rules,
//!     NormalFormOutcome,
//!     PlayOptions::new(),
//!     1,
//! )?.build();
//! simulation.step()?;
//! assert_eq!(simulation.world().payoffs(), &[0.0, 5.0]);
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! See the [game guide](https://github.com/akitenkrad/rs-social-simulation-tools/blob/main/docs/games.md)
//! for the execution model, built-in components, and complete examples.

#![warn(missing_docs)]

/// Stage-game traits, built-in games, matrix games, and game specifications.
pub mod game;
pub mod learn;
/// Probability distributions over finite actions.
pub mod mixed;
/// Equilibrium, dominance, evolutionary, and continuous-game solvers.
pub mod solve;

#[cfg(feature = "export")]
pub mod export;

pub use game::catalog;
pub use game::continuous;
pub use mixed::MixedStrategy;

#[cfg(feature = "llm")]
/// LLM-backed decisions, deterministic replay storage, and prompt components.
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

#[cfg(feature = "export")]
pub use export::{
    write_artifacts, write_game_spec, write_solutions, EssLabel, ExportError, GameSpec, SolutionSet,
};

#[cfg(feature = "core")]
/// World construction and simulation assembly helpers.
pub mod assemble;
#[cfg(feature = "core")]
/// Errors raised while validating or executing a game simulation.
pub mod error;
#[cfg(feature = "core")]
/// Encounter-generation strategies for populations of agents.
pub mod matching;
#[cfg(feature = "core")]
/// Engine mechanisms for decisions, play, learning, dynamics, and metrics.
pub mod mechanism;
#[cfg(feature = "core")]
/// Rules that turn an encounter's action profile into payoffs.
pub mod outcome;
#[cfg(feature = "core")]
/// Immediate, learning, and continuous-action decision rules.
pub mod rule;
#[cfg(feature = "core")]
/// Read-only contexts exposed to decision and learning rules.
pub mod view;
#[cfg(feature = "core")]
/// Persistent game-world state, histories, policy state, and topologies.
pub mod world;

#[cfg(feature = "config")]
pub mod config;
#[cfg(feature = "factory")]
pub mod factory;

#[cfg(feature = "engine")]
pub use assemble::{
    assemble, assemble_async, assemble_with, assemble_with_aggregates, counting_play, Counting,
    NoDynamics, Pairwise, PlayPath, PopulationDynamics, WithAggregates,
};
#[cfg(feature = "core")]
pub use assemble::{
    build_world, build_world_with, uniform_initial_actions, AssignGroup, AssignKind, Assignment,
    InitialActions, OnInvalidAction, PayoffAggregation, PlayOptions,
};
#[cfg(feature = "config")]
pub use config::{
    game_family, GameFamily, RuleCtor, RuleRegistry, ScenarioConfig, ScenarioGame,
    SCENARIO_MECHANISM_NAMES,
};
#[cfg(feature = "core")]
pub use error::GameError;
#[cfg(feature = "config")]
pub use factory::GameTrialFactory;
#[cfg(feature = "factory")]
pub use factory::{GameSimFactory, NoMatching, SchedulerChoice};
#[cfg(feature = "grid")]
pub use matching::GridNeighborhood;
#[cfg(feature = "net")]
pub use matching::NetworkEdges;
#[cfg(feature = "core")]
pub use matching::{
    EncounterBounds, FixedPlayers, ImplicitPattern, MatchContext, Matching, MatchingState,
    RandomGroups, RandomPairs, RoundRobin, WholePopulation,
};
#[cfg(feature = "engine")]
pub use mechanism::{AsyncActivation, AsyncPopulationStep};
#[cfg(feature = "core")]
pub use mechanism::{
    ContinuousEquilibrium, ContinuousMetrics, ContinuousMetricsOptions, EquilibriumTarget,
    FermiUpdate, Fitness, GameMetrics, ImitateBest, ImitationTie, MetricsOptions, MixedSource,
    MoranUpdate, PayoffTiming, ProportionalImitation, RuleDriver, Update,
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
