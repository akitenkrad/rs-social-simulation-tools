use std::any::Any;
use std::sync::Arc;

use socsim_core::{Recorder, WorldState};
use socsim_engine::Simulation;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers, GameMetrics,
    GameWorld, InitialActions, MatrixGame, MetricsOptions, Mixed, MixedSource, NormalFormOutcome,
    PlayOptions, RuleBook, UniformRandom,
};

type World = GameWorld<MatrixGame>;

#[derive(Default)]
struct MetricRecorder {
    values: Vec<(u64, String, f64)>,
}

impl Recorder for MetricRecorder {
    fn record_metric(&mut self, t: u64, key: &str, value: f64) {
        self.values.push((t, key.to_owned(), value));
    }

    fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        self.values.extend(
            row.iter()
                .map(|(column, value)| (t, format!("{table}.{column}"), *value)),
        );
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

fn matching_pennies() -> MatrixGame {
    MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap()
}

fn recorded(simulation: &Simulation<World>) -> Vec<(u64, String, f64)> {
    simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<MetricRecorder>()
        .unwrap()
        .values
        .clone()
}

fn fixed_simulation(seed: u64, t_max: u64, options: MetricsOptions) -> Simulation<World> {
    let game = matching_pennies();
    let metrics = GameMetrics::new(&game, options).unwrap();
    let mut rules = RuleBook::new();
    rules.register("zero", Arc::new(Fixed::new(0))).unwrap();
    rules.register("one", Arc::new(Fixed::new(1))).unwrap();
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("zero".into()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("one".into()),
                count: 1,
            },
        ]),
        4,
        t_max,
        seed,
    )
    .unwrap();
    assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new().report_mixed(true),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(MetricRecorder::default()))
    .build()
}

#[test]
fn snapshot_resume_preserves_metrics_state_reports_stop_time_and_values() {
    let options = || {
        MetricsOptions::new()
            .mixed_source(MixedSource::PlayedCumulative)
            .stop(1.0e-12, 2)
    };

    let mut uninterrupted = fixed_simulation(107, 20, options());
    uninterrupted.run().unwrap();
    let expected_metrics = recorded(&uninterrupted);
    let expected_world = serde_json::to_value(uninterrupted.world()).unwrap();
    assert!(expected_world.get("metrics_state").is_some());
    assert!(expected_world.get("reports").is_some());

    let mut partial = fixed_simulation(107, 20, options());
    partial.step().unwrap();
    let mut actual_metrics = recorded(&partial);
    let snapshot = partial.snapshot();

    let mut resumed = fixed_simulation(999, 20, options());
    resumed.restore(snapshot);
    resumed.run().unwrap();
    actual_metrics.extend(recorded(&resumed));

    assert_eq!(
        resumed.world().clock().t(),
        uninterrupted.world().clock().t()
    );
    assert_eq!(actual_metrics, expected_metrics);
    assert_eq!(
        serde_json::to_value(resumed.world()).unwrap(),
        expected_world
    );
}

#[test]
fn stop_window_counts_stable_transitions_after_the_initial_observation() {
    let mut simulation = fixed_simulation(109, 20, MetricsOptions::new().stop(0.01, 3));

    simulation.run().unwrap();

    assert!(simulation.stop_requested());
    assert_eq!(simulation.world().clock().t(), 4);
    assert_eq!(recorded(&simulation).last().unwrap().0, 4);
}

#[test]
fn default_metrics_leave_world_json_without_metrics_state() {
    let mut simulation = fixed_simulation(113, 1, MetricsOptions::new());

    simulation.step().unwrap();

    assert!(serde_json::to_value(simulation.world())
        .unwrap()
        .get("metrics_state")
        .is_none());
}

fn random_simulation(seed: u64, instrumented: bool) -> Simulation<World> {
    let game = matching_pennies();
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new().mixed_source(MixedSource::PlayedCumulative),
    )
    .unwrap();
    let mut rules = RuleBook::new();
    rules.register("uniform", Arc::new(UniformRandom)).unwrap();
    rules
        .register("mixed", Arc::new(Mixed::new(vec![0.25, 0.75])))
        .unwrap();
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("uniform".into()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("mixed".into()),
                count: 1,
            },
        ]),
        4,
        4,
        seed,
    )
    .unwrap();
    let builder = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new().report_mixed(instrumented),
        seed,
    )
    .unwrap();
    if instrumented {
        builder.add_mechanism(Box::new(metrics)).build()
    } else {
        builder.build()
    }
}

fn without_observation_state(world: &World) -> serde_json::Value {
    let mut value = serde_json::to_value(world).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("metrics_state");
    object.remove("reports");
    value
}

#[test]
fn metrics_and_reports_do_not_change_policy_held_or_play_trajectory() {
    let mut baseline = random_simulation(127, false);
    let mut instrumented = random_simulation(127, true);

    for _ in 0..4 {
        baseline.step().unwrap();
        instrumented.step().unwrap();
        assert_eq!(
            without_observation_state(baseline.world()),
            without_observation_state(instrumented.world())
        );
    }
}
