use socsim_games::continuous::Cournot;
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, InitialActions,
    NormalFormOutcome, Pairwise, PlayOptions, ProportionalImitation, RandomPairs, RuleBook, Update,
};

fn main() {
    let rules = RuleBook::new();
    let world = build_world(
        Cournot::new(2, 100.0, 1.0, 10.0, 100.0).unwrap(),
        4,
        (),
        InitialActions::Same(vec![20.0]),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: 4,
        }]),
        1,
        1,
        7,
    )
    .unwrap();

    let _ = assemble_with(
        world,
        RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        ProportionalImitation::new(0.001, Update::Synchronous).unwrap(),
        PlayOptions::new(),
        7,
    );
}
