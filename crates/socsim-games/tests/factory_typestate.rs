//! `GameSimFactory` builds the same simulation as the assembly functions.
//!
//! Every configuration is built twice: once with `build_world` and
//! `assemble_with` (plus the metrics added with `add_mechanism`), and once
//! with `GameSimFactory`. The world after every step (as JSON) and every
//! metric row must coincide, through `build`, `into_builder`, and
//! `build_trial` alike.

use std::any::Any;
use std::sync::Arc;

use socsim_core::{derive_seed, Recorder, WorldState};
use socsim_engine::{RandomActivationScheduler, Simulation, SimulationBuilder};
use socsim_games::catalog::{HawkDove, MinorityGame, PrisonersDilemma};
use socsim_games::continuous::Cournot;
use socsim_games::{
    assemble_with, build_world, counting_play, AssignGroup, AssignKind, Assignment,
    BuiltinPolicyState, ContinuousEquilibrium, ContinuousMetrics, ContinuousMetricsOptions,
    DampedBestResponse, DecisionRule, DecisionScope, EncounterRef, FermiUpdate, FictitiousPlay,
    Fixed, FixedPlayers, GameError, GameMetrics, GameSimFactory, GameWorld, GridBoundary,
    GridNeighborhood, GridNeighbors, GridSpec, ImitateBest, ImitationTie, InitialActions,
    MatrixGame, MetricsOptions, NoDynamics, NormalFormOutcome, OutcomeContext, OutcomeRule,
    Pairwise, PayoffAggregation, PlayOptions, RandomPairs, RothErev, RoundRobin, RuleBook,
    SchedulerChoice, StageGame, TitForTat, Topology, UniformRandom, Update, WholePopulation,
};
use socsim_runner::Trial;

type Rows = Vec<(u64, String, u64)>;

#[derive(Default)]
struct RowRecorder {
    rows: Rows,
}

impl Recorder for RowRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.rows.push((t, key.to_owned(), value.to_bits()));
    }

    fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        self.rows.extend(
            row.iter()
                .map(|(column, value)| (t, format!("{table}.{column}"), value.to_bits())),
        );
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

/// The world after every step and every recorded row.
#[derive(Debug, PartialEq)]
struct Trajectory {
    worlds: Vec<String>,
    rows: Rows,
}

/// Steps until `steps` steps have run or a mechanism requests a stop.
fn run<G>(mut simulation: Simulation<GameWorld<G>>, steps: u64) -> Trajectory
where
    G: StageGame + serde::Serialize,
    G::Action: serde::Serialize,
{
    let mut worlds = Vec::new();
    while !simulation.stop_requested() && simulation.world().clock().t() < steps {
        simulation.step().unwrap();
        worlds.push(serde_json::to_string(simulation.world()).unwrap());
    }
    let rows = simulation
        .recorder()
        .as_any()
        .and_then(|recorder| recorder.downcast_ref::<RowRecorder>())
        .map(|recorder| recorder.rows.clone())
        .unwrap_or_default();
    Trajectory { worlds, rows }
}

fn recorded<G: StageGame>(builder: SimulationBuilder<GameWorld<G>>) -> Simulation<GameWorld<G>> {
    builder.recorder(Box::new(RowRecorder::default())).build()
}

/// Runs a trial the way a trial runner does: its parts, its scheduler, and
/// its engine seed.
fn trial_simulation<G: StageGame>(trial: Trial<GameWorld<G>>) -> Simulation<GameWorld<G>> {
    let builder = trial
        .mechanisms
        .into_iter()
        .fold(SimulationBuilder::new(trial.world), |builder, mechanism| {
            builder.add_mechanism(mechanism)
        })
        .scheduler(trial.scheduler)
        .seed(trial.engine_seed);
    recorded(builder)
}

