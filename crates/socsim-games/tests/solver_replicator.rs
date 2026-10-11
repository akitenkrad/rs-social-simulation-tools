use socsim_games::catalog::{HawkDove, PrisonersDilemma};
use socsim_games::solve::{
    replicator_fixed_points, replicator_integrate, ReplicatorOptions, SolveError,
};
use socsim_games::{MatrixGame, MixedStrategy};

const ASSERT_TOLERANCE: f64 = 1e-8;

fn symmetric_matrix(payoffs: &[Vec<f64>]) -> MatrixGame {
    let transposed = (0..payoffs.len())
        .map(|column| payoffs.iter().map(|row| row[column]).collect())
        .collect::<Vec<Vec<f64>>>();
    MatrixGame::bimatrix(payoffs, &transposed).unwrap()
}

#[test]
fn hawk_dove_rk4_converges_to_the_d77_interior_fixed_point() {
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let initial = MixedStrategy::new(vec![0.1, 0.9]).unwrap();

    let run = replicator_integrate(&game, &initial, &ReplicatorOptions::new()).unwrap();

    assert!(run.converged);
    assert!(run.t >= 36.03);
    assert!(run.steps > 0);
    assert!((run.state.probs()[0] - 0.5).abs() <= ASSERT_TOLERANCE);
    assert!((run.state.probs()[1] - 0.5).abs() <= ASSERT_TOLERANCE);
}

#[test]
fn fixed_points_include_hawk_dove_vertices_and_interior_point() {
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let fixed = replicator_fixed_points(&game).unwrap();

    assert!(fixed.components.is_empty());
    assert!(fixed.inconclusive.is_empty());
    assert_eq!(fixed.isolated.len(), 3);
    assert_eq!(fixed.isolated[0].support, vec![0]);
    assert_eq!(fixed.isolated[0].point.probs(), &[1.0, 0.0]);
    assert_eq!(fixed.isolated[1].support, vec![1]);
    assert_eq!(fixed.isolated[1].point.probs(), &[0.0, 1.0]);
    assert_eq!(fixed.isolated[2].support, vec![0, 1]);
    assert!((fixed.isolated[2].point.probs()[0] - 0.5).abs() <= ASSERT_TOLERANCE);
    assert!((fixed.isolated[2].point.probs()[1] - 0.5).abs() <= ASSERT_TOLERANCE);
}

#[test]
fn prisoners_dilemma_has_no_interior_replicator_fixed_point() {
    let game = PrisonersDilemma::weak(2.0).unwrap();
    let fixed = replicator_fixed_points(&game).unwrap();

    assert!(fixed.components.is_empty());
    assert!(fixed.inconclusive.is_empty());
    assert!(fixed
        .isolated
        .iter()
        .all(|fixed_point| fixed_point.support.len() == 1));
}

#[test]
fn zero_game_returns_only_the_whole_simplex_component() {
    let game = symmetric_matrix(&[vec![0.0, 0.0], vec![0.0, 0.0]]);
    let fixed = replicator_fixed_points(&game).unwrap();

    assert!(fixed.isolated.is_empty());
    assert!(fixed.inconclusive.is_empty());
    assert_eq!(fixed.components.len(), 1);
    let component = &fixed.components[0];
    assert_eq!(component.support, vec![0, 1]);
    assert!((component.base.probs().iter().sum::<f64>() - 1.0).abs() <= 1e-10);
    assert!(component
        .base
        .probs()
        .iter()
        .all(|probability| *probability >= 0.0));
    assert_eq!(component.directions.len(), 1);
    assert_eq!(component.directions[0].len(), 2);
    assert!((component.directions[0][0] + component.directions[0][1]).abs() <= 1e-10);
}

#[test]
fn replicator_rejects_invalid_options_and_unsupported_games() {
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let initial = MixedStrategy::uniform(2).unwrap();
    for options in [
        ReplicatorOptions::new().with_dt(0.0),
        ReplicatorOptions::new().with_t_max(f64::NAN),
        ReplicatorOptions::new().with_tol(-1.0),
    ] {
        assert!(matches!(
            replicator_integrate(&game, &initial, &options),
            Err(SolveError::InvalidOptions(_))
        ));
    }

    let asymmetric = MatrixGame::bimatrix(
        &[vec![1.0, 0.0], vec![0.0, 1.0]],
        &[vec![0.0, 1.0], vec![1.0, 0.0]],
    )
    .unwrap();
    assert!(matches!(
        replicator_integrate(&asymmetric, &initial, &ReplicatorOptions::new()),
        Err(SolveError::InvalidOptions(_))
    ));
}
