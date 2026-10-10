use std::collections::BTreeSet;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use socsim_games::catalog::{BattleOfSexes, MatchingPennies, PrisonersDilemma, RockPaperScissors};
use socsim_games::mixed::{expected_payoff, regret};
use socsim_games::solve::support::{support_enumeration, SupportFinding};
use socsim_games::solve::{lemke_howson, LemkeHowsonOptions};
use socsim_games::solve::{pure_nash, tolerance, Equilibrium, SolveError};
use socsim_games::{FiniteGame, MatrixGame};

const ASSERT_TOLERANCE: f64 = 1e-9;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= ASSERT_TOLERANCE,
        "expected {expected}, got {actual}"
    );
}

fn assert_strategy(equilibrium: &Equilibrium, role: usize, expected: &[f64]) {
    assert_eq!(equilibrium.strategies[role].probs().len(), expected.len());
    for (&actual, &expected) in equilibrium.strategies[role].probs().iter().zip(expected) {
        assert_close(actual, expected);
    }
}

fn assert_verified<G: FiniteGame>(game: &G, equilibrium: &Equilibrium) {
    let verified = regret(game, &equilibrium.strategies).unwrap();
    assert!(verified <= tolerance(game).unwrap());
    assert_close(equilibrium.regret, verified);
    for role in 0..game.n_roles() {
        assert_close(
            equilibrium.payoffs[role],
            expected_payoff(game, role, &equilibrium.strategies).unwrap(),
        );
    }
}

fn find_equilibrium<'a>(
    equilibria: &'a [Equilibrium],
    role_0: &[f64],
    role_1: &[f64],
) -> &'a Equilibrium {
    equilibria
        .iter()
        .find(|equilibrium| {
            equilibrium.strategies[0]
                .probs()
                .iter()
                .zip(role_0)
                .all(|(actual, expected)| (actual - expected).abs() <= ASSERT_TOLERANCE)
                && equilibrium.strategies[1]
                    .probs()
                    .iter()
                    .zip(role_1)
                    .all(|(actual, expected)| (actual - expected).abs() <= ASSERT_TOLERANCE)
        })
        .expect("expected equilibrium was not returned")
}

#[test]
fn battle_of_sexes_has_two_pure_and_one_mixed_equilibrium() {
    let game = BattleOfSexes::default();
    let result = support_enumeration(&game).unwrap();

    assert!(
        result.is_complete(),
        "unexpected findings: {:?}",
        result.findings
    );
    assert_eq!(result.equilibria.len(), 3);
    let bb = find_equilibrium(&result.equilibria, &[1.0, 0.0], &[1.0, 0.0]);
    let vv = find_equilibrium(&result.equilibria, &[0.0, 1.0], &[0.0, 1.0]);
    let mixed = find_equilibrium(&result.equilibria, &[0.6, 0.4], &[0.4, 0.6]);
    assert_eq!(bb.payoffs, vec![2.0, 1.0]);
    assert_eq!(vv.payoffs, vec![1.0, 2.0]);
    assert_close(mixed.payoffs[0], 0.2);
    assert_close(mixed.payoffs[1], 0.2);
    for equilibrium in &result.equilibria {
        assert_verified(&game, equilibrium);
        assert!(equilibrium.regret <= ASSERT_TOLERANCE);
    }
}

#[test]
fn matching_pennies_has_only_the_uniform_equilibrium() {
    let game = MatchingPennies::default();
    let result = support_enumeration(&game).unwrap();

    assert!(result.is_complete());
    assert_eq!(result.equilibria.len(), 1);
    let equilibrium = &result.equilibria[0];
    assert_strategy(equilibrium, 0, &[0.5, 0.5]);
    assert_strategy(equilibrium, 1, &[0.5, 0.5]);
    assert_verified(&game, equilibrium);
    assert!(equilibrium.regret <= ASSERT_TOLERANCE);
}

#[test]
fn prisoners_dilemma_and_rock_paper_scissors_have_expected_unique_equilibria() {
    let dilemma = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
    let result = support_enumeration(&dilemma).unwrap();
    assert!(result.is_complete());
    assert_eq!(result.equilibria.len(), 1);
    assert_strategy(&result.equilibria[0], 0, &[0.0, 1.0]);
    assert_strategy(&result.equilibria[0], 1, &[0.0, 1.0]);
    assert_verified(&dilemma, &result.equilibria[0]);

    let rps = RockPaperScissors::default();
    let result = support_enumeration(&rps).unwrap();
    assert!(
        result.is_complete(),
        "unexpected findings: {:?}",
        result.findings
    );
    assert_eq!(result.equilibria.len(), 1);
    assert_strategy(&result.equilibria[0], 0, &[1.0 / 3.0; 3]);
    assert_strategy(&result.equilibria[0], 1, &[1.0 / 3.0; 3]);
    assert_verified(&rps, &result.equilibria[0]);
}

