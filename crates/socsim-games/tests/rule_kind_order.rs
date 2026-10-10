use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers, GameWorld,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, UniformRandom,
};

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn trajectory(register_random_first: bool) -> Vec<String> {
    let seed = 91;
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    if register_random_first {
        rules.register("z_random", Arc::new(UniformRandom)).unwrap();
        rules.register("a_fixed", Arc::new(Fixed::new(0))).unwrap();
    } else {
        rules.register("a_fixed", Arc::new(Fixed::new(0))).unwrap();
        rules.register("z_random", Arc::new(UniformRandom)).unwrap();
    }

    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup {
                kind: AssignKind::Rule("z_random".into()),
                count: 1,
            },
            AssignGroup {
                kind: AssignKind::Rule("a_fixed".into()),
                count: 1,
            },
        ]),
        8,
        12,
        seed,
    )
    .unwrap();
    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build();

    (0..12)
        .map(|_| {
            simulation.step().unwrap();
            serde_json::to_string(simulation.world()).unwrap()
        })
        .collect()
}

#[test]
fn registration_order_does_not_change_kind_ids_or_trajectory() {
    let left = trajectory(false);
    let right = trajectory(true);
    assert_eq!(left, right);

    let world: GameWorld<MatrixGame, ()> = serde_json::from_str(left.last().unwrap()).unwrap();
    assert_eq!(
        world.policies().kind(socsim_core::AgentId(0)),
        Some("z_random")
    );
    assert_eq!(
        world.policies().kind(socsim_core::AgentId(1)),
        Some("a_fixed")
    );
}
