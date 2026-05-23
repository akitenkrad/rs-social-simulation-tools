//! Train a learnable turnover policy on the HR lifecycle ABM (design §14.1.7).
//!
//! A burn policy network replaces the fixed `turnover` logit: each month every
//! employee chooses stay/quit, rewarded by individual-rationality utility
//! (stay = `0.5·satisfaction + 0.5·embeddedness`, quit = a fixed outside option).
//! The policy should learn to retain satisfied, embedded employees and shed
//! unhappy ones.
//!
//! Run with:
//! ```bash
//! cargo run -p socsim-hr-lifecycle --features marl --example marl_turnover
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{SimClock, SimRng};
use socsim_engine::{SequentialScheduler, Simulation, SimulationBuilder};
use socsim_hr_lifecycle::marl::{
    TurnoverActionApplier, TurnoverObsEncoder, TurnoverPrepMechanism, TurnoverReward,
    TURNOVER_OBS_DIM,
};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_marl::{
    DiscretePolicyNet, MarlTrainer, NetConfig, PolicyMechanism, TrainConfig, TrajectoryBuffer,
};

const N_TEAMS: usize = 4;
const TEAM_SIZE: usize = 6;
const T_MAX: u64 = 24;
const EPISODES: usize = 40;

/// Build the HR world plus the mechanism stack for one episode.
fn build_world(seed: u64) -> HrWorld {
    let mut rng = SimRng::from_seed(seed ^ 0xABCD);
    let mut world = HrWorld::new(N_TEAMS, TEAM_SIZE, 4, 0.1, &mut rng);
    world.clock = SimClock::new(T_MAX);
    world
}

/// Add the shared HR mechanisms (everything except the turnover decision) to a builder.
fn with_hr_stack(
    builder: SimulationBuilder<HrWorld>,
    turnover: Box<dyn socsim_core::Mechanism<HrWorld>>,
) -> Simulation<HrWorld> {
    let mut reg: Registry<HrWorld> = Registry::new();
    HrLifecyclePack.register(&mut reg);
    let p = Params::empty();
    builder
        .scheduler(Box::new(SequentialScheduler))
        .add_mechanism(Box::new(TurnoverPrepMechanism)) // PreStep: tenure + headcount
        .add_mechanism(reg.build("learning_curve", &p).unwrap()) // Environment
        .add_mechanism(reg.build("fit", &p).unwrap()) // Decision: updates satisfaction
        .add_mechanism(turnover) // Decision: the learned policy
        .add_mechanism(reg.build("org_performance", &p).unwrap()) // Reward
        .add_mechanism(reg.build("knowledge_loss", &p).unwrap()) // PostStep: drains departed
        .build()
}

fn main() {
    let cfg = NetConfig {
        obs_dim: TURNOVER_OBS_DIM,
        hidden: 16,
        n_actions: 2,
        lr: 0.02,
        gamma: 0.95,
    };
    let mut init_rng = SimRng::from_seed(42);
    let net = Rc::new(RefCell::new(
        DiscretePolicyNet::new(cfg, &mut init_rng).unwrap(),
    ));

    // ── Train ────────────────────────────────────────────────────────────────
    let mut trainer = MarlTrainer::new(net);
    let env_factory =
        |policy: Rc<RefCell<DiscretePolicyNet>>, buffer: Rc<RefCell<TrajectoryBuffer>>, seed| {
            let builder = SimulationBuilder::new(build_world(seed)).seed(seed);
            let turnover = Box::new(PolicyMechanism::collecting(
                policy,
                TurnoverObsEncoder,
                TurnoverActionApplier,
                buffer,
            ));
            with_hr_stack(builder, turnover)
        };

    let stats = trainer
        .train(
            &TrainConfig {
                episodes: EPISODES,
                seed: 7,
            },
            env_factory,
            &TurnoverReward,
        )
        .unwrap();

    println!("=== Training (episode reward = sum of per-agent utilities) ===");
    for s in stats.iter().step_by(5) {
        println!(
            "  ep {:>3}: reward = {:>8.2}   loss = {:>8.4}",
            s.episode, s.total_reward, s.loss
        );
    }
    let first = stats[..5].iter().map(|s| s.total_reward).sum::<f32>() / 5.0;
    let last = stats[stats.len() - 5..]
        .iter()
        .map(|s| s.total_reward)
        .sum::<f32>()
        / 5.0;
    println!("  first 5 episodes avg reward: {first:.2}");
    println!("  last  5 episodes avg reward: {last:.2}");

    // ── Inference with the frozen learned policy ───────────────────────────────
    println!("\n=== Greedy inference episode (frozen policy) ===");
    let policy = trainer.policy();
    let initial = N_TEAMS * TEAM_SIZE;
    let turnover = Box::new(PolicyMechanism::inference(
        policy,
        TurnoverObsEncoder,
        TurnoverActionApplier,
    ));
    let mut sim = with_hr_stack(SimulationBuilder::new(build_world(1000)).seed(1000), turnover);
    sim.run().unwrap();
    let remaining = sim.world().employee_count();
    println!("  headcount: {initial} → {remaining}");
    println!(
        "  retained {:.0}% over {T_MAX} months",
        100.0 * remaining as f64 / initial as f64
    );
    println!("  org_performance: {:.2}", sim.world().org_performance);
}
