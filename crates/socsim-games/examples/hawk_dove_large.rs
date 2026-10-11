//! A large well-mixed hawk-dove population under proportional imitation.
//!
//! `n` agents (default 10 000; `--n 1000000` is supported) are paired uniformly
//! at random every step and play hawk-dove with `V = 2`, `C = 4`. All agents
//! hold their action and update it synchronously by proportional imitation
//! with `beta = 1 / 3`, the inverse of the per-step payoff range `[-1, 2]`.
//! Half of the agents start as hawks, placed uniformly at random from the
//! seed. Only `held` actions are kept between steps (`history_len = 1`), and
//! no metrics mechanism is attached, so memory stays proportional to `n`.
//!
//! ```sh
//! cargo run --release -p socsim-games --features core,engine \
//!     --example hawk_dove_large -- --n 1000000 --steps 10
//! ```

use rand::seq::SliceRandom;
use socsim_core::{derive_seed, SimRng};
use socsim_games::catalog::HawkDove;
use socsim_games::{
    assemble_with, build_world, AssignGroup, AssignKind, Assignment, InitialActions,
    NormalFormOutcome, Pairwise, PlayOptions, ProportionalImitation, RandomPairs, RuleBook, Update,
};

const HAWK: usize = 0;
const DOVE: usize = 1;
const DEFAULT_N: u64 = 10_000;
const DEFAULT_STEPS: u64 = 10;
const DEFAULT_SEED: u64 = 1;

struct Args {
    n: usize,
    steps: u64,
    seed: u64,
}

fn parse_args() -> Args {
    let mut n = DEFAULT_N;
    let mut steps = DEFAULT_STEPS;
    let mut seed = DEFAULT_SEED;
    let mut iter = std::env::args().skip(1);
    while let Some(flag) = iter.next() {
        let mut value = |name: &str| -> u64 {
            iter.next()
                .and_then(|value| value.parse().ok())
                .unwrap_or_else(|| usage(&format!("{name} needs a non-negative integer")))
        };
        match flag.as_str() {
            "--n" => n = value("--n"),
            "--steps" => steps = value("--steps"),
            "--seed" => seed = value("--seed"),
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    let n = usize::try_from(n).unwrap_or_else(|_| usage("--n does not fit in usize"));
    if n < 2 {
        usage("--n must be at least 2");
    }
    Args { n, steps, seed }
}

fn usage(message: &str) -> ! {
    if !message.is_empty() {
        eprintln!("error: {message}");
    }
    eprintln!("usage: hawk_dove_large [--n <agents>] [--steps <n>] [--seed <n>]");
    std::process::exit(if message.is_empty() { 0 } else { 2 });
}

fn hawk_share(held: &[usize]) -> f64 {
    held.iter().filter(|&&action| action == HAWK).count() as f64 / held.len() as f64
}

fn main() {
    let args = parse_args();
    let game = HawkDove::new(2.0, 4.0).expect("V and C are valid");

    let mut initial = vec![DOVE; args.n];
    initial[..args.n / 2].fill(HAWK);
    initial.shuffle(&mut SimRng::from_seed(derive_seed(args.seed, &[0, 2])));

    let rules = RuleBook::new();
    let world = build_world(
        game,
        args.n,
        (),
        InitialActions::PerAgent(initial),
        &rules,
        &Assignment::Ordered(vec![AssignGroup {
            kind: AssignKind::Held,
            count: args.n,
        }]),
        1,
        args.steps,
        args.seed,
    )
    .expect("the world configuration is valid");
    let initial_share = hawk_share(world.held());
    let mut simulation = assemble_with(
        world,
        RandomPairs::new(),
        rules,
        Pairwise::new(NormalFormOutcome),
        ProportionalImitation::new(1.0 / 3.0, Update::Synchronous).expect("beta is valid"),
        PlayOptions::new(),
        args.seed,
    )
    .expect("the simulation configuration is valid")
    .build();

    for _ in 0..args.steps {
        simulation.step().expect("the game step succeeds");
    }

    let world = simulation.world();
    let mean_payoff = world.payoffs().iter().sum::<f64>() / args.n as f64;
    println!(
        "n={} steps={} seed={} hawk_share_initial={initial_share} hawk_share_final={} \
         mean_payoff_last_step={mean_payoff}",
        args.n,
        args.steps,
        args.seed,
        hawk_share(world.held()),
    );
}
