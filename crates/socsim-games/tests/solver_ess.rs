use socsim_games::catalog::{HawkDove, RockPaperScissors};
use socsim_games::solve::{ess_verdict, EssVerdict};
use socsim_games::{MatrixGame, MixedStrategy};

fn symmetric_matrix(payoffs: &[Vec<f64>]) -> MatrixGame {
    let transposed = (0..payoffs.len())
        .map(|column| payoffs.iter().map(|row| row[column]).collect())
        .collect::<Vec<Vec<f64>>>();
    MatrixGame::bimatrix(payoffs, &transposed).unwrap()
}

#[test]
fn generalized_rock_paper_scissors_matches_the_d77_eigenvalue_cases() {
    let uniform = MixedStrategy::uniform(3).unwrap();

    assert_eq!(
        ess_verdict(&RockPaperScissors::default(), &uniform).unwrap(),
        EssVerdict::False
    );
    assert_eq!(
        ess_verdict(&RockPaperScissors::new(2.0, 1.0).unwrap(), &uniform).unwrap(),
        EssVerdict::True
    );
    assert_eq!(
        ess_verdict(&RockPaperScissors::new(1.0, 2.0).unwrap(), &uniform).unwrap(),
        EssVerdict::False
    );
}

#[test]
fn non_quasi_strong_one_dimensional_cones_are_decided() {
    let pure_zero = MixedStrategy::pure(2, 0).unwrap();

    assert_eq!(
        ess_verdict(&HawkDove::new(2.0, 2.0).unwrap(), &pure_zero).unwrap(),
        EssVerdict::True
    );
    assert_eq!(
        ess_verdict(
            &symmetric_matrix(&[vec![1.0, 1.0], vec![1.0, 2.0]]),
            &pure_zero,
        )
        .unwrap(),
        EssVerdict::False
    );
}

#[test]
fn non_quasi_strong_two_dimensional_cones_check_the_cross_term() {
    let pure_zero = MixedStrategy::pure(3, 0).unwrap();
    let invading_interior = symmetric_matrix(&[
        vec![0.0, 0.0, 0.0],
        vec![0.0, -1.0, 2.0],
        vec![0.0, 2.0, -1.0],
    ]);
    let negative_cone = symmetric_matrix(&[
        vec![0.0, 0.0, 0.0],
        vec![0.0, -1.0, 0.0],
        vec![0.0, 0.0, -1.0],
    ]);

    assert_eq!(
        ess_verdict(&invading_interior, &pure_zero).unwrap(),
        EssVerdict::False
    );
    assert_eq!(
        ess_verdict(&negative_cone, &pure_zero).unwrap(),
        EssVerdict::True
    );
}

#[test]
fn a_non_nash_candidate_is_not_an_ess() {
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let dove = MixedStrategy::pure(2, 1).unwrap();

    assert_eq!(ess_verdict(&game, &dove).unwrap(), EssVerdict::False);
}
