use std::sync::Arc;

use socsim_engine::Simulation;
use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Automaton, AutomatonKind,
    DecisionScope, EstimateMode, FictitiousPlay, GameWorld, InitialActions, LogitChoice,
    MatrixGame, Mixed, NormalFormOutcome, PlayOptions, RothErev, RoundRobin, RuleBook,
};

type World = GameWorld<MatrixGame>;

fn positive_prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![4.0, 1.0], vec![6.0, 2.0]],
        &[vec![4.0, 6.0], vec![1.0, 2.0]],
    )
    .unwrap()
}

fn build_simulation(seed: u64, t_max: u64) -> Simulation<World> {
    let mut rules = RuleBook::new();
    rules
        .register("fictitious", Arc::new(FictitiousPlay::new()))
        .unwrap();
    rules
        .register("roth_erev", Arc::new(RothErev::new(0.15, 0.2, 1.0)))
        .unwrap();
    rules
        .register(
            "logit",
            Arc::new(LogitChoice::new(0.7).estimate(EstimateMode::EmpiricalFrequency)),
        )
        .unwrap();
    rules
        .register(
            "gtft",
            Arc::new(Automaton::new(AutomatonKind::Gtft { generosity: 0.35 }, 0)),
        )
        .unwrap();
    rules
        .register("mixed", Arc::new(Mixed::new(vec![0.4, 0.6])))
        .unwrap();

    let world = build_world(
        positive_prisoners_dilemma(),
        5,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(
            ["fictitious", "roth_erev", "logit", "gtft", "mixed"]
                .into_iter()
                .map(|kind| AssignGroup {
                    kind: AssignKind::Rule(kind.to_owned()),
                    count: 1,
                })
                .collect(),
        ),
        t_max as usize,
        t_max,
        seed,
    )
    .unwrap();

    assemble(
        world,
        RoundRobin::new(),
        rules,
        NormalFormOutcome,
        PlayOptions::new().decision_scope(DecisionScope::PerEncounter),
        seed,
    )
    .unwrap()
    .build()
}

fn trajectory(seed: u64, t_max: u64) -> Vec<String> {
    let mut simulation = build_simulation(seed, t_max);
    (0..t_max)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn round_robin_learning_tournament_is_deterministic_at_every_step() {
    assert_eq!(trajectory(2027, 16), trajectory(2027, 16));
}
