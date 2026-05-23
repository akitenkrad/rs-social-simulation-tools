//! Per-agent trajectory collection during an episode.
//!
//! The [`PolicyMechanism`](crate::PolicyMechanism) records each agent's
//! `(obs, action)` as it acts; the [`MarlTrainer`](crate::MarlTrainer) fills in
//! the reward after the step resolves.  At episode end the buffer hands back one
//! [`Transition`] vector per agent, ordered by [`AgentId`] for determinism.

use std::collections::BTreeMap;

use socsim_core::AgentId;

use crate::policy::Transition;

/// Collects per-agent trajectories for one training episode.
#[derive(Default)]
pub struct TrajectoryBuffer {
    /// Decisions awaiting their reward, keyed by agent.
    pending: BTreeMap<AgentId, (Vec<f32>, usize)>,
    /// Completed transitions for the current episode, keyed by agent.
    episodes: BTreeMap<AgentId, Vec<Transition>>,
}

impl TrajectoryBuffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `agent` observed `obs` and chose `action`, pending its reward.
    pub fn begin_decision(&mut self, agent: AgentId, obs: Vec<f32>, action: usize) {
        self.pending.insert(agent, (obs, action));
    }

    /// Attach `reward` to `agent`'s pending decision, finalising the transition.
    ///
    /// No-op if the agent has no pending decision (it did not act this step).
    pub fn complete(&mut self, agent: AgentId, reward: f32) {
        if let Some((obs, action)) = self.pending.remove(&agent) {
            self.episodes.entry(agent).or_default().push(Transition {
                obs,
                action,
                reward,
            });
        }
    }

    /// Agents with a decision still awaiting its reward, in [`AgentId`] order.
    pub fn pending_agents(&self) -> Vec<AgentId> {
        self.pending.keys().copied().collect()
    }

    /// Drain the episode into one trajectory per agent (ordered by [`AgentId`]),
    /// clearing all state for the next episode.  Any uncompleted pending
    /// decisions are discarded.
    pub fn take_episode(&mut self) -> Vec<Vec<Transition>> {
        self.pending.clear();
        std::mem::take(&mut self.episodes)
            .into_values()
            .collect()
    }

    /// Forget all pending and completed state.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.episodes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_pairs_decision_with_reward() {
        let mut buf = TrajectoryBuffer::new();
        buf.begin_decision(AgentId(0), vec![1.0], 1);
        assert_eq!(buf.pending_agents(), vec![AgentId(0)]);
        buf.complete(AgentId(0), 2.5);
        assert!(buf.pending_agents().is_empty());
        let eps = buf.take_episode();
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0][0].action, 1);
        assert_eq!(eps[0][0].reward, 2.5);
    }

    #[test]
    fn complete_without_pending_is_noop() {
        let mut buf = TrajectoryBuffer::new();
        buf.complete(AgentId(9), 1.0); // never decided
        assert!(buf.take_episode().is_empty());
    }
}