/// Checks the three entry points of `factory` against the reference
/// trajectory, and that building twice gives the same result.
fn assert_matches_reference<G, M, P, D>(
    factory: &GameSimFactory<G, (), M, P, D>,
    reference: Trajectory,
    seed: u64,
    steps: u64,
) where
    G: StageGame + Clone + serde::Serialize,
    G::Action: serde::Serialize,
    M: socsim_games::Matching<G, ()> + Clone + 'static,
    P: socsim_games::PlayPath<G, ()> + Clone,
    D: socsim_games::PopulationDynamics<G, ()> + Clone,
{
    assert!(!reference.worlds.is_empty());
    let from_builder = run(recorded(factory.into_builder(seed).unwrap()), steps);
    assert_eq!(from_builder, reference);
    let again = run(recorded(factory.into_builder(seed).unwrap()), steps);
    assert_eq!(again, reference);

    let trial = factory.build_trial(seed).unwrap();
    assert_eq!(trial.engine_seed, derive_seed(seed, &[1]));
    assert_eq!(run(trial_simulation(trial), steps), reference);

    // `build` has no recorder; its worlds still coincide.
    let built = run(factory.build(seed).unwrap(), steps);
    assert_eq!(built.worlds, reference.worlds);
    assert!(built.rows.is_empty());
}

// ── (i) a two-agent prisoner's dilemma with tit for tat ────────────────────

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

const PD_SEED: u64 = 101;
const PD_STEPS: u64 = 32;
const PD_HISTORY: usize = 8;

fn pd_assignment(opponent: &str) -> Assignment {
    Assignment::Ordered(vec![
        AssignGroup {
            kind: AssignKind::Rule("tit_for_tat".into()),
            count: 1,
        },
        AssignGroup {
            kind: AssignKind::Rule(opponent.into()),
            count: 1,
        },
    ])
}

fn pd_reference(opponent: &str, rule: Arc<dyn DecisionRule<MatrixGame, ()>>) -> Trajectory {
    let mut rules = RuleBook::new();
    rules
        .register("tit_for_tat", Arc::new(TitForTat::new(0)))
        .unwrap();
    rules.register(opponent, rule).unwrap();
    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &pd_assignment(opponent),
        PD_HISTORY,
        PD_STEPS,
        PD_SEED,
    )
    .unwrap();
    let builder = assemble_with(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        Pairwise::new(NormalFormOutcome),
        NoDynamics,
        PlayOptions::new(),
        PD_SEED,
    )
    .unwrap();
    run(recorded(builder), PD_STEPS)
}

fn pd_factory<R: DecisionRule<MatrixGame, ()> + 'static>(
    opponent: &str,
    rule: R,
) -> GameSimFactory<MatrixGame, (), FixedPlayers> {
    GameSimFactory::new(prisoners_dilemma())
        .agents(2)
        .matching(FixedPlayers::pairs(&[(0, 1)]))
        .rule("tit_for_tat", TitForTat::new(0))
        .rule(opponent, rule)
        .assign(pd_assignment(opponent))
        .history_len(PD_HISTORY)
        .t_max(PD_STEPS)
}

#[test]
fn tit_for_tat_against_uniform_random_matches_the_assembly_functions() {
    let reference = pd_reference("uniform_random", Arc::new(UniformRandom));
    let factory = pd_factory("uniform_random", UniformRandom);
    assert_matches_reference(&factory, reference, PD_SEED, PD_STEPS);
}

#[test]
fn tit_for_tat_against_fixed_matches_the_assembly_functions() {
    let reference = pd_reference("fixed_defect", Arc::new(Fixed::new(1)));
    let factory = pd_factory("fixed_defect", Fixed::new(1));
    assert_matches_reference(&factory, reference, PD_SEED, PD_STEPS);
}

#[test]
fn the_seed_changes_the_trajectory_and_the_initial_world_depends_only_on_the_seed() {
    let factory = pd_factory("uniform_random", UniformRandom);
    let initial = |seed: u64| serde_json::to_string(factory.build(seed).unwrap().world()).unwrap();
    assert_eq!(initial(PD_SEED), initial(PD_SEED));
    assert_ne!(initial(PD_SEED), initial(PD_SEED + 1));
    assert_eq!(
        initial(PD_SEED),
        serde_json::to_string(&factory.build_trial(PD_SEED).unwrap().world).unwrap()
    );
    let first = run(factory.build(PD_SEED).unwrap(), 64);
    let second = run(factory.build(PD_SEED + 1).unwrap(), 64);
    assert_ne!(first.worlds, second.worlds);
    assert_eq!(first, run(factory.build(PD_SEED).unwrap(), 64));
}

