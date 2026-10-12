//! Simultaneous best responses in linear Cournot competition (Theocharis 1960).
//!
//! The Jacobian of the simultaneous best-response map has eigenvalues
//! `-(n - 1) / 2` (the symmetric direction) and `1 / 2`, so undamped iteration
//! is neutral for three firms and unstable for four or more. Damping by `alpha`
//! gives eigenvalues `1 - alpha (n + 1) / 2` and `1 - alpha / 2`, which converge
//! for `alpha < 4 / (n + 1)`.

use socsim_games::continuous::Cournot;
use socsim_games::solve::{
    best_response_iteration, best_response_step, BrIterationOptions, BrStatus,
};

const A: f64 = 100.0;
const B: f64 = 1.0;
const C: f64 = 10.0;
const Q_MAX: f64 = 100.0;

fn cournot(n: usize) -> Cournot {
    Cournot::new(n, A, B, C, Q_MAX).unwrap()
}

fn max_deviation(profile: &[Vec<f64>], target: f64) -> f64 {
    profile
        .iter()
        .map(|action| (action[0] - target).abs())
        .fold(0.0, f64::max)
}

fn inside_bounds(profile: &[Vec<f64>]) -> bool {
    profile
        .iter()
        .all(|action| action.len() == 1 && (0.0..=Q_MAX).contains(&action[0]))
}

#[test]
fn three_firms_cycle_forever_without_damping() {
    let game = cournot(3);
    let q_star = game.equilibrium_quantity();
    assert_eq!(q_star, 22.5);

    let mut profile = vec![vec![27.5]; 3];
    for t in 1..=1000 {
        profile = best_response_step(&game, &profile, 1.0, None).unwrap();
        // 27.5 -> 17.5 -> 27.5 -> ... exactly: every value is a dyadic rational.
        let expected = if t % 2 == 1 { 17.5 } else { 27.5 };
        assert_eq!(profile, vec![vec![expected]; 3], "t = {t}");
        assert_eq!(max_deviation(&profile, q_star), 5.0, "t = {t}");
    }
}

#[test]
fn five_firms_move_away_and_settle_on_a_boundary_cycle() {
    let game = cournot(5);
    let q_star = game.equilibrium_quantity();
    assert_eq!(q_star, 15.0);
    let monopoly = (A - C) / (2.0 * B);
    assert_eq!(monopoly, 45.0);

    let mut profile = vec![vec![q_star + 1e-3]; 5];
    let initial_deviation = max_deviation(&profile, q_star);
    let mut previous_is_zero = None;
    for t in 1..=1000 {
        profile = best_response_step(&game, &profile, 1.0, None).unwrap();
        // The projection keeps the iterates in the box: no divergence.
        assert!(inside_bounds(&profile), "t = {t}: {profile:?}");
        if t == 10 {
            // The deviation doubles at each step near the equilibrium.
            assert!(max_deviation(&profile, q_star) > 100.0 * initial_deviation);
        }
        if t >= 100 {
            let all_zero = profile.iter().all(|action| action[0] == 0.0);
            let all_monopoly = profile.iter().all(|action| action[0] == monopoly);
            assert!(all_zero || all_monopoly, "t = {t}: {profile:?}");
            if let Some(previous) = previous_is_zero {
                assert_ne!(previous, all_zero, "t = {t}: the cycle must alternate");
            }
            previous_is_zero = Some(all_zero);
        }
    }
}

#[test]
fn damped_steps_converge_for_three_and_five_firms() {
    for n in [3, 5] {
        let game = cournot(n);
        let q_star = game.equilibrium_quantity();
        // An asymmetric start excites both eigen-directions.
        let mut profile: Vec<Vec<f64>> = (0..n)
            .map(|i| vec![Q_MAX * i as f64 / (n - 1) as f64])
            .collect();
        for _ in 0..1000 {
            profile = best_response_step(&game, &profile, 0.5, None).unwrap();
        }
        assert!(
            max_deviation(&profile, q_star) <= 1e-9,
            "n = {n}: {profile:?}"
        );
    }
}

#[test]
fn ten_firms_need_a_damping_below_four_elevenths() {
    let game = cournot(10);
    let q_star = game.equilibrium_quantity();
    let start: Vec<Vec<f64>> = (0..10).map(|i| vec![10.0 * i as f64]).collect();

    // alpha = 0.5 > 4 / 11: the symmetric eigenvalue is -1.75 and the iterates
    // stay away from the equilibrium.
    let mut profile = start.clone();
    for _ in 0..1000 {
        profile = best_response_step(&game, &profile, 0.5, None).unwrap();
        assert!(inside_bounds(&profile));
    }
    assert!(max_deviation(&profile, q_star) > 1.0, "{profile:?}");

    // alpha = 0.25: the eigenvalues are -0.375 and 0.875.
    let mut profile = start.clone();
    for _ in 0..1000 {
        profile = best_response_step(&game, &profile, 0.25, None).unwrap();
    }
    assert!(max_deviation(&profile, q_star) <= 1e-9, "{profile:?}");
}

#[test]
fn the_solver_damps_and_halves_as_the_eigenvalues_require() {
    for (n, damping) in [(3, 0.5), (5, 0.5), (10, 0.25)] {
        let game = cournot(n);
        let x0: Vec<Vec<f64>> = (0..n).map(|i| vec![(10 * i) as f64]).collect();
        let result = best_response_iteration(&game, &x0, &BrIterationOptions::new()).unwrap();
        assert_eq!(result.status, BrStatus::Converged, "n = {n}");
        assert_eq!(result.damping, damping, "n = {n}");
        assert!(max_deviation(&result.profile, game.equilibrium_quantity()) <= 1e-8);
    }

    // Undamped iteration of three firms never converges, and the solver says so
    // instead of returning the last iterate as an equilibrium.
    let game = cournot(3);
    let result = best_response_iteration(
        &game,
        &[vec![27.5], vec![27.5], vec![27.5]],
        &BrIterationOptions::new().damping(1.0).adaptive(false),
    )
    .unwrap();
    assert_eq!(result.status, BrStatus::MaxIter);
    assert_eq!(result.residual, 10.0);
}
