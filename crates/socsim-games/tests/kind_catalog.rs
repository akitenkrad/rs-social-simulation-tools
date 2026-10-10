use std::sync::Arc;

use socsim_games::{
    assemble, build_world, build_world_with, AssignGroup, AssignKind, Assignment, Fixed,
    FixedPlayers, GameError, InitialActions, KindCatalog, MatrixGame, NormalFormOutcome,
    PlayOptions, RuleBook, RuleError, RuleKindId, TitForTat, UniformRandom,
};

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn fixed_assignment(kind: &str, count: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule(kind.to_owned()),
        count,
    }])
}

#[test]
fn with_rules_rejects_a_kind_registered_by_an_earlier_rule_book() {
    let mut first: RuleBook<MatrixGame, ()> = RuleBook::new();
    first.register("same", Arc::new(Fixed::new(0))).unwrap();
    let mut second: RuleBook<MatrixGame, ()> = RuleBook::new();
    second.register("same", Arc::new(Fixed::new(1))).unwrap();

    let result = KindCatalog::new()
        .with_rules(&first)
        .unwrap()
        .with_rules(&second);

    assert!(matches!(
        result,
        Err(RuleError::DuplicateKind(kind)) if kind == "same"
    ));
}

#[test]
fn kind_ids_are_the_sorted_union_of_all_rule_books() {
    let mut first: RuleBook<MatrixGame, ()> = RuleBook::new();
    first.register("zeta", Arc::new(Fixed::new(0))).unwrap();
    first.register("alpha", Arc::new(Fixed::new(0))).unwrap();
    let mut second: RuleBook<MatrixGame, ()> = RuleBook::new();
    second.register("middle", Arc::new(Fixed::new(1))).unwrap();
    second.register("beta", Arc::new(Fixed::new(1))).unwrap();

    let catalog = KindCatalog::new()
        .with_rules(&first)
        .unwrap()
        .with_rules(&second)
        .unwrap();

    assert_eq!(
        catalog.kinds().collect::<Vec<_>>(),
        vec!["alpha", "beta", "middle", "zeta"]
    );
    assert_eq!(catalog.kind_id("alpha"), Some(RuleKindId(0)));
    assert_eq!(catalog.kind_id("beta"), Some(RuleKindId(1)));
    assert_eq!(catalog.kind_id("middle"), Some(RuleKindId(2)));
    assert_eq!(catalog.kind_id("zeta"), Some(RuleKindId(3)));
    assert_eq!(catalog.kind_id("missing"), None);
}

#[test]
fn build_world_matches_build_world_with_for_mixed_builtin_rules() {
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    rules
        .register("tit_for_tat", Arc::new(TitForTat::new(0)))
        .unwrap();
    rules
        .register("uniform_random", Arc::new(UniformRandom))
        .unwrap();
    rules
        .register("fixed_defect", Arc::new(Fixed::new(1)))
        .unwrap();
    let assignment = Assignment::Ordered(vec![
        AssignGroup {
            kind: AssignKind::Rule("tit_for_tat".into()),
            count: 1,
        },
        AssignGroup {
            kind: AssignKind::Rule("uniform_random".into()),
            count: 1,
        },
        AssignGroup {
            kind: AssignKind::Rule("fixed_defect".into()),
            count: 1,
        },
    ]);
    let seed = 73;

    let legacy = build_world(
        prisoners_dilemma(),
        3,
        (),
        InitialActions::Same(0),
        &rules,
        &assignment,
        8,
        12,
        seed,
    )
    .unwrap();
    let catalog = KindCatalog::from_rules(&rules);
    let catalogued = build_world_with(
        prisoners_dilemma(),
        3,
        (),
        InitialActions::Same(0),
        &catalog,
        &assignment,
        8,
        12,
        seed,
    )
    .unwrap();

    assert_eq!(
        serde_json::to_value(legacy).unwrap(),
        serde_json::to_value(catalogued).unwrap()
    );
}

#[test]
fn assemble_rejects_world_kinds_that_differ_from_the_rule_catalog() {
    let mut world_rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    world_rules
        .register("fixed", Arc::new(Fixed::new(0)))
        .unwrap();
    world_rules
        .register("world_only", Arc::new(Fixed::new(1)))
        .unwrap();
    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &world_rules,
        &fixed_assignment("fixed", 2),
        1,
        1,
        5,
    )
    .unwrap();

    let mut assemble_rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    assemble_rules
        .register("fixed", Arc::new(Fixed::new(0)))
        .unwrap();
    let error = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        assemble_rules,
        NormalFormOutcome,
        PlayOptions::new(),
        5,
    )
    .err()
    .expect("different kind lists must be rejected");

    assert!(matches!(
        error,
        GameError::InvalidKinds { expected, actual }
            if expected == vec!["fixed"] && actual == vec!["fixed", "world_only"]
    ));
}

#[test]
fn rule_kinds_are_owned_by_the_rule_driver() {
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    rules.register("uniform", Arc::new(UniformRandom)).unwrap();
    let catalog = KindCatalog::from_rules(&rules);

    assert_eq!(catalog.driver_name("fixed"), Some("game_decide"));
    assert_eq!(catalog.driver_name("uniform"), Some("game_decide"));
    assert_eq!(catalog.driver_name("missing"), None);
}