#[test]
fn one_factory_builds_simulations_from_several_threads() {
    fn assert_send_sync<T: Send + Sync>(_: &T) {}
    let factory = pd_factory("uniform_random", UniformRandom);
    assert_send_sync(&factory);
    let sequential: Vec<_> = (0..4)
        .map(|seed| run(factory.build(seed).unwrap(), 16).worlds)
        .collect();
    let parallel: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|seed| {
                let factory = &factory;
                scope.spawn(move || run(factory.build(seed).unwrap(), 16).worlds)
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    assert_eq!(sequential, parallel);
}

// ── (ii) a held population on a grid imitating the best neighbor ──────────

const GRID_SIDE: usize = 10;
const GRID_SEED: u64 = 1;
const GRID_STEPS: u64 = 20;

fn grid_topology() -> Topology {
    Topology::grid(GridSpec {
        rows: GRID_SIDE,
        cols: GRID_SIDE,
        boundary: GridBoundary::Toroidal,
        neighborhood: GridNeighbors::Moore,
    })
    .unwrap()
}

fn grid_shares() -> InitialActions<usize> {
    InitialActions::Shares(vec![(0, 0.9), (1, 0.1)])
}

fn grid_play_options() -> PlayOptions<usize> {
    PlayOptions::new().payoff_aggregation(PayoffAggregation::Sum)
}

fn grid_metrics() -> MetricsOptions {
    MetricsOptions::new().population_share(true)
}

#[test]
fn imitate_best_on_a_grid_with_initial_shares_matches_the_assembly_functions() {
    let n = GRID_SIDE * GRID_SIDE;
    let game = PrisonersDilemma::weak(1.85).unwrap();
    let metrics = GameMetrics::new(&game, grid_metrics()).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game.clone(),
        n,
        (),
        grid_shares(),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n,
        }]),
        1,
        GRID_STEPS,
        GRID_SEED,
    )
    .unwrap()
    .with_topology(grid_topology())
    .unwrap();
    let held = world.held().to_vec();
    assert_eq!(held.iter().filter(|&&action| action == 1).count(), 10);
    let builder = assemble_with(
        world,
        GridNeighborhood::new().include_self(true),
        rules,
        Pairwise::new(NormalFormOutcome),
        ImitateBest::new(Update::Synchronous).tie_break(ImitationTie::KeepOwn),
        grid_play_options(),
        GRID_SEED,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics));
    let reference = run(recorded(builder), GRID_STEPS);
    assert!(reference
        .rows
        .iter()
        .any(|(_, column, _)| column == "game.population_share.a0"));

    // No rule is registered, so every agent is held by default.
    let factory = GameSimFactory::new(game)
        .agents(n)
        .topology(grid_topology())
        .matching(GridNeighborhood::new().include_self(true))
        .initial_held(grid_shares())
        .dynamics(ImitateBest::new(Update::Synchronous).tie_break(ImitationTie::KeepOwn))
        .play_options(grid_play_options())
        .metrics(grid_metrics())
        .history_len(1)
        .t_max(GRID_STEPS);
    assert_eq!(
        factory.build(GRID_SEED).unwrap().world().held(),
        held.as_slice()
    );
    assert_matches_reference(&factory, reference, GRID_SEED, GRID_STEPS);
}

// ── (iii) a Cournot oligopoly with damped best responses ──────────────────

const COURNOT_SEED: u64 = 41;
const COURNOT_STEPS: u64 = 60;

fn cournot_options() -> ContinuousMetricsOptions {
    ContinuousMetricsOptions::new()
        .equilibrium(ContinuousEquilibrium::Solve)
        .stop(1.0e-4, 5)
}

