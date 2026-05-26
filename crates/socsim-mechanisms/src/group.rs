//! Group-dynamics mechanisms.
//!
//! This module ships the **group-conformity** family, operating over any world
//! implementing [`GroupMembership`](socsim_core::GroupMembership) +
//! [`ScalarOpinions`](socsim_core::ScalarOpinions):
//!
//! - [`GroupConformityMechanism`] — a *synchronous* DeGroot-style update where
//!   every agent moves a fraction α of the way toward the **mean opinion of its
//!   own group**.  The within-group averaging is the consensus mechanism of
//!   DeGroot (1974, "Reaching a Consensus"), restricted to each group's members
//!   as the influence set — i.e. group conformity.

use socsim_core::{
    GroupId, GroupMembership, Mechanism, Phase, Result, ScalarOpinions, StepContext,
};

use std::collections::HashMap;

// ── GroupConformityMechanism ─────────────────────────────────────────────────

/// Group-conformity opinion update (synchronous, DeGroot-style within-group
/// averaging).
///
/// On each step this mechanism, for every agent `i`:
/// 1. takes a snapshot of all agents' opinions (so the update is synchronous);
/// 2. computes, from the snapshot, the **mean opinion of `i`'s group**
///    `g = group_of(i)` over all members `group_members(g)`;
/// 3. nudges `i` a fraction α of the way toward that group mean:
///
/// ```text
/// x_i ← x_i + α · (group_mean_i − x_i)
/// ```
///
/// 4. batch-writes the new opinions.
///
/// Because every new opinion is computed from the *same* start-of-step snapshot
/// (the group means are all computed first, then the opinions are written), the
/// result is independent of agent activation order — the synchronous
/// (simultaneous) DeGroot consensus update restricted to within-group influence.
/// With `α = 1` each agent jumps straight to its group mean in one step; with a
/// small α the groups relax toward their (conserved) means gradually.
///
/// The conformity rate `α` is clamped to `[0, 1]` at construction.
#[derive(Clone, Copy, Debug)]
pub struct GroupConformityMechanism {
    /// Conformity rate α ∈ [0, 1]: the fraction of the gap to the group mean
    /// each agent closes per step.
    pub alpha: f64,
}

impl GroupConformityMechanism {
    /// Create a group-conformity mechanism with conformity rate `alpha`.
    ///
    /// `alpha` is clamped to `[0, 1]`: `0` freezes opinions, `1` snaps every
    /// agent onto its group mean each step.
    pub fn new(alpha: f64) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
        }
    }
}

impl Default for GroupConformityMechanism {
    /// α = 0.3 — a moderate per-step pull toward the group mean.
    fn default() -> Self {
        Self { alpha: 0.3 }
    }
}

