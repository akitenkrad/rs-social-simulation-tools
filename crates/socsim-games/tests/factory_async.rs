//! `GameSimFactory` selects the asynchronous assembly path and payoff timing.

use std::any::Any;
use std::collections::BTreeMap;

use socsim_core::{derive_seed, Recorder, WorldState};
use socsim_engine::{RandomActivationScheduler, Simulation, SimulationBuilder};
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{
    assemble_async, build_world, AssignGroup, AssignKind, Assignment, AsyncPopulationStep,
    GameError, GameMetrics, GameSimFactory, GameWorld, GridBoundary, GridNeighborhood,
    GridNeighbors, GridSpec, ImitateBest, InitialActions, MetricsOptions, NoDynamics,
    NormalFormOutcome, Pairwise, PayoffTiming, PlayOptions, RandomPairs, RuleBook, SchedulerChoice,
    StageGame, Topology, Update,
};
use socsim_runner::Trial;

const SIDE: usize = 50;
const N_AGENTS: usize = SIDE * SIDE;
const SEED: u64 = 92;
const STEPS: u64 = 20;

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

#[derive(Debug, PartialEq)]
struct Trajectory {
    worlds: Vec<String>,
    rows: Rows,
}

fn recorded<G: StageGame>(builder: SimulationBuilder<GameWorld<G>>) -> Simulation<GameWorld<G>> {
    builder.recorder(Box::new(RowRecorder::default())).build()
}

fn run<G>(mut simulation: Simulation<GameWorld<G>>, steps: u64) -> Trajectory
where
    G: StageGame + serde::Serialize,
    G::Action: serde::Serialize,
{
    let mut worlds = Vec::new();
    while simulation.world().clock().t() < steps {
        simulation.step().unwrap();
        worlds.push(serde_json::to_string(simulation.world()).unwrap());
    }
    let rows = simulation
        .recorder()
        .as_any()
        .and_then(|recorder| recorder.downcast_ref::<RowRecorder>())
        .unwrap()
        .rows
        .clone();
    Trajectory { worlds, rows }
}

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

fn topology() -> Topology {
    Topology::grid(GridSpec {
        rows: SIDE,
        cols: SIDE,
        boundary: GridBoundary::Toroidal,
        neighborhood: GridNeighbors::Moore,
    })
    .unwrap()
}

fn initial_actions() -> InitialActions<usize> {
    InitialActions::Shares(vec![(0, 0.9), (1, 0.1)])
}

fn held_assignment() -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Held,
        count: N_AGENTS,
    }])
}

fn async_step() -> AsyncPopulationStep<ImitateBest> {
    AsyncPopulationStep::new(ImitateBest::new(Update::Asynchronous))
}

fn metrics_options() -> MetricsOptions {
    MetricsOptions::new().population_share(true)
}

fn world(seed: u64) -> GameWorld<PrisonersDilemma> {
    let game = PrisonersDilemma::weak(1.85).unwrap();
    build_world(
        game,
        N_AGENTS,
        (),
        initial_actions(),
        &RuleBook::new(),
        &held_assignment(),
        2,
        STEPS,
        seed,
    )
    .unwrap()
    .with_topology(topology())
    .unwrap()
}

type BaseFactory =
    GameSimFactory<PrisonersDilemma, (), GridNeighborhood, Pairwise<NormalFormOutcome>, NoDynamics>;

fn base_factory() -> BaseFactory {
    GameSimFactory::new(PrisonersDilemma::weak(1.85).unwrap())
        .agents(N_AGENTS)
        .topology(topology())
        .matching(GridNeighborhood::new().include_self(true))
        .initial_held(initial_actions())
        .history_len(2)
        .t_max(STEPS)
}

fn async_factory() -> GameSimFactory<
    PrisonersDilemma,
    (),
    GridNeighborhood,
    Pairwise<NormalFormOutcome>,
    AsyncPopulationStep<ImitateBest>,
> {
    base_factory()
        .async_dynamics(async_step())
        .metrics(metrics_options())
        .scheduler(SchedulerChoice::RandomActivation)
}

