//! Spatial prisoner's dilemma of Nowak and May (1992) on a 100 x 100 torus.
//!
//! Every agent plays the weak prisoner's dilemma (`T = b = 1.85`, `R = 1`,
//! `P = S = 0`) with its eight Moore neighbors and with itself, then all agents
//! synchronously copy the action of the highest-earning agent among themselves
//! and their neighbors (ties keep the own action). Ten percent of the agents
//! start as defectors, placed uniformly at random from the seed.
//!
//! The cooperator share after each generation is read from the
//! `game.population_share.a0` column of [`GameMetrics`] and printed as CSV.
//! Two runs with the same seed print identical output.
//!
//! ```sh
//! cargo run --release -p socsim-games --features core,engine,grid \
//!     --example nowak_may -- --steps 200 --seed 1
//! ```

use std::any::Any;

use rand::seq::SliceRandom;
use socsim_core::{derive_seed, Recorder, SimRng};
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, GameMetrics, GridBoundary,
    GridNeighborhood, GridNeighbors, GridSpec, ImitateBest, ImitationTie, InitialActions,
    MetricsOptions, NormalFormOutcome, Pairwise, PlayOptions, RuleBook, Topology, Update,
};

const SIDE: usize = 100;
const B: f64 = 1.85;
const INITIAL_DEFECTOR_FRACTION: f64 = 0.1;
const COOPERATE: usize = 0;
const DEFECT: usize = 1;
const DEFAULT_STEPS: u64 = 200;
const DEFAULT_SEED: u64 = 1;

/// Keeps the cooperator share reported by `game_metrics` for every step.
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

struct Args {
    steps: u64,
    seed: u64,
}

fn parse_args() -> Args {
    let mut args = Args {
        steps: DEFAULT_STEPS,
        seed: DEFAULT_SEED,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(flag) = iter.next() {
        let mut value = |name: &str| -> u64 {
            iter.next()
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(|| usage(&format!("{name} needs a non-negative integer")))
        };
        match flag.as_str() {
            "--steps" => args.steps = value("--steps"),
            "--seed" => args.seed = value("--seed"),
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    args
}

fn usage(message: &str) -> ! {
    if !message.is_empty() {
        eprintln!("error: {message}");
    }
    eprintln!("usage: nowak_may [--steps <n>] [--seed <n>]");
    std::process::exit(if message.is_empty() { 0 } else { 2 });
}

fn main() {
    let args = parse_args();
    let n = SIDE * SIDE;
    let game = PrisonersDilemma::weak(B).expect("b is a valid temptation");
    let metrics = GameMetrics::new(&game, MetricsOptions::new().population_share(true))
        .expect("the metrics configuration is valid");

    let defectors = (n as f64 * INITIAL_DEFECTOR_FRACTION).round() as usize;
    let mut initial = vec![COOPERATE; n];
    initial[..defectors].fill(DEFECT);
    initial.shuffle(&mut SimRng::from_seed(derive_seed(args.seed, &[0, 2])));

    let rules = RuleBook::new();
    let world = build_world(
        game,
        n,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n,
        }]),
        1,
        args.steps,
        args.seed,
    )
    .expect("the world configuration is valid")
    .with_topology(
        Topology::grid(GridSpec {
            rows: SIDE,
            cols: SIDE,
            boundary: GridBoundary::Toroidal,
            neighborhood: GridNeighbors::Moore,
        })
        .expect("the grid is non-empty"),
    )
    .expect("the grid matches the population");
    let mut simulation = assemble_with(
        world,
        GridNeighborhood::new().include_self(true),
        rules,
        Pairwise::new(NormalFormOutcome),
        ImitateBest::new(Update::Synchronous).tie_break(ImitationTie::KeepOwn),
        PlayOptions::new(),
        args.seed,
    )
    .expect("the simulation configuration is valid")
    .add_mechanism(Box::new(metrics))
    .recorder(Box::new(ShareRecorder::default()))
    .build();

    for _ in 0..args.steps {
        simulation.step().expect("the game step succeeds");
    }

    let shares = &simulation
        .recorder()
        .as_any()
        .and_then(|recorder| recorder.downcast_ref::<ShareRecorder>())
        .expect("the recorder is a ShareRecorder")
        .shares;
    assert_eq!(shares.len() as u64, args.steps, "one share per generation");
    println!("generation,cooperator_share");
    for (t, share) in shares {
        println!("{t},{share}");
    }
}