impl<W: GroupMembership + ScalarOpinions> Mechanism<W> for GroupConformityMechanism {
    fn name(&self) -> &str {
        "group_conformity"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let ids = ctx.world.agent_ids();

        // Snapshot every agent's opinion at the start of the step (the canonical
        // copy for the synchronous update).
        let prev: Vec<f64> = ids.iter().map(|&id| ctx.world.opinion(id)).collect();

        // Compute the mean opinion of every group from the snapshot first, so the
        // write-back order does not matter.  Iterate groups in sorted id order so
        // the floating-point summation order is deterministic.
        let mut group_ids = ctx.world.groups();
        group_ids.sort_unstable();
        group_ids.dedup();

        let mut group_mean: HashMap<GroupId, f64> = HashMap::with_capacity(group_ids.len());
        for &g in &group_ids {
            let mut members = ctx.world.group_members(g);
            members.sort_unstable();
            members.dedup();
            if members.is_empty() {
                continue;
            }
            let sum: f64 = members.iter().map(|&m| ctx.world.opinion(m)).sum();
            group_mean.insert(g, sum / members.len() as f64);
        }

        // Compute each agent's new opinion against its group's snapshot mean.
        let mut new_opinions: Vec<f64> = Vec::with_capacity(ids.len());
        for (idx, &id) in ids.iter().enumerate() {
            let xi = prev[idx];
            let g = ctx.world.group_of(id);
            // An agent whose group has a mean (the normal case) moves toward it;
            // a group with no enumerable members leaves the agent unchanged.
            let xi_new = match group_mean.get(&g) {
                Some(&mean) => xi + self.alpha * (mean - xi),
                None => xi,
            };
            new_opinions.push(xi_new);
        }

        // Batch write-back (synchronous update).
        for (idx, &id) in ids.iter().enumerate() {
            ctx.world.set_opinion(id, new_opinions[idx]);
        }

        Ok(())
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use socsim_core::{AgentId, Blackboard, NullRecorder, SimClock, SimRng, WorldState};

    /// A world with one scalar opinion per agent and a fixed group partition.
    struct GroupWorld {
        clock: SimClock,
        opinions: Vec<f64>,
        /// `group[i]` = the group id of agent `i`.
        group: Vec<GroupId>,
    }

    impl GroupWorld {
        fn new(opinions: Vec<f64>, group: Vec<GroupId>) -> Self {
            assert_eq!(opinions.len(), group.len());
            Self {
                clock: SimClock::new(10_000),
                opinions,
                group,
            }
        }
    }

    impl WorldState for GroupWorld {
        fn agent_ids(&self) -> Vec<AgentId> {
            (0..self.opinions.len() as u64).map(AgentId).collect()
        }
        fn clock(&self) -> &SimClock {
            &self.clock
        }
        fn clock_mut(&mut self) -> &mut SimClock {
            &mut self.clock
        }
    }
    impl ScalarOpinions for GroupWorld {
        fn opinion(&self, id: AgentId) -> f64 {
            self.opinions[id.0 as usize]
        }
        fn set_opinion(&mut self, id: AgentId, value: f64) {
            self.opinions[id.0 as usize] = value;
        }
    }
    impl GroupMembership for GroupWorld {
        fn group_of(&self, id: AgentId) -> GroupId {
            self.group[id.0 as usize]
        }
        fn group_members(&self, g: GroupId) -> Vec<AgentId> {
            self.group
                .iter()
                .enumerate()
                .filter(|&(_, &gg)| gg == g)
                .map(|(i, _)| AgentId(i as u64))
                .collect()
        }
        fn groups(&self) -> Vec<GroupId> {
            let mut gs = self.group.clone();
            gs.sort_unstable();
            gs.dedup();
            gs
        }
    }

    /// Run a mechanism for `steps` Interaction steps against `world`.
    fn run<M, W>(mech: &mut M, world: &mut W, rng: &mut SimRng, steps: usize)
    where
        M: Mechanism<W>,
        W: WorldState,
    {
        let order = world.agent_ids();
        for _ in 0..steps {
            let mut scratch = Blackboard::new();
            let mut stop = false;
            let mut rec = NullRecorder;
            let clock = *world.clock();
            let mut ctx = StepContext {
                world,
                clock,
                rng,
                recorder: &mut rec,
                agent_order: &order,
                scratch: &mut scratch,
                stop: &mut stop,
            };
            mech.apply(Phase::Interaction, &mut ctx).unwrap();
        }
    }

    fn group_mean(opinions: &[f64], group: &[GroupId], g: GroupId) -> f64 {
        let members: Vec<f64> = opinions
            .iter()
            .zip(group.iter())
            .filter(|&(_, &gg)| gg == g)
            .map(|(&x, _)| x)
            .collect();
        members.iter().sum::<f64>() / members.len() as f64
    }

    fn spread_within(opinions: &[f64], group: &[GroupId], g: GroupId) -> f64 {
        let xs: Vec<f64> = opinions
            .iter()
            .zip(group.iter())
            .filter(|&(_, &gg)| gg == g)
            .map(|(&x, _)| x)
            .collect();
        let lo = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        hi - lo
    }

    #[test]
    fn alpha_is_clamped_to_unit_interval() {
        assert_eq!(GroupConformityMechanism::new(2.0).alpha, 1.0);
        assert_eq!(GroupConformityMechanism::new(-0.5).alpha, 0.0);
        assert!((GroupConformityMechanism::new(0.3).alpha - 0.3).abs() < 1e-12);
    }

    #[test]
    fn within_group_converges_and_conserves_mean() {
        // One group of five distinct opinions; α moves everyone toward the mean.
        let opinions = vec![0.0, 0.25, 0.5, 0.75, 1.0];
        let group = vec![0u64; 5];
        let mean0 = group_mean(&opinions, &group, 0);
        let spread0 = spread_within(&opinions, &group, 0);
        let mut world = GroupWorld::new(opinions, group.clone());
        let mut rng = SimRng::from_seed(1);
        let mut m = GroupConformityMechanism::new(0.3);
        run(&mut m, &mut world, &mut rng, 200);

        // Group mean is preserved exactly by the averaging step (conservation).
        let mean1 = group_mean(&world.opinions, &group, 0);
        assert!(
            (mean0 - mean1).abs() < 1e-9,
            "group mean not conserved: {mean0} → {mean1}"
        );
        // Opinions converge toward the mean: spread shrinks toward zero.
        let spread1 = spread_within(&world.opinions, &group, 0);
        assert!(spread1 < spread0, "spread did not shrink");
        assert!(spread1 < 1e-6, "expected within-group consensus, spread = {spread1}");
        assert!(world.opinions.iter().all(|&x| (x - mean0).abs() < 1e-6));
    }

    #[test]
    fn alpha_one_snaps_to_group_mean_in_one_step() {
        let opinions = vec![0.0, 1.0, 0.2, 0.8];
        let group = vec![0u64, 0, 1, 1];
        let mean_a = group_mean(&opinions, &group, 0);
        let mean_b = group_mean(&opinions, &group, 1);
        let mut world = GroupWorld::new(opinions, group);
        let mut rng = SimRng::from_seed(0);
        let mut m = GroupConformityMechanism::new(1.0);
        run(&mut m, &mut world, &mut rng, 1);
        assert!((world.opinions[0] - mean_a).abs() < 1e-12);
        assert!((world.opinions[1] - mean_a).abs() < 1e-12);
        assert!((world.opinions[2] - mean_b).abs() < 1e-12);
        assert!((world.opinions[3] - mean_b).abs() < 1e-12);
    }

    #[test]
    fn two_disjoint_groups_converge_independently() {
        // Group 0 around 0.1, group 1 around 0.9; cross-group must not mix.
        let opinions = vec![0.0, 0.2, 0.1, 0.8, 1.0, 0.9];
        let group = vec![0u64, 0, 0, 1, 1, 1];
        let mean_a = group_mean(&opinions, &group, 0);
        let mean_b = group_mean(&opinions, &group, 1);
        let mut world = GroupWorld::new(opinions, group.clone());
        let mut rng = SimRng::from_seed(2);
        let mut m = GroupConformityMechanism::new(0.5);
        run(&mut m, &mut world, &mut rng, 500);

        // Each group converges to its own (unchanged) mean — no cross influence.
        assert!((group_mean(&world.opinions, &group, 0) - mean_a).abs() < 1e-9);
        assert!((group_mean(&world.opinions, &group, 1) - mean_b).abs() < 1e-9);
        for (i, &g) in group.iter().enumerate() {
            let target = if g == 0 { mean_a } else { mean_b };
            assert!(
                (world.opinions[i] - target).abs() < 1e-6,
                "agent {i} (group {g}) did not converge to its group mean"
            );
        }
        // The two group means stay distinct (groups are isolated).
        assert!((mean_a - mean_b).abs() > 0.5);
    }

    #[test]
    fn is_deterministic_given_same_initial_state() {
        let opinions: Vec<f64> = (0..12).map(|i| i as f64 / 11.0).collect();
        let group: Vec<GroupId> = (0..12).map(|i| (i % 3) as u64).collect();
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut world = GroupWorld::new(opinions.clone(), group.clone());
            let mut rng = SimRng::from_seed(42);
            let mut m = GroupConformityMechanism::new(0.4);
            run(&mut m, &mut world, &mut rng, 80);
            runs.push(world.opinions);
        }
        assert_eq!(runs[0], runs[1]);
    }
}
