use socsim_games::{
    assemble_with, build_world, counting_play, AssignGroup, AssignKind, Assignment, InitialActions,
    MatrixGame, NoDynamics, PlayOptions, RoundRobin, RuleBook,
};

fn main() {
    let game = MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap();
    let rules = RuleBook::new();
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 2,
        }]),
        1,
        1,
        7,
    )
    .unwrap();

    let _ = assemble_with(
        world,
        RoundRobin::new(),
        rules,
        counting_play(),
        NoDynamics,
        PlayOptions::new(),
        7,
    );
}