fn cournot_initial() -> InitialActions<Vec<f64>> {
    InitialActions::PerAgent(vec![vec![0.0], vec![50.0], vec![100.0]])
}

#[test]
fn damped_best_response_cournot_matches_the_assembly_functions_until_the_stop() {
    let game = Cournot::new(3, 100.0, 1.1, 10.0, 100.0).unwrap();
    let metrics = ContinuousMetrics::new(&game, cournot_options()).unwrap();
    let mut rules = RuleBook::new();
    let rule: Arc<dyn DecisionRule<Cournot, ()>> =
        Arc::new(DampedBestResponse::<Cournot>::new(0.5).unwrap());
    rules.register("damped", rule).unwrap();
    let world = build_world(
        game.clone(),
        3,
        (),
        cournot_initial(),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Rule("damped".into()),
            count: 3,
        }]),
        4,
        COURNOT_STEPS,
        COURNOT_SEED,
    )
    .unwrap();
    let builder = assemble_with(
        world,
        WholePopulation::new(),
        rules,
        Pairwise::new(NormalFormOutcome).with_aggregates(4),
        NoDynamics,
        PlayOptions::new(),
        COURNOT_SEED,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics));
    let mut reference_simulation = recorded(builder);
    let mut worlds = Vec::new();
    while !reference_simulation.stop_requested()
        && reference_simulation.world().clock().t() < COURNOT_STEPS
    {
        reference_simulation.step().unwrap();
        worlds.push(serde_json::to_string(reference_simulation.world()).unwrap());
    }
    // The early stop is part of what is compared: it must happen before the
    // last step.
    assert!(reference_simulation.stop_requested());
    assert!(reference_simulation.world().clock().t() < COURNOT_STEPS);
    let reference = Trajectory {
        worlds,
        rows: reference_simulation
            .recorder()
            .as_any()
            .and_then(|recorder| recorder.downcast_ref::<RowRecorder>())
            .unwrap()
            .rows
            .clone(),
    };
    assert!(reference
        .rows
        .iter()
        .any(|(_, column, _)| column.starts_with("game.aggregate.")));

    // One rule is registered, so every agent follows it by default.
    let factory = GameSimFactory::new(game)
        .agents(3)
        .matching(WholePopulation::new())
        .rule("damped", DampedBestResponse::<Cournot>::new(0.5).unwrap())
        .initial_held(cournot_initial())
        .aggregates(4)
        .continuous_metrics(cournot_options())
        .history_len(4)
        .t_max(COURNOT_STEPS);
    assert_matches_reference(&factory, reference, COURNOT_SEED, COURNOT_STEPS);

    // The stop is requested in the same step through every entry point.
    let mut simulation = factory.build(COURNOT_SEED).unwrap();
    while !simulation.stop_requested() && simulation.world().clock().t() < COURNOT_STEPS {
        simulation.step().unwrap();
    }
    assert_eq!(
        simulation.stop_requested(),
        reference_simulation.stop_requested()
    );
    assert_eq!(
        simulation.world().clock().t(),
        reference_simulation.world().clock().t()
    );
}

// ── the other play paths and settings ──────────────────────────────────────

#[test]
fn counting_play_with_aggregates_matches_the_assembly_functions() {
    const SEED: u64 = 13;
    const STEPS: u64 = 12;
    let game = MinorityGame::new(5).unwrap();
    let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("uniform_random", Arc::new(UniformRandom))
        .unwrap();
    let assign = Assignment::Shuffled(vec![
        AssignGroup {
            kind: AssignKind::Rule("uniform_random".into()),
            count: 3,
        },
        AssignGroup {
            kind: AssignKind::Held,
            count: 2,
        },
    ]);
    let world = build_world(
        game.clone(),
        5,
        (),
        InitialActions::Same(1),
        &rules,
        &assign,
        3,
        STEPS,
        SEED,
    )
    .unwrap();
    let builder = assemble_with(
        world,
        WholePopulation::new(),
        rules,
        counting_play().with_aggregates(3),
        NoDynamics,
        PlayOptions::new(),
        SEED,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics));
    let reference = run(recorded(builder), STEPS);

    let factory = GameSimFactory::new(game)
        .agents(5)
        .matching(WholePopulation::new())
        .rule("uniform_random", UniformRandom)
        .assign(assign)
        .initial_held(InitialActions::Same(1))
        .counting_play()
        .aggregates(3)
        .metrics(MetricsOptions::new())
        .history_len(3)
        .t_max(STEPS);
    assert_matches_reference(&factory, reference, SEED, STEPS);
}

