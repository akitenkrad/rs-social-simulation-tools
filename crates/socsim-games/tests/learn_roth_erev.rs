use rand::rngs::StdRng;
use rand::SeedableRng;
use socsim_games::learn::logit::{EstimateMode, LogitState};
use socsim_games::learn::roth_erev::RothErevState;
use socsim_games::learn::LearnError;
use socsim_games::MatrixGame;

fn positive_pd() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![4.0, 20.0], vec![1.0, 5.0]],
        &[vec![4.0, 1.0], vec![20.0, 5.0]],
    )
    .unwrap()
}

#[test]
fn roth_erev_learns_the_strictly_dominant_action() {
    const T: usize = 100_000;
    let game = positive_pd();
    let mut row = RothErevState::new(&game, 0, 100.0, 0.0, 0.0).unwrap();
    let mut column = RothErevState::new(&game, 1, 100.0, 0.0, 0.0).unwrap();
    let mut rng = StdRng::seed_from_u64(29);

    for _ in 0..T {
        let row_action = row.choose_action(&mut rng).unwrap();
        let column_action = column.choose_action(&mut rng).unwrap();
        let profile = [row_action, column_action];
        row.update_from_profile(&game, 0, &profile).unwrap();
        column.update_from_profile(&game, 1, &profile).unwrap();
    }

    let row_c = row.selection_probabilities().unwrap()[1];
    let column_c = column.selection_probabilities().unwrap()[1];
    eprintln!("p_C row={row_c}, column={column_c}");
    assert!(row_c <= 0.0423);
    assert!(column_c <= 0.0423);
}

#[test]
fn roth_erev_validates_parameters_and_all_game_payoffs() {
    let game = positive_pd();
    for (initial, phi, epsilon) in [
        (0.0, 0.0, 0.0),
        (-1.0, 0.0, 0.0),
        (f64::INFINITY, 0.0, 0.0),
        (1.0, -0.1, 0.0),
        (1.0, 1.0, 0.0),
        (1.0, 0.0, -0.1),
        (1.0, 0.0, 1.0),
    ] {
        assert!(matches!(
            RothErevState::new(&game, 0, initial, phi, epsilon),
            Err(LearnError::InvalidParameter { .. })
        ));
    }

    let zero_payoff = MatrixGame::bimatrix(
        &[vec![1.0, 1.0], vec![1.0, 0.0]],
        &[vec![1.0, 1.0], vec![1.0, 1.0]],
    )
    .unwrap();
    assert!(matches!(
        RothErevState::new(&zero_payoff, 0, 1.0, 0.0, 0.0),
        Err(LearnError::NonPositivePayoff { .. })
    ));
}

#[test]
fn zero_forgetting_adds_payoff_to_the_only_propensity() {
    let game = MatrixGame::bimatrix(&[vec![2.0]], &[vec![3.0]]).unwrap();
    let mut state = RothErevState::new(&game, 0, 10.0, 0.0, 0.0).unwrap();
    state.update(0, 2.0).unwrap();
    assert_eq!(state.propensities(), &[12.0]);
    assert_eq!(state.selection_probabilities().unwrap(), vec![1.0]);
}

#[test]
fn logit_supports_last_and_empirical_estimates_and_validates_lambda() {
    let game = positive_pd();
    for lambda in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            LogitState::new(&game, 0, lambda, EstimateMode::EmpiricalFrequency),
            Err(LearnError::InvalidParameter { .. })
        ));
    }

    let mut empirical = LogitState::new(&game, 0, 1.0, EstimateMode::EmpiricalFrequency).unwrap();
    assert_eq!(
        empirical.selection_probabilities(&game, 0).unwrap(),
        vec![0.5, 0.5]
    );
    empirical.observe(0).unwrap();
    empirical.observe(1).unwrap();
    assert_eq!(
        empirical.expected_payoffs(&game, 0).unwrap(),
        vec![12.0, 3.0]
    );

    let mut last = LogitState::new(&game, 0, 1.0, EstimateMode::LastOpponent).unwrap();
    last.observe(1).unwrap();
    assert_eq!(last.expected_payoffs(&game, 0).unwrap(), vec![20.0, 5.0]);
    let probabilities = last.selection_probabilities(&game, 0).unwrap();
    assert!(probabilities[0] > 0.999_999);
    assert!(probabilities.iter().all(|value| value.is_finite()));
}
