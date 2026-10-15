//! Huberman and Glance's asynchronous spatial prisoner's dilemma, with a
//! synchronous Nowak--May control.
//!
//! A single defector starts at the center of a fixed 99 x 99 lattice. Every
//! agent plays the weak prisoner's dilemma with its eight Moore neighbors and
//! itself, then imitates the best earner. The asynchronous form activates 99 x
//! 99 agents per generation by uniform sampling with replacement and writes
//! each selected action immediately.
//!
//! The all-defection result is limited to `b > 1.8`; at or below that value,
//! cooperation remains even with asynchronous updates.
//!
//! ```sh
//! cargo run -p socsim-games --features core,engine,grid \
//!     --example huberman_glance -- --update async --steps 217 --seed 1
//! ```

use std::any::Any;

use socsim_core::Recorder;
use socsim_engine::SequentialScheduler;
use socsim_games::catalog::PrisonersDilemma;
use socsim_games::{
    assemble_async, assemble_with, build_world, AssignGroup, AssignKind, Assignment,
    AsyncActivation, AsyncPopulationStep, GameMetrics, GridBoundary, GridNeighborhood,
    GridNeighbors, GridSpec, ImitateBest, ImitationTie, InitialActions, MetricsOptions,
    NormalFormOutcome, Pairwise, PayoffTiming, PlayOptions, RuleBook, Topology, Update,
};

const SIDE: usize = 99;
const CENTER: usize = 49;
const B: f64 = 1.85;
const COOPERATE: usize = 0;
const DEFECT: usize = 1;
const DEFAULT_STEPS: u64 = 217;
const DEFAULT_SEED: u64 = 1;

#[derive(Clone, Copy)]
enum UpdateMode {
    Sync,
    Async,
}

struct Args {
    update: UpdateMode,
    steps: u64,
    seed: u64,
}

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
        if let Some((_, share)) = row
            .iter()
            .find(|(column, _)| *column == "population_share.a0")
        {
            self.shares.push((t, *share));
        }
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

fn parse_args() -> Args {
    let mut args = Args {
        update: UpdateMode::Async,
        steps: DEFAULT_STEPS,
        seed: DEFAULT_SEED,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--update" => {
                args.update = match iter.next().as_deref() {
                    Some("sync") => UpdateMode::Sync,
                    Some("async") => UpdateMode::Async,
                    _ => usage("--update needs sync or async"),
                };
            }
            "--steps" => args.steps = parse_u64(&mut iter, "--steps"),
            "--seed" => args.seed = parse_u64(&mut iter, "--seed"),
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    args
}

fn parse_u64(iter: &mut impl Iterator<Item = String>, name: &str) -> u64 {
    iter.next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| usage(&format!("{name} needs a non-negative integer")))
}

fn usage(message: &str) -> ! {
    if !message.is_empty() {
        eprintln!("error: {message}");
    }
    eprintln!("usage: huberman_glance [--update sync|async] [--steps <n>] [--seed <n>]");
    std::process::exit(if message.is_empty() { 0 } else { 2 });
}

fn main() {
    let args = parse_args();
    let n_agents = SIDE * SIDE;
    let game = PrisonersDilemma::weak(B).expect("b is a valid temptation");
    let timing = match args.update {
        UpdateMode::Sync => PayoffTiming::BeforeUpdate,
        UpdateMode::Async => PayoffTiming::AfterUpdate,
    };
    let metrics = GameMetrics::new(
        &game,
        MetricsOptions::new()
            .population_share(true)
            .payoff_timing(timing),
    )
    .expect("the metrics configuration is valid");

    let mut initial = vec![COOPERATE; n_agents];
    initial[CENTER * SIDE + CENTER] = DEFECT;
    let rules = RuleBook::new();
    let world = build_world(
        game,
        n_agents,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: n_agents,
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
            boundary: GridBoundary::Fixed,
            neighborhood: GridNeighbors::Moore,
        })
        .expect("the grid is non-empty"),
    )
    .expect("the grid matches the population");

    let builder = match args.update {
        UpdateMode::Sync => assemble_with(
            world,
            GridNeighborhood::new().include_self(true),
            rules,
            Pairwise::new(NormalFormOutcome),
            ImitateBest::new(Update::Synchronous).tie_break(ImitationTie::KeepOwn),
            PlayOptions::new(),
            args.seed,
        ),
        UpdateMode::Async => assemble_async(
            world,
            GridNeighborhood::new().include_self(true),
            Pairwise::new(NormalFormOutcome),
            AsyncPopulationStep::new(
                ImitateBest::new(Update::Asynchronous).tie_break(ImitationTie::KeepOwn),
            )
            .activation(AsyncActivation::UniformWithReplacement),
            PlayOptions::new(),
            args.seed,
        ),
    }
    .expect("the simulation configuration is valid");
    let mut simulation = builder
        .scheduler(Box::new(SequentialScheduler))
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
    for (generation, share) in shares {
        println!("{generation},{share}");
    }
}
