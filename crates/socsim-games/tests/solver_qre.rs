use socsim_games::catalog::{
    BattleOfSexes, MatchingPennies, PrisonersDilemma, RockPaperScissors, StagHunt,
};
use socsim_games::mixed::{action_payoffs, regrets, MixedError};
use socsim_games::solve::{logit_qre, tolerance, QreOptions, QreSolution, SolveError};
use socsim_games::{FiniteGame, MatrixGame, MixedStrategy};

const FIXED_POINT_TOLERANCE: f64 = 1e-10;

fn three_player_game() -> MatrixGame {
    MatrixGame::new(
        vec![2, 2, 2],
        vec![
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0],
            vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
        ],
    )
    .unwrap()
}

fn softmax(payoffs: &[f64], lambda: f64) -> Vec<f64> {
    let maximum = payoffs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut weights = payoffs
        .iter()
        .map(|payoff| (lambda * (payoff - maximum)).exp())
        .collect::<Vec<_>>();
    let total: f64 = weights.iter().sum();
    weights.iter_mut().for_each(|weight| *weight /= total);
    weights
}

fn recomputed_residual<G: FiniteGame>(game: &G, strategies: &[MixedStrategy], lambda: f64) -> f64 {
    (0..game.n_roles())
        .map(|role| {
            let response = softmax(&action_payoffs(game, role, strategies).unwrap(), lambda);
            strategies[role]
                .probs()
                .iter()
                .zip(response)
                .map(|(&probability, response)| (probability - response).abs())
                .fold(0.0, f64::max)
        })
        .fold(0.0, f64::max)
}

fn assert_uniform(solution: &QreSolution, counts: &[usize]) {
    assert_eq!(solution.strategies.len(), counts.len());
    for (strategy, &count) in solution.strategies.iter().zip(counts) {
        assert_eq!(strategy.len(), count);
        for &probability in strategy.probs() {
            assert!((probability - 1.0 / count as f64).abs() <= 1e-12);
        }
    }
}

fn assert_verified<G: FiniteGame>(game: &G, solution: &QreSolution) {
    assert!(solution.residual <= FIXED_POINT_TOLERANCE);
    let residual = recomputed_residual(game, &solution.strategies, solution.lambda);
    assert!(
        residual <= FIXED_POINT_TOLERANCE,
        "lambda={}, reported={}, recomputed={residual}",
        solution.lambda,
        solution.residual
    );
    assert_eq!(solution.residual, residual);

    let verified_regrets = regrets(game, &solution.strategies).unwrap();
    assert_eq!(solution.regrets, verified_regrets);
    let game_tolerance = tolerance(game).unwrap();
    for (role, &regret) in solution.regrets.iter().enumerate() {
        let bound = (game.n_actions(role) as f64).ln() / solution.lambda;
        assert!(
            regret <= bound + game_tolerance,
            "role={role}, lambda={}, regret={regret}, bound={bound}",
            solution.lambda
        );
    }
}

fn verify_lambda_sequence<G: FiniteGame>(name: &str, game: &G) {
    let mut previous = None;
    for lambda in [0.5, 1.0, 2.0, 5.0, 10.0] {
        let mut options = QreOptions::new(lambda).with_damping(0.1);
        if let Some(strategies) = previous.take() {
            options = options.with_init(strategies);
        }
        let solution = logit_qre(game, &options)
            .unwrap_or_else(|error| panic!("{name} did not converge for lambda={lambda}: {error}"));
        println!(
            "{name}: lambda={lambda}, iterations={}, residual={:e}",
            solution.iterations, solution.residual
        );
        assert_verified(game, &solution);
        previous = Some(solution.strategies.clone());
    }
}

#[test]
fn options_have_the_decided_defaults_and_builders() {
    let init = vec![MixedStrategy::uniform(2).unwrap()];
    let options = QreOptions::new(3.0)
        .with_init(init.clone())
        .with_damping(0.25)
        .with_tol(1e-9)
        .with_max_iter(42);

    assert_eq!(options.lambda, 3.0);
    assert_eq!(options.init, Some(init));
    assert_eq!(options.damping, 0.25);
    assert_eq!(options.tol, 1e-9);
    assert_eq!(options.max_iter, 42);

    let defaults = QreOptions::new(1.0);
    assert_eq!(defaults.init, None);
    assert_eq!(defaults.damping, 0.1);
    assert_eq!(defaults.tol, 1e-12);
    assert_eq!(defaults.max_iter, 100_000);
}

#[test]
fn lambda_zero_returns_uniform_for_two_and_three_player_games() {
    let battle = BattleOfSexes::default();
    let battle_solution = logit_qre(&battle, &QreOptions::new(0.0)).unwrap();
    assert_uniform(&battle_solution, &[2, 2]);
    assert_eq!(battle_solution.residual, 0.0);

    let rps = RockPaperScissors::default();
    let rps_solution = logit_qre(&rps, &QreOptions::new(0.0)).unwrap();
    assert_uniform(&rps_solution, &[3, 3]);
    assert_eq!(rps_solution.residual, 0.0);

    let three_player = three_player_game();
    let non_uniform = vec![MixedStrategy::pure(2, 0).unwrap(); 3];
    let solution = logit_qre(&three_player, &QreOptions::new(0.0).with_init(non_uniform)).unwrap();
    assert_uniform(&solution, &[2, 2, 2]);
    assert_eq!(solution.residual, 0.0);

    let extreme = MatrixGame::new(vec![2], vec![vec![-f64::MAX, f64::MAX]]).unwrap();
    let extreme_solution = logit_qre(&extreme, &QreOptions::new(0.0)).unwrap();
    assert_uniform(&extreme_solution, &[2]);
    assert_eq!(extreme_solution.residual, 0.0);
}