#[test]
fn outcome_dynamics_play_options_and_random_activation_match_the_assembly_functions() {
    let reference = scaled_hawk_dove_reference();
    let factory = scaled_hawk_dove_factory().outcome(ScaledOutcome);
    assert_matches_reference(&factory, reference, HAWK_DOVE_SEED, HAWK_DOVE_STEPS);
}

#[test]
fn the_outcome_rule_reaches_the_play_step() {
    // The default outcome rule gives a different trajectory, so the match
    // above depends on the outcome set through the factory.
    let default_outcome = run(
        scaled_hawk_dove_factory().build(HAWK_DOVE_SEED).unwrap(),
        HAWK_DOVE_STEPS,
    );
    assert_ne!(default_outcome.worlds, scaled_hawk_dove_reference().worlds);
}

/// An outcome rule that transforms the game's payoffs: `2 * payoff + 1`.
#[derive(Clone, Copy, Debug)]
struct ScaledOutcome;

impl OutcomeRule<HawkDove, ()> for ScaledOutcome {
    fn name(&self) -> &str {
        "scaled"
    }

    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, HawkDove, ()>,
        _encounter: EncounterRef<'_>,
        actions: &[usize],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        use socsim_games::NormalFormGame;
        ctx.game().payoffs_into(actions, payoffs);
        for payoff in payoffs.iter_mut() {
            *payoff = 2.0 * *payoff + 1.0;
        }
        Ok(())
    }
}

const HAWK_DOVE_SEED: u64 = 211;
const HAWK_DOVE_STEPS: u64 = 15;

fn hawk_dove_initial() -> InitialActions<usize> {
    InitialActions::Shares(vec![(1, 0.5), (0, 0.5)])
}

fn hawk_dove_options() -> PlayOptions<usize> {
    PlayOptions::new().payoff_aggregation(PayoffAggregation::Mean)
}

/// The factory without an outcome rule set (the default outcome).
fn scaled_hawk_dove_factory(
) -> GameSimFactory<HawkDove, (), RandomPairs, Pairwise<NormalFormOutcome>, FermiUpdate> {
    GameSimFactory::with_state(HawkDove::new(2.0, 4.0).unwrap(), ())
        .agents(10)
        .matching(RandomPairs::new())
        .dynamics(FermiUpdate::new(1.0, Update::Synchronous).unwrap())
        .initial_held(hawk_dove_initial())
        .play_options(hawk_dove_options())
        .scheduler(SchedulerChoice::RandomActivation)
        .history_len(2)
        .t_max(HAWK_DOVE_STEPS)
}

fn scaled_hawk_dove_reference() -> Trajectory {
    const SEED: u64 = HAWK_DOVE_SEED;
    const STEPS: u64 = HAWK_DOVE_STEPS;
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let initial = hawk_dove_initial;
    let options = hawk_dove_options;
    let rules = RuleBook::new();
    let world = build_world(
        game,
        10,
        (),
        initial(),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 10,
        }]),
        2,
        STEPS,
        SEED,
    )
    .unwrap();
    let builder = assemble_with(
        world,
        RandomPairs::new(),
        rules,
        Pairwise::new(ScaledOutcome),
        FermiUpdate::new(1.0, Update::Synchronous).unwrap(),
        options(),
        SEED,
    )
    .unwrap()
    .scheduler(Box::new(RandomActivationScheduler));
    run(recorded(builder), STEPS)
}

