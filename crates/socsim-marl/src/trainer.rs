//! [`MarlTrainer`] — the outer learning loop described in §14.1.3.
//!
//! One episode = one full simulation run.  The trainer:
//!
//! 1. clears the shared [`TrajectoryBuffer`];
//! 2. asks the caller's `env_factory` to build a fresh [`Simulation`] wired with
//!    a collect-mode [`PolicyMechanism`](crate::PolicyMechanism) that shares the
//!    trainer's policy and buffer;
//! 3. steps the simulation, attributing each acting agent's reward via a
//!    [`RewardFn`] right after the step resolves;
//! 4. updates the policy from the episode's trajectories.
//!
//! The factory receives a per-episode seed so episodes differ yet stay fully
//! reproducible: same trainer seed ⇒ identical [`EpisodeStat`] sequence.

use std::cell::RefCell;
use std::rc::Rc;

use socsim_core::{Result, WorldState};
use socsim_engine::Simulation;

use crate::buffer::TrajectoryBuffer;
use crate::policy::{Policy, RewardFn};

/// Training schedule.
#[derive(Clone, Copy, Debug)]
pub struct TrainConfig {
    /// Number of episodes (full simulation runs) to train over.
    pub episodes: usize,
    /// Base seed; episode `e` uses `seed + e` for its environment.
    pub seed: u64,
}

/// Per-episode training summary.
#[derive(Clone, Copy, Debug)]
pub struct EpisodeStat {
    /// Zero-based episode index.
    pub episode: usize,
    /// Sum of all per-agent rewards collected during the episode.
    pub total_reward: f32,
    /// Scalar policy-gradient loss from the update.
    pub loss: f32,
}

/// Drives episode rollouts and policy updates around a shared [`Policy`].
pub struct MarlTrainer<P: Policy> {
    policy: Rc<RefCell<P>>,
    buffer: Rc<RefCell<TrajectoryBuffer>>,
}

impl<P: Policy> MarlTrainer<P> {
    /// Create a trainer that owns shared handles to `policy` and a fresh buffer.
    pub fn new(policy: Rc<RefCell<P>>) -> Self {
        Self {
            policy,
            buffer: Rc::new(RefCell::new(TrajectoryBuffer::new())),
        }
    }

    /// Shared handle to the policy (e.g. to build an inference mechanism later).
    pub fn policy(&self) -> Rc<RefCell<P>> {
        self.policy.clone()
    }

    /// Run the training loop.
    ///
    /// `env_factory(policy, buffer, seed)` must build a [`Simulation`] that
    /// includes a collect-mode
    /// [`PolicyMechanism`](crate::PolicyMechanism::collecting) sharing the given
    /// `policy` and `buffer`.  `reward` attributes per-agent reward after each
    /// step.
    pub fn train<W, F>(
        &mut self,
        cfg: &TrainConfig,
        mut env_factory: F,
        reward: &dyn RewardFn<W>,
    ) -> Result<Vec<EpisodeStat>>
    where
        W: WorldState,
        F: FnMut(Rc<RefCell<P>>, Rc<RefCell<TrajectoryBuffer>>, u64) -> Simulation<W>,
    {
        let mut stats = Vec::with_capacity(cfg.episodes);

        for episode in 0..cfg.episodes {
            self.buffer.borrow_mut().clear();
            let seed = cfg.seed.wrapping_add(episode as u64);
            let mut sim = env_factory(self.policy.clone(), self.buffer.clone(), seed);

            let mut total_reward = 0.0f32;
            while !sim.world().clock().is_done() && !sim.stop_requested() {
                sim.step()?;
                // Reward every agent that acted this step, then clear pending.
                let acted = self.buffer.borrow().pending_agents();
                for aid in acted {
                    let r = reward.reward(sim.world(), aid);
                    total_reward += r;
                    self.buffer.borrow_mut().complete(aid, r);
                }
            }

            let trajectories = self.buffer.borrow_mut().take_episode();
            let loss = self.policy.borrow_mut().update(&trajectories)?;
            stats.push(EpisodeStat {
                episode,
                total_reward,
                loss,
            });
        }

        Ok(stats)
    }
}
