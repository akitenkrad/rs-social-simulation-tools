use std::sync::Arc;

use socsim_core::WorldState;
use socsim_engine::Simulation;
use socsim_games::view::{DecisionScope, DecisionUnit};
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
        InitialActions::Same(0),
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

fn trajectory(seed: u64, t_max: u64) -> Vec<String> {
    let mut simulation = build_simulation(seed, t_max);
    let mut states = Vec::new();
    for _ in 0..t_max {
        simulation.step().unwrap();
        states.push(serde_json::to_string(simulation.world()).unwrap());
    }
    states
}

#[test]
fn fixed_against_fixed_is_deterministic_at_every_step() {
    assert_eq!(trajectory(41, 5), trajectory(41, 5));
}

#[test]
fn fixed_cooperate_against_fixed_defect_has_sucker_and_temptation_payoffs() {
    let mut simulation = build_simulation(41, 1);
    simulation.step().unwrap();

    assert_eq!(simulation.world().payoffs(), &[0.0, 5.0]);
    assert_eq!(simulation.world().n_played(), &[1, 1]);
}

#[test]
fn game_world_satisfies_the_engine_world_bounds() {
    fn assert_world<W: WorldState + Send + 'static>() {}
    assert_world::<World>();

    let _ = DecisionScope::PerStep;
    let _ = DecisionUnit::Step;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct CustomExperiment;

impl socsim_games::ExperimentState for CustomExperiment {
    type PolicyState = socsim_games::BuiltinPolicyState;
    type View<'a> = ();

    fn view_for(&self, _me: socsim_core::AgentId) -> Self::View<'_> {}
}

#[derive(Clone, Copy)]
enum OpponentRule {
    Fixed,
    UniformRandom,
}

fn build_stateful_simulation(seed: u64, t_max: u64, opponent: OpponentRule) -> Simulation<World> {
    let mut rules = RuleBook::new();
    rules
        .register("tit_for_tat", Arc::new(socsim_games::TitForTat::new(0)))
        .unwrap();
    let opponent_kind = match opponent {
        OpponentRule::Fixed => {
            rules
                .register("fixed_defect", Arc::new(Fixed::new(1)))
                .unwrap();
            "fixed_defect"
        }
        OpponentRule::UniformRandom => {
            rules
                .register("uniform_random", Arc::new(socsim_games::UniformRandom))
                .unwrap();
            "uniform_random"
        }
    };
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
                kind: AssignKind::Rule(opponent_kind.into()),
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

fn stateful_trajectory(seed: u64, t_max: u64, opponent: OpponentRule) -> Vec<String> {
    let mut simulation = build_stateful_simulation(seed, t_max, opponent);
    (0..t_max)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn tit_for_tat_against_uniform_random_is_deterministic_at_every_step() {
    assert_eq!(
        stateful_trajectory(101, 32, OpponentRule::UniformRandom),
        stateful_trajectory(101, 32, OpponentRule::UniformRandom)
    );
}

#[test]
fn tit_for_tat_against_fixed_is_deterministic_at_every_step() {
    assert_eq!(
        stateful_trajectory(101, 8, OpponentRule::Fixed),
        stateful_trajectory(101, 8, OpponentRule::Fixed)
    );
}

#[test]
fn uniform_random_trajectory_changes_with_the_seed() {
    assert_ne!(
        stateful_trajectory(101, 64, OpponentRule::UniformRandom),
        stateful_trajectory(102, 64, OpponentRule::UniformRandom)
    );
}

#[test]
fn custom_experiment_world_satisfies_the_engine_world_bounds() {
    fn assert_world<W: WorldState + Send + 'static>() {}
    assert_world::<GameWorld<MatrixGame, CustomExperiment>>();
}
