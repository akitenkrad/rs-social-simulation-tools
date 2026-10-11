//! Fixation probabilities of the Moran birth-death process.
//!
//! With `N = 10`, `RandomPairs` gives every agent exactly one encounter, so a
//! game with constant payoffs yields constant fitness under
//! `Fitness::Linear { w: 1 }`. The fixation probability of a single mutant is
//! `1 / N` when neutral and `(1 - 1/r) / (1 - 1/r^N)` with relative fitness
//! `r`. Each case counts fixations over a fixed number of independent trials
//! and accepts the count in an equal-tailed region of the exact binomial
//! distribution at the theoretical probability (each tail at most 2.5e-4).

use std::time::Instant;

use socsim_core::derive_seed;
use socsim_engine::Simulation;
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, Fitness, GameWorld,
    InitialActions, MatrixGame, MoranUpdate, NormalFormOutcome, Pairwise, PlayOptions, RandomPairs,
    RuleBook, Update,
};

const N: usize = 10;
const MAX_STEPS: u64 = 2000;
const BASE_SEED: u64 = 0x6d6f_7261_6e5f_6669;

fn symmetric(a: [[f64; 2]; 2]) -> MatrixGame {
    MatrixGame::bimatrix(
        &[a[0].to_vec(), a[1].to_vec()],
        &[vec![a[0][0], a[1][0]], vec![a[0][1], a[1][1]]],
    )
    .unwrap()
}

fn simulation(game: MatrixGame, seed: u64) -> Simulation<GameWorld<MatrixGame>> {
    let mut initial = vec![0; N];
    initial[0] = 1;
    let rules = RuleBook::new();
    let world = build_world(
        game,
        N,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: N,
        }]),
        1,
        MAX_STEPS,
        seed,
    )
    .unwrap();
    assemble_with(
        world,
        RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        MoranUpdate::new(Fitness::Linear { w: 1.0 }, 0.0, Update::Synchronous).unwrap(),
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build()
}

/// Runs one trial to absorption and reports whether action 1 fixed.
fn fixes(game: &MatrixGame, seed: u64) -> bool {
    let mut simulation = simulation(game.clone(), seed);
    for _ in 0..MAX_STEPS {
        simulation.step().unwrap();
        let held = simulation.world().held();
        let mutants = held.iter().filter(|&&action| action == 1).count();
        if mutants == N {
            return true;
        }
        if mutants == 0 {
            return false;
        }
    }
    panic!("trial with seed {seed} did not absorb within {MAX_STEPS} steps");
}

fn count_fixations(game: &MatrixGame, case: u64, trials: u64) -> u64 {
    let base = derive_seed(BASE_SEED, &[case]);
    (0..trials)
        .filter(|&trial| fixes(game, derive_seed(base, &[trial])))
        .count() as u64
}

#[test]
fn neutral_fixation_probability_is_one_over_n() {
    let started = Instant::now();
    let fixed = count_fixations(&symmetric([[1.0, 1.0], [1.0, 1.0]]), 0, 40_000);
    eprintln!(
        "neutral: {fixed} fixations in 40000 trials ({:.2?})",
        started.elapsed()
    );
    assert!(
        (3793..=4210).contains(&fixed),
        "neutral fixation count {fixed} is outside [3793, 4210] (rho = 0.1)"
    );
}

#[test]
fn constant_fitness_two_fixation_probability_matches_the_moran_formula() {
    let started = Instant::now();
    let fixed = count_fixations(&symmetric([[1.0, 1.0], [2.0, 2.0]]), 1, 8_000);
    eprintln!(
        "r = 2: {fixed} fixations in 8000 trials ({:.2?})",
        started.elapsed()
    );
    // rho = (1 - 1/2) / (1 - 1/2^10) = 512 / 1023.
    assert!(
        (3848..=4160).contains(&fixed),
        "r = 2 fixation count {fixed} is outside [3848, 4160] (rho = 512/1023)"
    );
}
