//! Continuous games resumed from a snapshot that went through a JSON string.
//!
//! A snapshot taken at step 23 is serialized with `serde_json::to_string`,
//! parsed back with `serde_json::from_str`, and restored into a newly assembled
//! simulation. The resumed trajectory (worlds, metric rows, and stop time) must
//! equal the uninterrupted one bit for bit, which requires floating-point
//! values to survive the JSON round trip.

use std::any::Any;
use std::sync::Arc;

use rand::Rng;
use socsim_core::{derive_seed, Recorder, SimRng, WorldState};
use socsim_engine::{Simulation, Snapshot};
use socsim_games::continuous::{Cournot, PublicGoodsContinuous};
use socsim_games::{
    assemble_with_aggregates, build_world, AssignGroup, AssignKind, Assignment,
    ContinuousEquilibrium, ContinuousGame, ContinuousMetrics, ContinuousMetricsOptions,
    DampedBestResponse, DecisionRule, GameWorld, GradientAscent, InitialActions, NormalFormOutcome,
    PlayOptions, RandomGroups, RuleBook, WholePopulation,
};

const STEPS: u64 = 60;
const SPLIT: u64 = 23;

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

fn everyone(kind: &str, n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule(kind.to_owned()),
        count: n_agents,
    }])
}

/// Configuration (1): Cournot, n = 3, a = 100, b = 1.1, c = 10, q_max = 100,
/// damped best response 0.5 from [0, 50, 100].
fn cournot_simulation(seed: u64) -> Simulation<GameWorld<Cournot>> {
    let game = Cournot::new(3, 100.0, 1.1, 10.0, 100.0).unwrap();
    let metrics = ContinuousMetrics::new(
        &game,
        ContinuousMetricsOptions::new()
            .equilibrium(ContinuousEquilibrium::Solve)
            .stop(1.0e-4, 5),
    )
    .unwrap();
    let mut rules = RuleBook::new();
    let rule: Arc<dyn DecisionRule<Cournot, ()>> =
        Arc::new(DampedBestResponse::<Cournot>::new(0.5).unwrap());
    rules.register("damped", rule).unwrap();
    let world = build_world(
        game,
        3,
        (),
        InitialActions::PerAgent(vec![vec![0.0], vec![50.0], vec![100.0]]),
        &rules,
        &everyone("damped", 3),
        4,
        STEPS,
        seed,
    )
    .unwrap();
    assemble_with_aggregates(
        world,
        WholePopulation::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
        4,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(RowRecorder::default()))
    .build()
}

/// Configuration (2): continuous public goods, N = 10, k = 4, e = 10, r = 2.5,
/// random groups of four, gradient ascent 0.3, initial contributions drawn
/// uniformly from [0, e].
fn public_goods_simulation(seed: u64) -> Simulation<GameWorld<PublicGoodsContinuous>> {
    let game = PublicGoodsContinuous::new(4, 10.0, 2.5).unwrap();
    let metrics = ContinuousMetrics::new(
        &game,
        ContinuousMetricsOptions::new()
            .equilibrium(ContinuousEquilibrium::Given(vec![vec![0.0]; 4]))
            .stop(1.0e-9, 3),
    )
    .unwrap();
    let mut rules = RuleBook::new();
    let rule: Arc<dyn DecisionRule<PublicGoodsContinuous, ()>> =
        Arc::new(GradientAscent::new(0.3).unwrap());
    rules.register("gradient", rule).unwrap();
    let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 2]));
    let initial = (0..10).map(|_| vec![rng.gen_range(0.0..=10.0)]).collect();
    let world = build_world(
        game,
        10,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &everyone("gradient", 10),
        4,
        STEPS,
        seed,
    )
    .unwrap();
    assemble_with_aggregates(
        world,
        RandomGroups::new(4).unwrap(),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
        4,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(RowRecorder::default()))
    .build()
}

fn rows<G: ContinuousGame>(simulation: &Simulation<GameWorld<G>>) -> Rows {
    simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<RowRecorder>()
        .unwrap()
        .rows
        .clone()
}

/// Steps until the clock reaches `until` or a stop is requested, returning
/// every step's world as JSON.
fn run_until<G>(simulation: &mut Simulation<GameWorld<G>>, until: u64) -> Vec<String>
where
    G: ContinuousGame + serde::Serialize,
{
    let mut worlds = Vec::new();
    while !simulation.stop_requested() && simulation.world().clock().t() < until {
        simulation.step().unwrap();
        worlds.push(serde_json::to_string(simulation.world()).unwrap());
    }
    worlds
}