fn manual_async(seed: u64) -> Simulation<GameWorld<PrisonersDilemma>> {
    let world = world(seed);
    let metrics = GameMetrics::new(
        world.game(),
        metrics_options().payoff_timing(PayoffTiming::AfterUpdate),
    )
    .unwrap();
    recorded(
        assemble_async(
            world,
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            async_step(),
            PlayOptions::new(),
            seed,
        )
        .unwrap()
        .scheduler(Box::new(RandomActivationScheduler))
        .add_mechanism(Box::new(metrics)),
    )
}

fn mechanism_names(trial: &Trial<GameWorld<PrisonersDilemma>>) -> Vec<&str> {
    trial
        .mechanisms
        .iter()
        .map(|mechanism| mechanism.name())
        .collect()
}

fn descriptions(options: MetricsOptions) -> BTreeMap<String, String> {
    GameMetrics::new(&PrisonersDilemma::weak(1.85).unwrap(), options)
        .unwrap()
        .describe()
        .into_iter()
        .collect()
}

fn error_message<T>(result: Result<T, GameError>) -> String {
    match result {
        Ok(_) => panic!("expected construction to fail"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn async_factory_matches_manual_assembly_for_worlds_and_metric_rows() {
    let reference = run(manual_async(SEED), STEPS);
    let actual = run(recorded(async_factory().into_builder(SEED).unwrap()), STEPS);
    assert_eq!(actual, reference);
}

#[test]
fn factory_state_selects_the_matching_payoff_description() {
    const AFTER_UPDATE: &str =
        "the actions held after every update of the step (asynchronous update)";

    let async_trial = async_factory().build_trial(SEED).unwrap();
    assert_eq!(
        mechanism_names(&async_trial),
        ["async_population_step", "game_metrics"]
    );
    let after = descriptions(metrics_options().payoff_timing(PayoffTiming::AfterUpdate));
    assert!(after["game.mean_payoff"].contains(AFTER_UPDATE));

    let synchronous = base_factory()
        .dynamics(ImitateBest::new(Update::Synchronous))
        .metrics(metrics_options());
    let synchronous_trial = synchronous.build_trial(SEED).unwrap();
    assert_eq!(
        mechanism_names(&synchronous_trial),
        [
            "game_match",
            "game_decide",
            "imitate_best",
            "game_play",
            "game_metrics"
        ]
    );
    let before = descriptions(metrics_options().payoff_timing(PayoffTiming::BeforeUpdate));
    assert!(!before["game.mean_payoff"].contains(AFTER_UPDATE));
}

#[test]
fn the_later_dynamics_setting_replaces_the_earlier_one() {
    let asynchronous = base_factory()
        .dynamics(ImitateBest::new(Update::Synchronous))
        .async_dynamics(async_step())
        .build_trial(SEED)
        .unwrap();
    assert_eq!(mechanism_names(&asynchronous), ["async_population_step"]);

    let synchronous = base_factory()
        .async_dynamics(async_step())
        .dynamics(ImitateBest::new(Update::Synchronous))
        .build_trial(SEED)
        .unwrap();
    assert_eq!(
        mechanism_names(&synchronous),
        ["game_match", "game_decide", "imitate_best", "game_play"]
    );
}

#[test]
fn factory_preserves_async_assembly_population_errors() {
    let manual = error_message(assemble_async(
        world(SEED),
        RandomPairs::new(),
        Pairwise::new(NormalFormOutcome),
        async_step(),
        PlayOptions::new(),
        SEED,
    ));
    let from_factory = error_message(
        GameSimFactory::new(PrisonersDilemma::weak(1.85).unwrap())
            .agents(N_AGENTS)
            .topology(topology())
            .matching(RandomPairs::new())
            .initial_held(initial_actions())
            .async_dynamics(async_step())
            .history_len(2)
            .t_max(STEPS)
            .build(SEED),
    );
    assert_eq!(from_factory, manual);
    assert!(manual.contains("requires a matching with an implicit pattern"));
}

#[test]
fn build_trial_uses_the_async_parts_and_engine_seed() {
    let trial = async_factory().build_trial(SEED).unwrap();
    assert_eq!(trial.engine_seed, derive_seed(SEED, &[1]));
    assert_eq!(
        mechanism_names(&trial),
        ["async_population_step", "game_metrics"]
    );
    assert_eq!(
        run(trial_simulation(trial), STEPS),
        run(manual_async(SEED), STEPS)
    );
}
