//! Integration test for the learnable turnover policy (design §14.1.7).
//!
//! Only compiled with the `marl` feature:
//! `cargo test -p socsim-packs --features marl`.
#![cfg(feature = "marl")]

use std::cell::RefCell;
use std::rc::Rc;

use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{Mechanism, SimClock, SimRng};
use socsim_engine::{SequentialScheduler, Simulation, SimulationBuilder};
use socsim_packs::hr_lifecycle::marl::{
    TurnoverActionApplier, TurnoverObsEncoder, TurnoverPrepMechanism, TurnoverReward,
    TURNOVER_OBS_DIM,
};
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_marl::{
    DiscretePolicyNet, EpisodeStat, MarlTrainer, NetConfig, PolicyMechanism, TrainConfig,
    TrajectoryBuffer,
};

fn build_sim(
    seed: u64,
    turnover: Box<dyn Mechanism<HrWorld>>,
) -> Simulation<HrWorld> {
    let mut rng = SimRng::from_seed(seed ^ 0xABCD);
    let mut world = HrWorld::new(3, 6, 4, 0.1, &mut rng);
    world.clock = SimClock::new(18);

    let mut reg: Registry<HrWorld> = Registry::new();
    HrLifecyclePack.register(&mut reg);
    let p = Params::empty();
    SimulationBuilder::new(world)
        .scheduler(Box::new(SequentialScheduler))
        .seed(seed)
        .add_mechanism(Box::new(TurnoverPrepMechanism))
        .add_mechanism(reg.build("learning_curve", &p).unwrap())
        .add_mechanism(reg.build("fit", &p).unwrap())
        .add_mechanism(turnover)
        .add_mechanism(reg.build("org_performance", &p).unwrap())
        .add_mechanism(reg.build("knowledge_loss", &p).unwrap())
        .build()
}

fn train(seed: u64) -> Vec<EpisodeStat> {
    let cfg = NetConfig {
        obs_dim: TURNOVER_OBS_DIM,
        hidden: 16,
        n_actions: 2,
        lr: 0.02,
        gamma: 0.95,
    };
    let mut rng = SimRng::from_seed(seed);
    let net = Rc::new(RefCell::new(DiscretePolicyNet::new(cfg, &mut rng).unwrap()));
    let mut trainer = MarlTrainer::new(net);
    trainer
        .train(
            &TrainConfig {
                episodes: 30,
                seed: 7,
            },
            |policy: Rc<RefCell<DiscretePolicyNet>>,
             buffer: Rc<RefCell<TrajectoryBuffer>>,
             s| {
                let turnover = Box::new(PolicyMechanism::collecting(
                    policy,
                    TurnoverObsEncoder,
                    TurnoverActionApplier,
                    buffer,
                ));
                build_sim(s, turnover)
            },
            &TurnoverReward,
        )
        .unwrap()
}

#[test]
fn learned_turnover_policy_improves_episode_reward() {
    let stats = train(42);
    let first = stats[..5].iter().map(|s| s.total_reward).sum::<f32>() / 5.0;
    let last = stats[stats.len() - 5..]
        .iter()
        .map(|s| s.total_reward)
        .sum::<f32>()
        / 5.0;
    assert!(
        last > first,
        "learned policy should raise episode reward: first5={first:.1}, last5={last:.1}"
    );
}

#[test]
fn training_is_reproducible() {
    let a = train(42);
    let b = train(42);
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.total_reward, y.total_reward);
        assert_eq!(x.loss, y.loss);
    }
}

#[test]
fn frozen_policy_inference_is_deterministic() {
    // Two greedy inference runs of the same (untrained) net must match exactly.
    let cfg = NetConfig::new(TURNOVER_OBS_DIM, 2);
    let mut rng = SimRng::from_seed(123);
    let net = Rc::new(RefCell::new(DiscretePolicyNet::new(cfg, &mut rng).unwrap()));

    let run = || {
        let turnover = Box::new(PolicyMechanism::inference(
            net.clone(),
            TurnoverObsEncoder,
            TurnoverActionApplier,
        ));
        let mut sim = build_sim(2024, turnover);
        sim.run().unwrap();
        sim.world().employee_count()
    };
    assert_eq!(run(), run());
}