fn random_game(rng: &mut StdRng, rows: usize, columns: usize) -> MatrixGame {
    let profiles = rows * columns;
    let a = (0..profiles)
        .map(|_| rng.gen_range(-10.0..10.0))
        .collect::<Vec<_>>();
    let b = (0..profiles)
        .map(|_| rng.gen_range(-10.0..10.0))
        .collect::<Vec<_>>();
    MatrixGame::new(vec![rows, columns], vec![a, b]).unwrap()
}

fn pure_profiles(equilibria: &[Equilibrium]) -> BTreeSet<Vec<usize>> {
    equilibria
        .iter()
        .filter(|equilibrium| equilibrium.is_pure())
        .map(|equilibrium| {
            equilibrium
                .strategies
                .iter()
                .map(|strategy| {
                    strategy
                        .probs()
                        .iter()
                        .position(|&probability| probability == 1.0)
                        .unwrap()
                })
                .collect()
        })
        .collect()
}

#[test]
fn seeded_non_degenerate_games_return_a_complete_odd_verified_set() {
    let mut rng = StdRng::seed_from_u64(0x5eed_cafe);
    for (rows, columns) in [(3, 3), (4, 4), (3, 5), (3, 3), (4, 4), (3, 5)] {
        let game = random_game(&mut rng, rows, columns);
        let result = support_enumeration(&game).unwrap();
        assert!(
            result.is_complete(),
            "unexpected findings for {rows}x{columns}: {:?}",
            result.findings
        );
        assert_eq!(result.equilibria.len() % 2, 1);
        assert!(!result.equilibria.is_empty());
        for equilibrium in &result.equilibria {
            assert_verified(&game, equilibrium);
        }

        let expected = pure_nash(&game)
            .unwrap()
            .into_iter()
            .map(|equilibrium| equilibrium.profile)
            .collect::<BTreeSet<_>>();
        assert_eq!(pure_profiles(&result.equilibria), expected);
    }
}

#[test]
fn degenerate_game_reports_a_component_or_degenerate_support_without_fabrication() {
    let game = MatrixGame::bimatrix(
        &[vec![1.0, 0.0], vec![1.0, 0.0]],
        &[vec![1.0, 0.0], vec![1.0, 0.0]],
    )
    .unwrap();
    let result = support_enumeration(&game).unwrap();

    assert!(result.findings.iter().any(|finding| matches!(
        finding,
        SupportFinding::Component { .. } | SupportFinding::Degenerate { .. }
    )));
    for finding in &result.findings {
        if let SupportFinding::Component { base, .. } = finding {
            assert_verified(&game, base);
        }
    }
    for equilibrium in &result.equilibria {
        assert_verified(&game, equilibrium);
    }
}

#[test]
fn rejects_unsupported_roles_and_support_pair_counts_above_the_limit() {
    let three_player = MatrixGame::new(
        vec![2, 2, 2],
        vec![vec![0.0; 8], vec![0.0; 8], vec![0.0; 8]],
    )
    .unwrap();
    assert_eq!(
        support_enumeration(&three_player),
        Err(SolveError::UnsupportedRoles { n_roles: 3 })
    );

    let eleven_by_ten =
        MatrixGame::new(vec![11, 10], vec![vec![0.0; 110], vec![0.0; 110]]).unwrap();
    assert_eq!(
        support_enumeration(&eleven_by_ten),
        Err(SolveError::TooLarge {
            profiles: 2_094_081
        })
    );
}

fn assert_equilibrium_matches(actual: &Equilibrium, expected: &Equilibrium) {
    assert_eq!(actual.strategies.len(), expected.strategies.len());
    actual
        .strategies
        .iter()
        .zip(&expected.strategies)
        .for_each(|(actual, expected)| {
            assert_eq!(actual.probs().len(), expected.probs().len());
            actual
                .probs()
                .iter()
                .zip(expected.probs())
                .for_each(|(&actual, &expected)| assert_close(actual, expected));
        });
}