fn assert_resume<G>(build: impl Fn(u64) -> Simulation<GameWorld<G>>, expect_stop: bool)
where
    G: ContinuousGame + Clone + serde::Serialize + serde::de::DeserializeOwned,
{
    let seed = 20261011;
    let mut uninterrupted = build(seed);
    let expected = run_until(&mut uninterrupted, STEPS);
    let expected_rows: Vec<_> = rows(&uninterrupted)
        .into_iter()
        .filter(|(t, _, _)| *t > SPLIT)
        .collect();
    let stop_time = uninterrupted.world().clock().t();
    assert_eq!(uninterrupted.stop_requested(), expect_stop);
    assert!(stop_time > SPLIT, "the run must outlast the snapshot step");
    assert!(!expected_rows.is_empty());

    let mut partial = build(seed);
    let before = run_until(&mut partial, SPLIT);
    assert_eq!(before, expected[..SPLIT as usize]);
    assert_eq!(partial.world().clock().t(), SPLIT);

    // The snapshot goes through a JSON string, as `Snapshot::save` and
    // `Snapshot::load` do, before it is restored.
    let text = serde_json::to_string(&partial.snapshot()).unwrap();
    let snapshot: Snapshot<GameWorld<G>> = serde_json::from_str(&text).unwrap();
    assert_eq!(
        serde_json::to_string(&snapshot.world).unwrap(),
        serde_json::to_string(partial.world()).unwrap()
    );

    let mut resumed = build(seed ^ 0x5eed);
    resumed.restore(snapshot);
    let actual = run_until(&mut resumed, STEPS);
    assert_eq!(actual, expected[SPLIT as usize..]);
    assert_eq!(rows(&resumed), expected_rows);
    assert_eq!(resumed.world().clock().t(), stop_time);
    assert_eq!(resumed.stop_requested(), uninterrupted.stop_requested());
}

#[test]
fn cournot_resumes_from_a_json_snapshot_with_the_same_stop_time() {
    assert_resume(cournot_simulation, true);
}

#[test]
fn public_goods_in_random_groups_resumes_from_a_json_snapshot() {
    assert_resume(public_goods_simulation, false);
}

#[test]
fn continuous_world_json_round_trip_preserves_every_value() {
    let mut simulation = public_goods_simulation(7);
    run_until(&mut simulation, SPLIT);
    let world = simulation.world();
    let text = serde_json::to_string(world).unwrap();
    let parsed: GameWorld<PublicGoodsContinuous> = serde_json::from_str(&text).unwrap();
    assert_eq!(serde_json::to_string(&parsed).unwrap(), text);
    let bits = |world: &GameWorld<PublicGoodsContinuous>| -> Vec<u64> {
        world
            .held()
            .iter()
            .flatten()
            .chain(world.payoffs())
            .map(|value| value.to_bits())
            .collect()
    };
    assert_eq!(bits(&parsed), bits(world));
}

/// Configuration (3): configuration (2) with held agents and a Moran process
/// (exponential fitness w = 0.1, mutation 0.2, Gaussian mutation sd = 0.5).
fn moran_public_goods_simulation(seed: u64) -> Simulation<GameWorld<PublicGoodsContinuous>> {
    let game = PublicGoodsContinuous::new(4, 10.0, 2.5).unwrap();
    let metrics = ContinuousMetrics::new(
        &game,
        ContinuousMetricsOptions::new()
            .equilibrium(ContinuousEquilibrium::Given(vec![vec![0.0]; 4]))
            .stop(1.0e-9, 3),
    )
    .unwrap();
    let rules = RuleBook::new();
    let mut rng = SimRng::from_seed(derive_seed(seed, &[0, 2]));
    let initial = (0..10).map(|_| vec![rng.gen_range(0.0..=10.0)]).collect();
    let world = build_world(
        game,
        10,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 10,
        }]),
        4,
        STEPS,
        seed,
    )
    .unwrap();
    let moran = socsim_games::MoranUpdate::new(
        socsim_games::Fitness::Exponential { w: 0.1 },
        0.2,
        socsim_games::Update::Synchronous,
    )
    .unwrap()
    .gaussian_mutation(0.5)
    .unwrap();
    socsim_games::assemble_with(
        world,
        RandomGroups::new(4).unwrap(),
        rules,
        socsim_games::Pairwise::new(NormalFormOutcome).with_aggregates(4),
        moran,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(RowRecorder::default()))
    .build()
}

#[test]
fn public_goods_moran_with_gaussian_mutation_resumes_from_a_json_snapshot() {
    assert_resume(moran_public_goods_simulation, true);
}