// ── (iv) fictitious play against Roth-Erev in a round robin ────────────────

// Values from `tests/snapshot_resume_learning.rs`: the game (lines 52-58),
// the two learning rules (lines 62-74), the matching and the decision scope
// (lines 121-127), the history length equal to `t_max` (line 116), and the
// seed and the number of steps (lines 144 and 147).
const LEARNING_SEED: u64 = 211;
const LEARNING_STEPS: u64 = 12;
const LEARNING_AGENTS: usize = 4;

fn positive_prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![4.0, 1.0], vec![6.0, 2.0]],
        &[vec![4.0, 6.0], vec![1.0, 2.0]],
    )
    .unwrap()
}

fn fictitious_play() -> FictitiousPlay {
    FictitiousPlay::new()
        .initial_counts(vec![0.5, 0.5])
        .tie_break_random()
}

fn roth_erev() -> RothErev {
    RothErev::new(0.1, 0.25, 1.0)
}

fn learning_assignment() -> Assignment {
    Assignment::Ordered(vec![
        AssignGroup {
            kind: AssignKind::Rule("fictitious".into()),
            count: 2,
        },
        AssignGroup {
            kind: AssignKind::Rule("roth_erev".into()),
            count: 2,
        },
    ])
}

fn per_encounter() -> PlayOptions<usize> {
    PlayOptions::new().decision_scope(DecisionScope::PerEncounter)
}

fn learning_factory() -> GameSimFactory<MatrixGame, (), RoundRobin> {
    GameSimFactory::new(positive_prisoners_dilemma())
        .agents(LEARNING_AGENTS)
        .matching(RoundRobin::new())
        .rule("fictitious", fictitious_play())
        .rule("roth_erev", roth_erev())
        .assign(learning_assignment())
        .metrics(MetricsOptions::new())
        .history_len(LEARNING_STEPS as usize)
        .t_max(LEARNING_STEPS)
}

#[test]
fn fictitious_play_against_roth_erev_per_encounter_matches_the_assembly_functions() {
    let game = positive_prisoners_dilemma();
    let metrics = GameMetrics::new(&game, MetricsOptions::new()).unwrap();
    let mut rules = RuleBook::new();
    rules
        .register("fictitious", Arc::new(fictitious_play()))
        .unwrap();
    rules.register("roth_erev", Arc::new(roth_erev())).unwrap();
    let world = build_world(
        game,
        LEARNING_AGENTS,
        (),
        InitialActions::Same(0),
        &rules,
        &learning_assignment(),
        LEARNING_STEPS as usize,
        LEARNING_STEPS,
        LEARNING_SEED,
    )
    .unwrap();
    let builder = assemble_with(
        world,
        RoundRobin::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        NoDynamics,
        per_encounter(),
        LEARNING_SEED,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics));
    let reference = run(recorded(builder), LEARNING_STEPS);

    let factory = learning_factory().play_options(per_encounter());
    // Both learning states are in the world the factory builds.
    let initial = factory.build(LEARNING_SEED).unwrap();
    let policies = initial.world().policies();
    assert!(matches!(
        policies.state(socsim_core::AgentId(0)),
        Some(BuiltinPolicyState::Belief(_))
    ));
    assert!(matches!(
        policies.state(socsim_core::AgentId(3)),
        Some(BuiltinPolicyState::Propensity(_))
    ));
    assert_matches_reference(&factory, reference, LEARNING_SEED, LEARNING_STEPS);
}

#[test]
fn the_decision_scope_reaches_the_matching_check() {
    // With the default per-step scope, a round robin of four agents gives
    // agents several roles in one step, which the matching rejects; the
    // per-encounter scope set through `play_options` is accepted.
    assert!(matches!(
        learning_factory().build(LEARNING_SEED),
        Err(GameError::InvalidMatching(_))
    ));
    assert!(learning_factory()
        .play_options(per_encounter())
        .build(LEARNING_SEED)
        .is_ok());
}