#[test]
fn representative_games_satisfy_the_fixed_point_and_regret_bound() {
    verify_lambda_sequence("battle-of-sexes", &BattleOfSexes::default());
    verify_lambda_sequence("matching-pennies", &MatchingPennies::default());
    verify_lambda_sequence(
        "prisoners-dilemma",
        &PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap(),
    );
    verify_lambda_sequence("stag-hunt", &StagHunt::default());
    verify_lambda_sequence("rock-paper-scissors", &RockPaperScissors::default());
    verify_lambda_sequence("three-player", &three_player_game());
}

fn verify_default_from_uniform<G: FiniteGame>(name: &str, game: &G) {
    for lambda in [0.5, 1.0, 2.0, 5.0, 10.0] {
        let solution = logit_qre(game, &QreOptions::new(lambda)).unwrap_or_else(|error| {
            panic!("default options did not converge for {name}, lambda={lambda}: {error}")
        });
        println!(
            "default {name}: lambda={lambda}, iterations={}, residual={:e}",
            solution.iterations, solution.residual
        );
        assert_verified(game, &solution);
    }
}

#[test]
fn default_options_converge_from_uniform_for_representative_games() {
    verify_default_from_uniform("battle-of-sexes", &BattleOfSexes::default());
    verify_default_from_uniform("stag-hunt", &StagHunt::default());
    verify_default_from_uniform(
        "prisoners-dilemma",
        &PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap(),
    );
    verify_default_from_uniform("three-player", &three_player_game());
}

#[test]
fn a_previous_solution_can_initialize_the_next_lambda() {
    let game = BattleOfSexes::default();
    let first = logit_qre(&game, &QreOptions::new(1.0)).unwrap();
    let second = logit_qre(
        &game,
        &QreOptions::new(2.0).with_init(first.strategies.clone()),
    )
    .unwrap();

    assert_verified(&game, &second);
    assert_eq!(second.lambda, 2.0);
}

#[test]
fn symmetric_zero_sum_games_stay_uniform_at_positive_lambda() {
    let pennies = logit_qre(&MatchingPennies::default(), &QreOptions::new(37.0)).unwrap();
    assert_uniform(&pennies, &[2, 2]);
    assert!(pennies.residual <= FIXED_POINT_TOLERANCE);

    let rps = logit_qre(&RockPaperScissors::default(), &QreOptions::new(37.0)).unwrap();
    assert_uniform(&rps, &[3, 3]);
    assert!(rps.residual <= FIXED_POINT_TOLERANCE);
}

#[test]
fn iteration_limit_returns_not_converged_instead_of_a_solution() {
    let game = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
    assert!(matches!(
        logit_qre(&game, &QreOptions::new(10.0).with_max_iter(1)),
        Err(SolveError::NotConverged {
            iterations: 1,
            residual
        }) if residual.is_finite() && residual > 1e-12
    ));
}

#[test]
fn invalid_options_and_initial_dimensions_are_rejected() {
    let game = BattleOfSexes::default();
    for options in [
        QreOptions::new(-1.0),
        QreOptions::new(f64::NAN),
        QreOptions::new(f64::INFINITY),
        QreOptions::new(1.0).with_damping(0.0),
        QreOptions::new(1.0).with_damping(1.000_001),
        QreOptions::new(1.0).with_damping(f64::NAN),
        QreOptions::new(1.0).with_tol(0.0),
        QreOptions::new(1.0).with_tol(-1.0),
        QreOptions::new(1.0).with_tol(f64::INFINITY),
        QreOptions::new(1.0).with_max_iter(0),
    ] {
        assert!(matches!(
            logit_qre(&game, &options),
            Err(SolveError::InvalidOptions(_))
        ));
    }

    assert_eq!(
        logit_qre(
            &game,
            &QreOptions::new(1.0).with_init(vec![MixedStrategy::uniform(2).unwrap()]),
        ),
        Err(SolveError::Mixed(MixedError::RoleCount {
            expected: 2,
            actual: 1,
        }))
    );
    assert_eq!(
        logit_qre(
            &game,
            &QreOptions::new(1.0).with_init(vec![
                MixedStrategy::uniform(3).unwrap(),
                MixedStrategy::uniform(2).unwrap(),
            ]),
        ),
        Err(SolveError::Mixed(MixedError::Dimension {
            role: 0,
            expected: 2,
            actual: 3,
        }))
    );
}

#[test]
fn repeated_calls_are_exactly_deterministic() {
    let game = StagHunt::default();
    let options = QreOptions::new(5.0).with_damping(0.25);
    let first = logit_qre(&game, &options);
    let second = logit_qre(&game, &options);
    assert_eq!(first, second);
}

#[test]
fn large_lambda_does_not_overflow_or_produce_nan() {
    let game = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
    match logit_qre(&game, &QreOptions::new(100.0).with_max_iter(10_000)) {
        Ok(solution) => {
            println!(
                "large-lambda: lambda=100, iterations={}, residual={:e}",
                solution.iterations, solution.residual
            );
            assert!(solution.residual.is_finite());
            assert!(solution.regrets.iter().all(|value| value.is_finite()));
            assert!(solution
                .strategies
                .iter()
                .flat_map(|strategy| strategy.probs())
                .all(|probability| probability.is_finite()));
        }
        Err(SolveError::NotConverged {
            residual,
            iterations,
        }) => {
            println!(
                "large-lambda: lambda=100, iterations={iterations}, residual={residual:e}, not-converged"
            );
            assert!(residual.is_finite());
        }
        Err(error) => panic!("unexpected error: {error}"),
    }
}
