//! Proportional imitation in a well-mixed hawk-dove population approaches the
//! mixed equilibrium hawk share `V / C = 0.5`.
//!
//! Configuration: `V = 2`, `C = 4`, `RandomPairs`, `beta = 1 / 3` (the inverse
//! of the per-step payoff range `[-1, 2]`), `N = 10^4`, exactly 3000 initial
//! hawks placed uniformly at random, and 20 seeds. The measured quantity is
//! `game.population_share.a0` (the hawk share after the update) over steps
//! 201 to 1000. The 95% t interval (19 degrees of freedom) of the per-seed time
//! averages must lie inside `[0.45, 0.55]`.

use std::any::Any;
use std::time::Instant;

use rand::seq::SliceRandom;
use socsim_core::{derive_seed, Recorder, SimRng};
use socsim_games::catalog::HawkDove;
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, GameMetrics, InitialActions,
    MetricsOptions, NormalFormOutcome, Pairwise, PlayOptions, ProportionalImitation, RandomPairs,
    RuleBook, Update,
};

const N: usize = 10_000;
const INITIAL_HAWKS: usize = 3_000;
const STEPS: u64 = 1_000;
const BURN_IN: u64 = 200;
const SEEDS: u64 = 20;
const T_975_DF19: f64 = 2.093;
const HAWK: usize = 0;
const DOVE: usize = 1;

#[derive(Default)]
struct ShareRecorder {
    shares: Vec<(u64, f64)>,
}

impl Recorder for ShareRecorder {
    fn record_metric(&mut self, _t: u64, _key: &str, _value: f64) {}

    fn record_event(&mut self, _t: u64, _kind: &str, _payload: serde_json::Value) {}

    fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]) {
        if table != "game" {
            return;
        }
        for (column, value) in row {
            if *column == "population_share.a0" {
                self.shares.push((t, *value));
            }
        }
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

/// Returns the post-update hawk share recorded after each of the `STEPS` steps.
fn hawk_shares(seed: u64) -> Vec<f64> {
    let game = HawkDove::new(2.0, 4.0).unwrap();
    let metrics = GameMetrics::new(&game, MetricsOptions::new().population_share(true)).unwrap();
    let mut initial = vec![DOVE; N];
    initial[..INITIAL_HAWKS].fill(HAWK);
    initial.shuffle(&mut SimRng::from_seed(derive_seed(seed, &[0, 2])));
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
        STEPS,
        seed,
    )
    .unwrap();
    let mut simulation = assemble_with(
        world,
        RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        ProportionalImitation::new(1.0 / 3.0, Update::Synchronous).unwrap(),
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(ShareRecorder::default()))
    .build();
    for _ in 0..STEPS {
        simulation.step().unwrap();
    }
    let recorded = &simulation
        .recorder()
        .as_any()
        .unwrap()
        .downcast_ref::<ShareRecorder>()
        .unwrap()
        .shares;
    assert_eq!(recorded.len(), STEPS as usize);
    for window in recorded.windows(2) {
        assert_eq!(window[1].0, window[0].0 + 1);
    }
    let held_share = simulation
        .world()
        .held()
        .iter()
        .filter(|&&a| a == HAWK)
        .count() as f64
        / N as f64;
    assert_eq!(recorded.last().unwrap().1, held_share);
    recorded.iter().map(|&(_, share)| share).collect()
}

#[test]
fn proportional_imitation_hawk_share_converges_to_v_over_c() {
    let started = Instant::now();
    let averages: Vec<f64> = (0..SEEDS)
        .map(|index| {
            let shares = hawk_shares(derive_seed(0x6861_776b_646f_7665, &[index]));
            // Steps are numbered from 1; steps 201..=1000 are indices 200..1000.
            let window = &shares[BURN_IN as usize..];
            assert_eq!(window.len(), 800);
            window.iter().sum::<f64>() / window.len() as f64
        })
        .collect();
    let mean = averages.iter().sum::<f64>() / SEEDS as f64;
    let variance = averages
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / (SEEDS - 1) as f64;
    let half_width = T_975_DF19 * (variance / SEEDS as f64).sqrt();
    let (low, high) = (mean - half_width, mean + half_width);
    eprintln!(
        "hawk share: mean {mean:.5}, 95% CI [{low:.5}, {high:.5}] ({:.2?})",
        started.elapsed()
    );
    assert!(
        low >= 0.45 && high <= 0.55,
        "95% interval [{low}, {high}] is not inside [0.45, 0.55]"
    );
}
