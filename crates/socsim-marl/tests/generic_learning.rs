//! End-to-end test of the MARL stack on a minimal learnable environment.
//!
//! `SignWorld` gives each agent a context `c ∈ {−1, +1}` (resampled every step).
//! The reward is `+1` when the chosen action matches the sign of the context
//! (`c > 0 → action 1`, else `action 0`), `0` otherwise.  A working policy
//! gradient must drive episode reward upward — and do so deterministically.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rand::Rng;
use socsim_core::{
    AgentId, Mechanism, Phase, Result, SimClock, SimRng, StepContext, WorldState,
};
use socsim_engine::{SequentialScheduler, Simulation, SimulationBuilder};
use socsim_marl::{
    ActionApplier, DiscretePolicyNet, MarlTrainer, NetConfig, ObsEncoder, PolicyMechanism,
    RewardFn, TrainConfig, TrajectoryBuffer,
};

// ── world ──────────────────────────────────────────────────────────────────

struct SignWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    ctx: BTreeMap<AgentId, f32>,
    last_action: BTreeMap<AgentId, usize>,
}

impl SignWorld {
    fn new(t_max: u64, n: u64) -> Self {
        Self {
            clock: SimClock::new(t_max),
            agents: (0..n).map(AgentId).collect(),
            ctx: BTreeMap::new(),
            last_action: BTreeMap::new(),
        }
    }
}

impl WorldState for SignWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        self.agents.clone()
    }
    fn clock(&self) -> &SimClock {
        &self.clock
    }
    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

// ── PreStep: resample each agent's context ───────────────────────────────────

struct ContextMechanism;

impl Mechanism<SignWorld> for ContextMechanism {
    fn name(&self) -> &str {
        "context"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::PreStep]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SignWorld>) -> Result<()> {
        let ids = ctx.world.agent_ids();
        for aid in ids {
            let c = if ctx.rng.gen::<bool>() { 1.0 } else { -1.0 };
            ctx.world.ctx.insert(aid, c);
        }
        Ok(())
    }
}

// ── encoder / applier / reward ───────────────────────────────────────────────

struct SignEncoder;
impl ObsEncoder<SignWorld> for SignEncoder {
    fn obs_dim(&self) -> usize {
        1
    }
    fn encode(&self, world: &SignWorld, agent: AgentId) -> Option<Vec<f32>> {
        world.ctx.get(&agent).map(|&c| vec![c])
    }
}

struct SignApplier;
impl ActionApplier<SignWorld> for SignApplier {
    fn n_actions(&self) -> usize {
        2
    }
    fn apply(&self, world: &mut SignWorld, agent: AgentId, action: usize, _rng: &mut SimRng) {
        world.last_action.insert(agent, action);
    }
}

struct SignReward;
impl RewardFn<SignWorld> for SignReward {
    fn reward(&self, world: &SignWorld, agent: AgentId) -> f32 {
        let c = world.ctx.get(&agent).copied().unwrap_or(0.0);
        let a = world.last_action.get(&agent).copied().unwrap_or(0);
        let want = if c > 0.0 { 1 } else { 0 };
        if a == want {
            1.0
        } else {
            0.0
        }
    }
}

// ── factory shared by the runs ───────────────────────────────────────────────

fn build_env(
    policy: Rc<RefCell<DiscretePolicyNet>>,
    buffer: Rc<RefCell<TrajectoryBuffer>>,
    seed: u64,
) -> Simulation<SignWorld> {
    let world = SignWorld::new(20, 4);
    SimulationBuilder::new(world)
        .scheduler(Box::new(SequentialScheduler))
        .seed(seed)
        .add_mechanism(Box::new(ContextMechanism))
        .add_mechanism(Box::new(PolicyMechanism::collecting(
            policy, SignEncoder, SignApplier, buffer,
        )))
        .build()
}

fn train_run(seed: u64) -> Vec<socsim_marl::EpisodeStat> {
    let cfg = NetConfig {
        obs_dim: 1,
        hidden: 8,
        n_actions: 2,
        lr: 0.05,
        gamma: 0.95,
    };
    let mut rng = SimRng::from_seed(seed);
    let net = Rc::new(RefCell::new(DiscretePolicyNet::new(cfg, &mut rng).unwrap()));
    let mut trainer = MarlTrainer::new(net);
    trainer
        .train(
            &TrainConfig { episodes: 80, seed },
            build_env,
            &SignReward,
        )
        .unwrap()
}

#[test]
fn policy_gradient_improves_reward() {
    let stats = train_run(0);
    let first: f32 = stats[..10].iter().map(|s| s.total_reward).sum::<f32>() / 10.0;
    let last: f32 = stats[stats.len() - 10..]
        .iter()
        .map(|s| s.total_reward)
        .sum::<f32>()
        / 10.0;
    // 4 agents × 20 steps = 80 max reward per episode.  Random ≈ 40.
    assert!(
        last > first + 10.0,
        "expected learning: first10={first:.1}, last10={last:.1}"
    );
}

#[test]
fn training_is_deterministic_for_same_seed() {
    let a = train_run(0);
    let b = train_run(0);
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.total_reward, y.total_reward, "reward diverged");
        assert_eq!(x.loss, y.loss, "loss diverged");
    }
}
