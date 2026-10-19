use std::collections::BTreeMap;
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, ActionError, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers,
    GameError, InitialActions, KindCatalog, MatrixGame, NormalFormOutcome, OnInvalidAction,
    PlayOptions, RuleBook, RuleError, TitForTat,
};

fn prisoners_dilemma() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
}

fn fixed_rules() -> RuleBook<MatrixGame, ()> {
    let mut rules = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    rules
}

fn fixed_assignment(n_agents: usize) -> Assignment {
    Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule("fixed".into()),
        count: n_agents,
    }])
}

#[test]
fn assemble_rejects_games_with_other_than_two_roles() {
    let game = MatrixGame::new(vec![1, 1, 1], vec![vec![0.0], vec![0.0], vec![0.0]]).unwrap();
    let mut rules = RuleBook::new();
    rules.register("fixed", Arc::new(Fixed::new(0))).unwrap();
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &fixed_assignment(2),
        1,
        1,
        1,
    )
    .unwrap();

    let error = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        1,
    )
    .err()
    .expect("three-role games must be rejected");
    assert!(matches!(error, GameError::UnsupportedRoles { n_roles: 3 }));
}

#[test]
fn build_world_rejects_zero_history_length_without_panicking() {
    let rules = fixed_rules();
    let result = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &fixed_assignment(2),
        0,
        1,
        1,
    );
    assert!(matches!(result, Err(GameError::InvalidHistoryLength)));
}

#[test]
fn build_world_rejects_an_unregistered_rule_kind() {
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    for kind in ["grim", "all_d", "fit_for_tat", "all_c", "fictitious_play"] {
        rules.register(kind, Arc::new(Fixed::new(0))).unwrap();
    }
    let assignment = Assignment::Ordered(vec![AssignGroup {
        kind: AssignKind::Rule("tit_for_tat".into()),
        count: 2,
    }]);
    let error = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &assignment,
        1,
        1,
        1,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "unknown rule kind \"tit_for_tat\"; registered kinds: all_c, all_d, fictitious_play, fit_for_tat, grim"
    );
}

#[test]
fn build_world_rejects_assignment_count_mismatch() {
    let rules = fixed_rules();
    let result = build_world(
        prisoners_dilemma(),
        3,
        (),
        InitialActions::Same(0),
        &rules,
        &fixed_assignment(2),
        1,
        1,
        1,
    );
    assert!(matches!(
        result,
        Err(GameError::AgentCount {
            expected: 3,
            actual: 2
        })
    ));
}

#[test]
fn build_world_rejects_per_agent_initial_action_count_mismatch() {
    let rules = fixed_rules();
    let result = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::PerAgent(vec![0]),
        &rules,
        &fixed_assignment(2),
        1,
        1,
        1,
    );
    assert!(matches!(
        result,
        Err(GameError::AgentCount {
            expected: 2,
            actual: 1
        })
    ));
}

#[test]
fn build_world_rejects_an_invalid_initial_action() {
    let rules = fixed_rules();
    let result = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(2),
        &rules,
        &fixed_assignment(2),
        1,
        1,
        1,
    );
    assert!(matches!(
        result,
        Err(GameError::Action(ActionError::InvalidAction {
            role: 0,
            action: 2
        }))
    ));
}

#[test]
fn assemble_rejects_a_player_used_twice_in_one_step() {
    let rules = fixed_rules();
    let world = build_world(
        prisoners_dilemma(),
        3,
        (),
        InitialActions::Same(0),
        &rules,
        &fixed_assignment(3),
        1,
        1,
        1,
    )
    .unwrap();
    let result = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1), (0, 2)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        1,
    );
    assert!(matches!(
        result,
        Err(GameError::InvalidMatching(message)) if message.contains("agent 0")
    ));
}

#[test]
fn assemble_rejects_a_nonexistent_player() {
    let rules = fixed_rules();
    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &fixed_assignment(2),
        1,
        1,
        1,
    )
    .unwrap();
    let result = assemble(
        world,
        FixedPlayers::pairs(&[(0, 2)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        1,
    );
    assert!(matches!(
        result,
        Err(GameError::InvalidMatching(message)) if message.contains("unknown agent 2")
    ));
}

#[test]
fn rule_book_rejects_duplicate_kind_names() {
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    rules.register("same", Arc::new(Fixed::new(0))).unwrap();
    let result = rules.register("same", Arc::new(Fixed::new(1)));
    assert!(matches!(
        result,
        Err(RuleError::DuplicateKind(kind)) if kind == "same"
    ));
}

#[test]
fn rule_book_add_registers_the_rule_name() {
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    rules.add(Arc::new(TitForTat::new(0))).unwrap();

    assert_eq!(
        KindCatalog::from_rules(&rules).kinds().collect::<Vec<_>>(),
        vec!["tit_for_tat"]
    );
}

#[test]
fn rule_book_add_rejects_a_duplicate_rule_name() {
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    rules.add(Arc::new(TitForTat::new(0))).unwrap();
    let result = rules.add(Arc::new(TitForTat::new(1)));

    assert!(matches!(
        result,
        Err(RuleError::DuplicateKind(kind)) if kind == "tit_for_tat"
    ));
}

#[test]
fn rule_book_add_and_register_can_be_combined() {
    let mut rules: RuleBook<MatrixGame, ()> = RuleBook::new();
    rules.add(Arc::new(TitForTat::new(0))).unwrap();
    rules
        .register("fixed_defect", Arc::new(Fixed::new(1)))
        .unwrap();

    assert_eq!(
        KindCatalog::from_rules(&rules).kinds().collect::<Vec<_>>(),
        vec!["fixed_defect", "tit_for_tat"]
    );
}

#[test]
fn assemble_rejects_invalid_fallback_actions() {
    let mut rules = fixed_rules();
    rules.register("other", Arc::new(Fixed::new(1))).unwrap();
    let assignment = Assignment::Ordered(vec![
        AssignGroup {
            kind: AssignKind::Rule("fixed".into()),
            count: 1,
        },
        AssignGroup {
            kind: AssignKind::Rule("other".into()),
            count: 1,
        },
    ]);

    let world = build_world(
        prisoners_dilemma(),
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &assignment,
        1,
        1,
        1,
    )
    .unwrap();
    let invalid = BTreeMap::from([("fixed".to_owned(), 2), ("other".to_owned(), 1)]);
    let result = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(invalid)),
        1,
    );
    assert!(matches!(result, Err(GameError::Action(_))));
}