fn assert_all_lemke_howson_labels_match_support_enumeration<G: FiniteGame>(game: &G) {
    let expected = support_enumeration(game).unwrap();
    assert!(
        expected.is_complete(),
        "unexpected findings: {:?}",
        expected.findings
    );

    let n_labels = game.n_actions(0) + game.n_actions(1);
    for dropped_label in 0..n_labels {
        let result = lemke_howson(
            game,
            &LemkeHowsonOptions::default().with_dropped_label(dropped_label),
        )
        .unwrap();
        assert!(result.equilibrium.regret <= ASSERT_TOLERANCE);
        assert_verified(game, &result.equilibrium);
        let matching = expected
            .equilibria
            .iter()
            .find(|expected| {
                result
                    .equilibrium
                    .strategies
                    .iter()
                    .zip(&expected.strategies)
                    .all(|(actual, expected)| {
                        actual
                            .probs()
                            .iter()
                            .zip(expected.probs())
                            .all(|(actual, expected)| (actual - expected).abs() <= ASSERT_TOLERANCE)
                    })
            })
            .expect("Lemke-Howson returned an equilibrium outside support enumeration");
        assert_equilibrium_matches(&result.equilibrium, matching);
    }
}

#[test]
fn lemke_howson_matches_support_enumeration_for_catalog_games() {
    assert_all_lemke_howson_labels_match_support_enumeration(&BattleOfSexes::default());
    assert_all_lemke_howson_labels_match_support_enumeration(&MatchingPennies::default());
    assert_all_lemke_howson_labels_match_support_enumeration(
        &PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap(),
    );
    assert_all_lemke_howson_labels_match_support_enumeration(&RockPaperScissors::default());
}

#[test]
fn lemke_howson_matches_support_enumeration_for_seeded_games() {
    let mut rng = StdRng::seed_from_u64(0x0001_e4be_5eed);
    for (rows, columns) in [(3, 3), (4, 4), (5, 3)] {
        let game = random_game(&mut rng, rows, columns);
        assert_all_lemke_howson_labels_match_support_enumeration(&game);
    }
}

#[test]
fn lemke_howson_lexicographic_pivoting_stops_on_a_degenerate_game() {
    // Every positive entry in a pivot column has the same initial ratio, so
    // each dropped label exercises the lexicographic minimum-ratio tie-break.
    let payoffs = vec![vec![1.0; 3]; 3];
    let game = MatrixGame::bimatrix(&payoffs, &payoffs).unwrap();

    for dropped_label in 0..6 {
        let result = lemke_howson(
            &game,
            &LemkeHowsonOptions::default().with_dropped_label(dropped_label),
        )
        .unwrap();
        assert!(result.pivots > 0);
        assert!(result.equilibrium.regret <= ASSERT_TOLERANCE);
        assert_verified(&game, &result.equilibrium);
    }
}

#[test]
fn lemke_howson_does_not_discard_positive_pivots_at_large_payoff_scales() {
    let game = MatrixGame::bimatrix(
        &[vec![2.0e12, 2.0e12], vec![0.0, 0.0]],
        &[vec![0.0, 0.0], vec![2.0e12, 1.0e12]],
    )
    .unwrap();

    let result = lemke_howson(&game, &LemkeHowsonOptions::default()).unwrap();
    assert!(result.equilibrium.regret <= ASSERT_TOLERANCE);
    assert_verified(&game, &result.equilibrium);
}

#[test]
fn lemke_howson_checks_options_and_role_count() {
    let defaults = LemkeHowsonOptions::default();
    assert_eq!(defaults.dropped_label, 0);
    assert_eq!(defaults.max_pivots, 100_000);

    let game = MatchingPennies::default();
    assert_eq!(
        lemke_howson(&game, &defaults.clone().with_dropped_label(4)),
        Err(SolveError::InvalidLabel {
            label: 4,
            n_labels: 4,
        })
    );
    assert_eq!(
        lemke_howson(&game, &defaults.with_max_pivots(1)),
        Err(SolveError::MaxPivots { limit: 1 })
    );

    let three_player = MatrixGame::new(
        vec![2, 2, 2],
        vec![vec![0.0; 8], vec![0.0; 8], vec![0.0; 8]],
    )
    .unwrap();
    assert_eq!(
        lemke_howson(&three_player, &LemkeHowsonOptions::default()),
        Err(SolveError::UnsupportedRoles { n_roles: 3 })
    );
}
