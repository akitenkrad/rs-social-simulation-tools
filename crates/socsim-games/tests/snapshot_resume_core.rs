use std::sync::Arc;

use socsim_engine::{Simulation, Snapshot};
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers, GameWorld,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook,
};

type World = GameWorld<MatrixGame, ()>;

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn build_simulation(seed: u64, t_max: u64) -> Simulation<World> {
    let mut rules = RuleBook::new();
    rules
        .register("cooperate", Arc::new(Fixed::new(0)))
        .unwrap();
    rules.register("defect", Arc::new(Fixed::new(1))).unwrap();

    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::PerAgent(vec![0, 1]),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("cooperate".into()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("defect".into()),
                count: 1,
            },
        ]),
        8,
        t_max,
        seed,
    )
    .unwrap();

    assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build()
}

fn run_steps(simulation: &mut Simulation<World>, count: u64) -> Vec<String> {
    (0..count)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn in_memory_snapshot_resume_matches_the_uninterrupted_trajectory() {
    let mut uninterrupted = build_simulation(73, 6);
    let expected = run_steps(&mut uninterrupted, 6);

    let mut partial = build_simulation(73, 6);
    run_steps(&mut partial, 2);
    let snapshot = partial.snapshot();

    let mut resumed = build_simulation(999, 6);
    resumed.restore(snapshot);
    let actual = run_steps(&mut resumed, 4);

    assert_eq!(actual, expected[2..]);
}

#[test]
fn saved_and_loaded_snapshot_resume_matches_the_uninterrupted_trajectory() {
    let mut uninterrupted = build_simulation(73, 6);
    let expected = run_steps(&mut uninterrupted, 6);

    let mut partial = build_simulation(73, 6);
    run_steps(&mut partial, 3);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.json");
    partial.snapshot().save(&path).unwrap();
    let snapshot = Snapshot::<World>::load(&path).unwrap();

    let mut resumed = build_simulation(0, 6);
    resumed.restore(snapshot);
    let actual = run_steps(&mut resumed, 3);

    assert_eq!(actual, expected[3..]);
}

fn build_tft_simulation(seed: u64, t_max: u64) -> Simulation<World> {
    let mut rules = RuleBook::new();
    rules
        .register("tit_for_tat", Arc::new(socsim_games::TitForTat::new(0)))
        .unwrap();
    rules.register("defect", Arc::new(Fixed::new(1))).unwrap();
    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("tit_for_tat".into()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("defect".into()),
                count: 1,
            },
        ]),
        8,
        t_max,
        seed,
    )
    .unwrap();
    assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build()
}

#[test]
fn tft_policy_state_is_restored_from_snapshot() {
    let mut uninterrupted = build_tft_simulation(79, 7);
    let expected = run_steps(&mut uninterrupted, 7);

    let mut partial = build_tft_simulation(79, 7);
    run_steps(&mut partial, 3);
    let snapshot = partial.snapshot();
    let mut resumed = build_tft_simulation(0, 7);
    resumed.restore(snapshot);

    assert_eq!(run_steps(&mut resumed, 4), expected[3..]);
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct CustomSnapshotState {
    reputation: Vec<f64>,
}

impl socsim_games::ExperimentState for CustomSnapshotState {
    type PolicyState = socsim_games::BuiltinPolicyState;
    type View<'a> = &'a [f64];

    fn view_for(&self, _me: socsim_core::AgentId) -> Self::View<'_> {
        &self.reputation
    }
}

type CustomWorld = GameWorld<MatrixGame, CustomSnapshotState>;

fn build_custom_simulation(seed: u64, t_max: u64) -> Simulation<CustomWorld> {
    let mut rules = RuleBook::new();
    rules
        .register("tit_for_tat", Arc::new(socsim_games::TitForTat::new(0)))
        .unwrap();
    rules.register("defect", Arc::new(Fixed::new(1))).unwrap();
    let world = build_world(
        prisoners_dilemma(),
        2,
        CustomSnapshotState {
            reputation: vec![0.25, 0.75],
        },
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("tit_for_tat".into()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("defect".into()),
                count: 1,
            },
        ]),
        8,
        t_max,
        seed,
    )
    .unwrap();
    assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build()
}

fn run_custom_steps(simulation: &mut Simulation<CustomWorld>, count: u64) -> Vec<String> {
    (0..count)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn custom_experiment_state_is_restored_from_saved_snapshot() {
    let mut uninterrupted = build_custom_simulation(83, 7);
    let expected = run_custom_steps(&mut uninterrupted, 7);

    let mut partial = build_custom_simulation(83, 7);
    run_custom_steps(&mut partial, 3);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("custom-snapshot.json");
    partial.snapshot().save(&path).unwrap();
    let snapshot = Snapshot::<CustomWorld>::load(&path).unwrap();

    let mut resumed = build_custom_simulation(0, 7);
    resumed.restore(snapshot);
    assert_eq!(run_custom_steps(&mut resumed, 4), expected[3..]);
}
