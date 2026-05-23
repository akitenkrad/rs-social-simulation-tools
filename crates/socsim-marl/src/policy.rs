//! Policy abstraction and the world-to-features bridge.
//!
//! A [`Policy`] operates on **fixed-length feature vectors** (`&[f32]`) and
//! emits **discrete action indices** (`usize`).  The mapping between a concrete
//! [`WorldState`](socsim_core::WorldState) and those flat features is the job of
//! an [`ObsEncoder`]; turning an action index back into a world mutation is the
//! job of an [`ActionApplier`].  This keeps the learning core agnostic of any
//! particular research module while letting modules plug in domain semantics.

use socsim_core::{AgentId, Result, SimRng, WorldState};

/// One recorded decision step for a single agent: the features it observed, the
/// action it chose, and the reward it subsequently received.
#[derive(Clone, Debug)]
pub struct Transition {
    /// Encoded observation features at decision time.
    pub obs: Vec<f32>,
    /// Discrete action index that was taken.
    pub action: usize,
    /// Reward attributed to this `(obs, action)` after the step resolved.
    pub reward: f32,
}

/// A learnable discrete-action policy over fixed-length feature vectors.
///
/// The analogue of an RL agent's policy network.  Implemented by
/// [`DiscretePolicyNet`](crate::DiscretePolicyNet); other approximators (linear,
/// tabular) can implement the same trait.
pub trait Policy {
    /// Greedy (arg-max) action for inference.  Deterministic, consumes no RNG —
    /// a frozen policy used this way keeps the simulation bit-reproducible.
    fn act(&self, obs: &[f32]) -> usize;

    /// Stochastic action sampled from the policy's categorical distribution,
    /// drawing from `rng`.  Used while collecting training trajectories.
    fn sample(&self, obs: &[f32], rng: &mut SimRng) -> usize;

    /// Update parameters from collected episodes.  `episodes` holds one
    /// trajectory (a `Vec<Transition>`) per agent.  Returns the scalar training
    /// loss for logging.
    fn update(&mut self, episodes: &[Vec<Transition>]) -> Result<f32>;

    /// Feature-vector length this policy expects.
    fn obs_dim(&self) -> usize;

    /// Number of discrete actions this policy chooses among.
    fn n_actions(&self) -> usize;
}

/// Encodes a world + agent into the flat feature vector a [`Policy`] consumes.
///
/// Returning `None` signals that the agent is not actionable this step (e.g. it
/// was removed by an earlier mechanism), so the
/// [`PolicyMechanism`](crate::PolicyMechanism) skips it.
pub trait ObsEncoder<W: WorldState> {
    /// Length of the feature vector produced by [`ObsEncoder::encode`].
    fn obs_dim(&self) -> usize;

    /// Build the feature vector for `agent`, or `None` if it should be skipped.
    fn encode(&self, world: &W, agent: AgentId) -> Option<Vec<f32>>;
}

/// Applies a chosen action index back onto the world for one agent.
pub trait ActionApplier<W: WorldState> {
    /// Number of discrete actions, matching the policy's output dimension.
    fn n_actions(&self) -> usize;

    /// Apply `action` for `agent`.  Called inside the `Decision` phase in
    /// scheduler order with mutable world access; `rng` is the simulation RNG
    /// for any stochastic side effects.
    fn apply(&self, world: &mut W, agent: AgentId, action: usize, rng: &mut SimRng);
}

/// Computes the per-agent reward for the step that just completed.
///
/// Read **after** `step()` so the reward reflects the resolved world (the RL
/// convention `r_t` follows the transition).  Implementations must tolerate an
/// `agent` that no longer exists in `world` (e.g. it quit this step) and return
/// a terminal reward in that case.
pub trait RewardFn<W: WorldState> {
    /// Reward attributed to `agent` for the step that just finished.
    fn reward(&self, world: &W, agent: AgentId) -> f32;
}
