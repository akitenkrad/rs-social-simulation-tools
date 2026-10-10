use std::borrow::Cow;

use socsim_games::{ActionError, FiniteGame, MatrixError, MatrixGame, NormalFormGame, StageGame};

#[test]
fn new_indexes_two_player_payoffs_in_role_zero_major_order() {
    let game = MatrixGame::new(
        vec![2, 3],
        vec![
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0],
        ],
    )
    .unwrap();

    assert_eq!(game.payoff(0, &[1, 2]), 5.0);
    assert_eq!(game.payoff(1, &[1, 2]), 15.0);

    let mut payoffs = [0.0; 2];
    game.payoffs_into(&[1, 0], &mut payoffs);
    assert_eq!(payoffs, [3.0, 13.0]);
}

#[test]
fn new_indexes_three_player_payoffs_in_role_zero_major_order() {
    let role_0: Vec<f64> = (0..12).map(f64::from).collect();
    let role_1: Vec<f64> = (100..112).map(f64::from).collect();
    let role_2: Vec<f64> = (200..212).map(f64::from).collect();
    let game = MatrixGame::new(vec![2, 2, 3], vec![role_0, role_1, role_2]).unwrap();

    // index = 1 * (2 * 3) + 0 * 3 + 2 = 8
    assert_eq!(game.payoff(0, &[1, 0, 2]), 8.0);
    assert_eq!(game.payoff(1, &[1, 0, 2]), 108.0);
    assert_eq!(game.payoff(2, &[1, 0, 2]), 208.0);
}

#[test]
fn bimatrix_and_zero_sum_construct_expected_payoffs() {
    let a = vec![vec![3.0, 0.0], vec![5.0, 1.0]];
    let b = vec![vec![3.0, 5.0], vec![0.0, 1.0]];
    let bimatrix = MatrixGame::bimatrix(&a, &b).unwrap();

    assert_eq!(bimatrix.n_roles(), 2);
    assert_eq!(bimatrix.n_actions(0), 2);
    assert_eq!(bimatrix.n_actions(1), 2);
    assert_eq!(bimatrix.payoff(0, &[1, 0]), 5.0);
    assert_eq!(bimatrix.payoff(1, &[1, 0]), 0.0);

    let zero_sum = MatrixGame::zero_sum(&a).unwrap();
    for (row, row_payoffs) in a.iter().enumerate() {
        for (col, &payoff) in row_payoffs.iter().enumerate() {
            assert_eq!(zero_sum.payoff(0, &[row, col]), payoff);
            assert_eq!(zero_sum.payoff(1, &[row, col]), -payoff);
        }
    }
}

#[test]
fn labels_override_the_default_action_labels() {
    let game = MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]])
        .unwrap()
        .with_labels(vec![
            vec!["heads".into(), "tails".into()],
            vec!["left".into(), "right".into()],
        ])
        .unwrap();

    assert_eq!(game.action_label(0, 0), Cow::Borrowed("heads"));
    assert_eq!(game.action_label(1, 1), Cow::Borrowed("right"));
}

#[test]
fn default_finite_game_methods_have_the_contract_values() {
    #[derive(Clone)]
    struct Defaults;

    impl StageGame for Defaults {
        type Action = usize;

        fn n_roles(&self) -> usize {
            1
        }

        fn check_action(&self, role: usize, action: &usize) -> Result<(), ActionError> {
            if role != 0 {
                return Err(ActionError::InvalidRole { role });
            }
            if *action >= 2 {
                return Err(ActionError::InvalidAction {
                    role,
                    action: *action,
                });
            }
            Ok(())
        }
    }

    impl NormalFormGame for Defaults {
        fn payoff(&self, role: usize, profile: &[usize]) -> f64 {
            assert_eq!(role, 0);
            profile[0] as f64
        }
    }

    impl FiniteGame for Defaults {
        fn n_actions(&self, role: usize) -> usize {
            assert_eq!(role, 0);
            2
        }
    }

    let game = Defaults;
    let mut payoffs = [f64::NAN];
    game.payoffs_into(&[1], &mut payoffs);

    assert_eq!(payoffs, [1.0]);
    assert_eq!(game.action_label(0, 1), Cow::Borrowed("a1"));
    assert!(!game.is_symmetric());

    fn assert_thread_safe_static<T: Send + Sync + 'static>(_: &T) {}
    assert_thread_safe_static(&game);
}

