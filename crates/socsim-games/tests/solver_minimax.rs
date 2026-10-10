use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use socsim_games::catalog::{BattleOfSexes, MatchingPennies, RockPaperScissors};
use socsim_games::mixed::expected_payoff;
use socsim_games::solve::{
    maximin_security_level, support_enumeration, zero_sum_minimax, SolveError,
};
use socsim_games::MatrixGame;

const ASSERT_TOLERANCE: f64 = 1e-9;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= ASSERT_TOLERANCE,
        "expected {expected}, got {actual}"
    );
}

fn assert_probs(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (&actual, &expected) in actual.iter().zip(expected) {
        assert_close(actual, expected);
    }
}

#[test]
fn matching_pennies_and_rock_paper_scissors_have_uniform_minimax_solutions() {
    let pennies = zero_sum_minimax(&MatchingPennies::default()).unwrap();
    assert_close(pennies.value, 0.0);
    assert_probs(pennies.row.probs(), &[0.5, 0.5]);
    assert_probs(pennies.col.probs(), &[0.5, 0.5]);
    assert!(pennies.duality_gap <= ASSERT_TOLERANCE);
    assert!(pennies.regret <= ASSERT_TOLERANCE);

    let rps = zero_sum_minimax(&RockPaperScissors::default()).unwrap();
    assert_close(rps.value, 0.0);
    assert_probs(rps.row.probs(), &[1.0 / 3.0; 3]);
    assert_probs(rps.col.probs(), &[1.0 / 3.0; 3]);
    assert!(rps.duality_gap <= ASSERT_TOLERANCE);
    assert!(rps.regret <= ASSERT_TOLERANCE);
}

#[test]
fn seeded_five_by_five_zero_sum_values_match_support_enumeration() {
    let mut rng = StdRng::seed_from_u64(0x5eed_5a17);
    let mut worst_gap: f64 = 0.0;
    let mut worst_regret: f64 = 0.0;

    for _ in 0..10 {
        let payoffs = (0..5)
            .map(|_| {
                (0..5)
                    .map(|_| rng.gen_range(-10.0..10.0))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let game = MatrixGame::zero_sum(&payoffs).unwrap();
        let minimax = zero_sum_minimax(&game).unwrap();
        let support = support_enumeration(&game).unwrap();

        assert!(!support.equilibria.is_empty());
        for equilibrium in support.equilibria {
            assert_close(
                minimax.value,
                expected_payoff(&game, 0, &equilibrium.strategies).unwrap(),
            );
        }
        assert!(minimax.duality_gap <= ASSERT_TOLERANCE);
        assert!(minimax.regret <= ASSERT_TOLERANCE);
        worst_gap = worst_gap.max(minimax.duality_gap);
        worst_regret = worst_regret.max(minimax.regret);
    }

    println!("seeded 5x5 worst duality_gap={worst_gap:e}, regret={worst_regret:e}");
}

#[test]
fn battle_of_sexes_security_levels_match_hand_calculation_for_both_roles() {
    let game = BattleOfSexes::default();

    let man = maximin_security_level(&game, 0).unwrap();
    assert_close(man.level, 1.0 / 5.0);
    assert_probs(man.strategy.probs(), &[2.0 / 5.0, 3.0 / 5.0]);
    assert!(man.duality_gap <= ASSERT_TOLERANCE);

    let woman = maximin_security_level(&game, 1).unwrap();
    assert_close(woman.level, 1.0 / 5.0);
    assert_probs(woman.strategy.probs(), &[3.0 / 5.0, 2.0 / 5.0]);
    assert!(woman.duality_gap <= ASSERT_TOLERANCE);
}

#[test]
fn minimax_rejects_non_zero_sum_and_unsupported_roles_and_security_checks_role() {
    let battle = BattleOfSexes::default();
    assert_eq!(zero_sum_minimax(&battle), Err(SolveError::NotZeroSum));

    let three_player = MatrixGame::new(
        vec![2, 2, 2],
        vec![vec![0.0; 8], vec![0.0; 8], vec![0.0; 8]],
    )
    .unwrap();
    assert_eq!(
        zero_sum_minimax(&three_player),
        Err(SolveError::UnsupportedRoles { n_roles: 3 })
    );
    assert_eq!(
        maximin_security_level(&three_player, 0),
        Err(SolveError::UnsupportedRoles { n_roles: 3 })
    );

    assert!(matches!(
        maximin_security_level(&battle, 2),
        Err(SolveError::InvalidOptions(_))
    ));
}

#[test]
fn asymmetric_two_by_three_zero_sum_game_is_supported() {
    let game = MatrixGame::zero_sum(&[vec![2.0, -1.0, 0.0], vec![-2.0, 1.0, 0.0]]).unwrap();

    let solution = zero_sum_minimax(&game).unwrap();
    assert_close(solution.value, 0.0);
    assert_probs(solution.row.probs(), &[0.5, 0.5]);
    assert!(solution.duality_gap <= ASSERT_TOLERANCE);
    assert!(solution.regret <= ASSERT_TOLERANCE);
}