#[test]
fn check_action_rejects_invalid_role_and_action() {
    let game = MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap();

    assert_eq!(
        game.check_action(2, &0),
        Err(ActionError::InvalidRole { role: 2 })
    );
    assert_eq!(
        game.check_action(0, &2),
        Err(ActionError::InvalidAction { role: 0, action: 2 })
    );
    assert_eq!(game.check_action(1, &1), Ok(()));
}

#[test]
fn new_rejects_empty_zero_and_mismatched_shapes() {
    assert_eq!(MatrixGame::new(vec![], vec![]), Err(MatrixError::Shape));
    assert_eq!(
        MatrixGame::new(vec![2, 0], vec![vec![], vec![]]),
        Err(MatrixError::Shape)
    );
    assert_eq!(
        MatrixGame::new(vec![2, 2], vec![vec![0.0; 4]]),
        Err(MatrixError::Shape)
    );
    assert_eq!(
        MatrixGame::new(vec![2, 2], vec![vec![0.0; 4], vec![0.0; 3]]),
        Err(MatrixError::Shape)
    );
}

#[test]
fn bimatrix_rejects_empty_ragged_and_different_shapes() {
    assert_eq!(MatrixGame::bimatrix(&[], &[]), Err(MatrixError::Shape));
    assert_eq!(
        MatrixGame::bimatrix(&[vec![]], &[vec![]]),
        Err(MatrixError::Shape)
    );
    assert_eq!(
        MatrixGame::bimatrix(&[vec![1.0, 2.0], vec![3.0]], &[vec![1.0, 2.0]]),
        Err(MatrixError::Shape)
    );
    assert_eq!(
        MatrixGame::bimatrix(&[vec![1.0, 2.0], vec![3.0, 4.0]], &[vec![1.0, 2.0]],),
        Err(MatrixError::Shape)
    );
    assert_eq!(
        MatrixGame::bimatrix(&[vec![1.0, 2.0], vec![3.0, 4.0]], &[vec![1.0], vec![2.0]],),
        Err(MatrixError::Shape)
    );
}

#[test]
fn constructors_reject_every_kind_of_non_finite_payoff() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            MatrixGame::new(vec![1], vec![vec![value]]),
            Err(MatrixError::NonFinite)
        );
        assert_eq!(
            MatrixGame::zero_sum(&[vec![value]]),
            Err(MatrixError::NonFinite)
        );
    }
}

#[test]
fn new_rejects_profile_limit_element_limit_and_product_overflow() {
    assert!(matches!(
        MatrixGame::new(vec![1_000_001], vec![]),
        Err(MatrixError::TooLarge { .. })
    ));

    let mut element_limited = vec![1_000_000];
    element_limited.extend([1; 10]);
    assert!(matches!(
        MatrixGame::new(element_limited, vec![]),
        Err(MatrixError::TooLarge { .. })
    ));

    assert!(matches!(
        MatrixGame::new(vec![usize::MAX, 2], vec![]),
        Err(MatrixError::TooLarge { .. })
    ));
}

#[test]
fn with_labels_rejects_outer_and_inner_shape_mismatches() {
    let game = MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]]).unwrap();

    assert_eq!(
        game.clone().with_labels(vec![vec!["h".into(), "t".into()]]),
        Err(MatrixError::Labels)
    );
    assert_eq!(
        game.with_labels(vec![vec!["h".into()], vec!["l".into(), "r".into()],]),
        Err(MatrixError::Labels)
    );
}

#[test]
fn symmetry_requires_equal_action_counts_and_transposed_payoffs() {
    let symmetric = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap();
    assert!(symmetric.is_symmetric());

    let wrong_payoff = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 4.0], vec![0.0, 1.0]],
    )
    .unwrap();
    assert!(!wrong_payoff.is_symmetric());

    let unequal_actions = MatrixGame::bimatrix(
        &[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]],
        &[vec![1.0, 4.0, 0.0], vec![2.0, 5.0, 0.0]],
    )
    .unwrap();
    assert!(!unequal_actions.is_symmetric());

    let three_player = MatrixGame::new(vec![1, 1, 1], vec![vec![0.0]; 3]).unwrap();
    assert!(!three_player.is_symmetric());
}

#[test]
fn matrix_game_round_trips_through_json() {
    let game = MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]])
        .unwrap()
        .with_labels(vec![
            vec!["heads".into(), "tails".into()],
            vec!["heads".into(), "tails".into()],
        ])
        .unwrap();

    let json = serde_json::to_string(&game).unwrap();
    let restored: MatrixGame = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, game);
}
