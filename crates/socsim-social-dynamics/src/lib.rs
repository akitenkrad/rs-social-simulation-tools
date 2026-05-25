//! General opinion-dynamics mechanisms for `socsim`.
//!
//! This is `socsim`'s first **general** (non-HR, domain-agnostic) mechanism
//! pack.  Where crates such as `socsim-hr-lifecycle` model one specific
//! scenario, this crate provides reusable *building blocks* that operate over
//! any world implementing the capability traits
//! [`ScalarOpinions`](socsim_core::ScalarOpinions) and
//! [`OpinionNeighbors`](socsim_core::OpinionNeighbors) from `socsim-core`.
//!
//! The first family it ships is **bounded confidence (BC)** opinion dynamics —
//! agents are only influenced by others whose opinion lies within a tolerance
//! ε of their own:
//!
//! - [`HegselmannKrauseMechanism`] — Hegselmann–Krause (2002, generalised
//!   2005): a *synchronous* update where every agent moves to the (chosen)
//!   mean of all opinions within ε of its own.  Math ported verbatim from the
//!   `hegselmann2005` replication.
//! - [`DeffuantMechanism`] — Deffuant et al. (2000): a *pairwise / event-based*
//!   update where, on each interaction, two agents within ε move toward each
//!   other by a rate μ.  Math ported verbatim from the `mou2024` replication.
//!
//! Convergence is decided by the **driver / world**, matching the
//! `hegselmann2005` reference; this crate offers the free helper
//! [`max_abs_delta`] and an optional [`ConvergenceMechanism`] (a `PostStep`
//! mechanism that calls `request_stop` once `max|Δx| < tol`) for convenience.
//!
//! # Usage (library mode)
//! ```ignore
//! use socsim_social_dynamics::{HegselmannKrauseMechanism, MeanOperator};
//! let hk = HegselmannKrauseMechanism::new(0.2, MeanOperator::Arithmetic);
//! // register `hk` with the engine for a world: ScalarOpinions + OpinionNeighbors
//! ```

pub mod means;

pub use means::{apply_mean, parse_mean, MeanOperator};

use socsim_core::{
    AgentId, Mechanism, OpinionNeighbors, Phase, Result, ScalarOpinions, StepContext,
};

use rand::Rng;

// ── HegselmannKrauseMechanism ───────────────────────────────────────────────

/// Hegselmann–Krause bounded-confidence update (synchronous).
///
/// On each step this mechanism, for every agent `i`:
/// 1. takes a snapshot of all agents' opinions (so the update is synchronous);
/// 2. collects, from `opinion_neighbors(i)` ∪ `{i}`, those opinions `x_j` with
///    `|x_i − x_j| ≤ ε` (the confidence set `I(i)`);
/// 3. aggregates them with the configured [`MeanOperator`] via
///    [`apply_mean`](means::apply_mean);
/// 4. batch-writes the new opinions.
///
/// Because every new opinion is computed from the *same* start-of-step
/// snapshot, the result is independent of agent activation order — exactly the
/// synchronous (simultaneous) update of the `hegselmann2005` reference.
#[derive(Clone, Copy, Debug)]
pub struct HegselmannKrauseMechanism {
    /// Symmetric confidence bound ε.
    pub epsilon: f64,
    /// Averaging operator applied to the confidence set.
    pub mean: MeanOperator,
}

impl HegselmannKrauseMechanism {
    /// Create a Hegselmann–Krause mechanism with confidence bound `epsilon` and
    /// the given averaging operator.
    pub fn new(epsilon: f64, mean: MeanOperator) -> Self {
        Self { epsilon, mean }
    }
}

impl Default for HegselmannKrauseMechanism {
    /// ε = 0.2 with the arithmetic mean — the canonical HK setting.
    fn default() -> Self {
        Self {
            epsilon: 0.2,
            mean: MeanOperator::Arithmetic,
        }
    }
}

impl<W: ScalarOpinions + OpinionNeighbors> Mechanism<W> for HegselmannKrauseMechanism {
    fn name(&self) -> &str {
        "hegselmann_krause"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let ids = ctx.world.agent_ids();

        // Snapshot every agent's opinion at the start of the step (the canonical
        // copy for the synchronous update).
        let prev: Vec<f64> = ids.iter().map(|&id| ctx.world.opinion(id)).collect();

        // Reusable buffers to avoid per-agent heap churn.
        let mut conf_set: Vec<f64> = Vec::with_capacity(ids.len());
        let mut new_opinions: Vec<f64> = Vec::with_capacity(ids.len());

        for (idx, &id) in ids.iter().enumerate() {
            let xi = prev[idx];
            conf_set.clear();
            // Self is always in its own confidence set.
            conf_set.push(xi);
            // Neighbours within ε of x_i, read from the snapshot.
            for nb in ctx.world.opinion_neighbors(id) {
                if nb == id {
                    continue; // self already added; avoid double-counting.
                }
                let xj = ctx.world.opinion(nb);
                if (xi - xj).abs() <= self.epsilon {
                    conf_set.push(xj);
                }
            }
            let xi_new = apply_mean(self.mean, &conf_set, ctx.rng);
            new_opinions.push(xi_new);
        }

        // Batch write-back (synchronous update).
        for (idx, &id) in ids.iter().enumerate() {
            ctx.world.set_opinion(id, new_opinions[idx]);
        }

        Ok(())
    }
}

// ── DeffuantMechanism ───────────────────────────────────────────────────────

/// Deffuant bounded-confidence update (pairwise / event-based).
///
/// `pairs_per_step` times per step, the mechanism draws an agent `i` (from
/// `ctx.agent_order`, or the world's agent roster as a fallback) and a random
/// neighbour `j` from `opinion_neighbors(i)`.  If `|x_i − x_j| ≤ ε`, both move
/// toward each other by the convergence rate μ:
///
/// ```text
/// x_i += μ · (x_j − x_i)
/// x_j += μ · (x_i − x_j)
/// ```
///
/// (Using the *pre-update* `x_i`/`x_j` for both deltas, so the pair contracts
/// symmetrically.)  This is the `bc` update of the `mou2024` reference applied
/// pairwise.  All randomness flows through `ctx.rng`, so a fixed seed yields a
/// deterministic trajectory.
#[derive(Clone, Copy, Debug)]
pub struct DeffuantMechanism {
    /// Symmetric confidence bound ε.
    pub epsilon: f64,
    /// Convergence rate μ ∈ (0, 0.5]; each agent moves a fraction μ of the gap.
    pub mu: f64,
    /// Number of pairwise interactions per simulation step.
    pub pairs_per_step: usize,
}

impl DeffuantMechanism {
    /// Create a Deffuant mechanism with confidence bound `epsilon`, convergence
    /// rate `mu`, and `pairs_per_step` pairwise interactions per step.
    pub fn new(epsilon: f64, mu: f64, pairs_per_step: usize) -> Self {
        Self {
            epsilon,
            mu,
            pairs_per_step,
        }
    }
}

impl Default for DeffuantMechanism {
    /// ε = 0.2, μ = 0.5, one interaction per step.
    fn default() -> Self {
        Self {
            epsilon: 0.2,
            mu: 0.5,
            pairs_per_step: 1,
        }
    }
}

impl<W: ScalarOpinions + OpinionNeighbors> Mechanism<W> for DeffuantMechanism {
    fn name(&self) -> &str {
        "deffuant"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        // Pool of agents to draw `i` from: the scheduler's order if non-empty,
        // else the world roster.
        let pool: Vec<AgentId> = if ctx.agent_order.is_empty() {
            ctx.world.agent_ids()
        } else {
            ctx.agent_order.to_vec()
        };
        if pool.is_empty() {
            return Ok(());
        }

        for _ in 0..self.pairs_per_step {
            let i = pool[ctx.rng.gen_range(0..pool.len())];
            let neighbors = ctx.world.opinion_neighbors(i);
            // Candidate partners exclude self.
            let candidates: Vec<AgentId> = neighbors.into_iter().filter(|&j| j != i).collect();
            if candidates.is_empty() {
                continue;
            }
            let j = candidates[ctx.rng.gen_range(0..candidates.len())];

            let xi = ctx.world.opinion(i);
            let xj = ctx.world.opinion(j);
            if (xi - xj).abs() <= self.epsilon {
                // Symmetric contraction using the pre-update values.
                ctx.world.set_opinion(i, xi + self.mu * (xj - xi));
                ctx.world.set_opinion(j, xj + self.mu * (xi - xj));
            }
        }

        Ok(())
    }
}

// ── Convergence helpers ─────────────────────────────────────────────────────

/// Maximum absolute element-wise difference between two opinion snapshots.
///
/// Returns `+∞` if the slices differ in length (treated as "not converged").
/// A driver can stop the run once `max_abs_delta(prev, curr) < tol`.
pub fn max_abs_delta(prev: &[f64], curr: &[f64]) -> f64 {
    if prev.len() != curr.len() {
        return f64::INFINITY;
    }
    prev.iter()
        .zip(curr.iter())
        .map(|(&a, &b)| (a - b).abs())
        .fold(0.0_f64, f64::max)
}

/// Optional `PostStep` mechanism that stops the run once opinions stop moving.
///
/// It keeps the previous step's opinion snapshot internally; each `PostStep` it
/// compares the current opinions to that snapshot and, if
/// `max|Δx| < tol`, calls [`StepContext::request_stop`].  This mirrors the
/// `hegselmann2005` convergence idiom while leaving the stop decision in a
/// (generic, driver-side) mechanism rather than baked into the world.
///
/// Note: meaningful only for deterministic updates (e.g. HK with A/G/H/P).  A
/// stochastic update (the random mean, or Deffuant) need not reach a fixed
/// point, so pairing this with such a mechanism may stop early or never.
#[derive(Clone, Debug, Default)]
pub struct ConvergenceMechanism {
    /// Convergence tolerance: stop when `max|Δx| < tol`.
    pub tol: f64,
    /// Previous-step opinion snapshot (`None` until the first `PostStep`).
    prev: Option<Vec<f64>>,
}

impl ConvergenceMechanism {
    /// Create a convergence checker with tolerance `tol`.
    pub fn new(tol: f64) -> Self {
        Self { tol, prev: None }
    }
}

impl<W: ScalarOpinions> Mechanism<W> for ConvergenceMechanism {
    fn name(&self) -> &str {
        "convergence"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let curr: Vec<f64> = ctx
            .world
            .agent_ids()
            .iter()
            .map(|&id| ctx.world.opinion(id))
            .collect();

        if let Some(prev) = &self.prev {
            if max_abs_delta(prev, &curr) < self.tol {
                ctx.request_stop();
            }
        }
        self.prev = Some(curr);
        Ok(())
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use socsim_core::{
        AgentId, Blackboard, NullRecorder, SimClock, SimRng, StepContext, WorldState,
    };

    /// Tiny fixture world: a `Vec<f64>` of opinions plus a neighbour rule.
    struct TestWorld {
        clock: SimClock,
        opinions: Vec<f64>,
        topology: Topology,
    }

    #[derive(Clone)]
    enum Topology {
        /// Every agent sees every *other* agent (non-spatial complete graph).
        Complete,
        /// Explicit per-agent adjacency (e.g. a ring).
        Adjacency(Vec<Vec<usize>>),
    }

    impl TestWorld {
        fn complete(opinions: Vec<f64>) -> Self {
            Self {
                clock: SimClock::new(10_000),
                opinions,
                topology: Topology::Complete,
            }
        }

        fn ring(opinions: Vec<f64>) -> Self {
            let n = opinions.len();
            let adj: Vec<Vec<usize>> = (0..n).map(|i| vec![(i + n - 1) % n, (i + 1) % n]).collect();
            Self {
                clock: SimClock::new(10_000),
                opinions,
                topology: Topology::Adjacency(adj),
            }
        }
    }

    impl WorldState for TestWorld {
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

    impl ScalarOpinions for TestWorld {
        fn opinion(&self, id: AgentId) -> f64 {
            self.opinions[id.0 as usize]
        }
        fn set_opinion(&mut self, id: AgentId, value: f64) {
            self.opinions[id.0 as usize] = value;
        }
    }

    impl OpinionNeighbors for TestWorld {
        fn opinion_neighbors(&self, id: AgentId) -> Vec<AgentId> {
            match &self.topology {
                Topology::Complete => self.agent_ids().into_iter().filter(|&j| j != id).collect(),
                Topology::Adjacency(adj) => adj[id.0 as usize]
                    .iter()
                    .map(|&j| AgentId(j as u64))
                    .collect(),
            }
        }
    }

    /// Run a mechanism for `steps` steps against `world` with a fresh RNG.
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

    fn num_clusters(opinions: &[f64], tol: f64) -> usize {
        let mut sorted = opinions.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut clusters = 0;
        let mut last = f64::NEG_INFINITY;
        for &x in &sorted {
            if (x - last).abs() > tol {
                clusters += 1;
            }
            last = x;
        }
        clusters
    }

    fn spread(opinions: &[f64]) -> f64 {
        let lo = opinions.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = opinions.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        hi - lo
    }

    // ── MeanOperator correctness ─────────────────────────────────────────────

    #[test]
    fn mean_operator_values() {
        let mut r = SimRng::from_seed(0);
        let v = [0.2, 0.5, 0.9];
        let a = apply_mean(MeanOperator::Arithmetic, &v, &mut r);
        let g = apply_mean(MeanOperator::Geometric, &v, &mut r);
        let h = apply_mean(MeanOperator::Harmonic, &v, &mut r);
        assert!((a - (0.2 + 0.5 + 0.9) / 3.0).abs() < 1e-12);
        // A ≥ G ≥ H for positive inputs.
        assert!(a >= g - 1e-12);
        assert!(g >= h - 1e-12);
        // P_1 == A.
        let p1 = apply_mean(MeanOperator::Power(1.0), &v, &mut r);
        assert!((p1 - a).abs() < 1e-12);
        // R within [min, max].
        for _ in 0..200 {
            let x = apply_mean(MeanOperator::Random, &v, &mut r);
            assert!((0.2..=0.9).contains(&x));
        }
    }

    // ── Hegselmann–Krause ────────────────────────────────────────────────────

    #[test]
    fn hk_large_epsilon_reaches_consensus() {
        // Opinions spread over [0,1]; ε ≥ range → everyone in everyone's
        // confidence set → arithmetic mean converges to consensus.
        let opinions: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let mut world = TestWorld::complete(opinions);
        let mut rng = SimRng::from_seed(7);
        let mut hk = HegselmannKrauseMechanism::new(1.0, MeanOperator::Arithmetic);
        run(&mut hk, &mut world, &mut rng, 100);
        assert!(
            spread(&world.opinions) < 1e-9,
            "expected consensus, spread = {}",
            spread(&world.opinions)
        );
    }

    #[test]
    fn hk_small_epsilon_fragments() {
        // Small ε → opinions far apart never merge → multiple surviving clusters.
        let opinions: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let mut world = TestWorld::complete(opinions);
        let mut rng = SimRng::from_seed(7);
        let mut hk = HegselmannKrauseMechanism::new(0.05, MeanOperator::Arithmetic);
        run(&mut hk, &mut world, &mut rng, 200);
        let clusters = num_clusters(&world.opinions, 1e-3);
        assert!(
            clusters > 1,
            "expected fragmentation, got {} cluster(s)",
            clusters
        );
    }

    #[test]
    fn hk_is_deterministic_given_seed() {
        let opinions: Vec<f64> = (0..15).map(|i| i as f64 / 14.0).collect();
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut world = TestWorld::complete(opinions.clone());
            let mut rng = SimRng::from_seed(42);
            let mut hk = HegselmannKrauseMechanism::new(0.15, MeanOperator::Arithmetic);
            run(&mut hk, &mut world, &mut rng, 50);
            runs.push(world.opinions);
        }
        assert_eq!(runs[0], runs[1]);
    }

    #[test]
    fn hk_respects_ring_topology() {
        // On a ring with small ε the global spread cannot collapse to a single
        // point in a way that depends on non-neighbours; sanity: it runs and
        // stays within [0,1].
        let opinions: Vec<f64> = (0..10).map(|i| i as f64 / 9.0).collect();
        let mut world = TestWorld::ring(opinions);
        let mut rng = SimRng::from_seed(1);
        let mut hk = HegselmannKrauseMechanism::new(0.2, MeanOperator::Arithmetic);
        run(&mut hk, &mut world, &mut rng, 50);
        assert!(world.opinions.iter().all(|&x| (0.0..=1.0).contains(&x)));
    }

    // ── Deffuant ─────────────────────────────────────────────────────────────

    #[test]
    fn deffuant_large_epsilon_reaches_consensus() {
        let opinions: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let mut world = TestWorld::complete(opinions);
        let mut rng = SimRng::from_seed(3);
        // ε large enough that any pair interacts; μ=0.5 contracts pairs.
        let mut d = DeffuantMechanism::new(1.0, 0.5, 200);
        run(&mut d, &mut world, &mut rng, 500);
        assert!(
            spread(&world.opinions) < 1e-6,
            "expected consensus, spread = {}",
            spread(&world.opinions)
        );
    }

    #[test]
    fn deffuant_small_epsilon_fragments() {
        let opinions: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let mut world = TestWorld::complete(opinions);
        let mut rng = SimRng::from_seed(3);
        let mut d = DeffuantMechanism::new(0.05, 0.5, 200);
        run(&mut d, &mut world, &mut rng, 500);
        let clusters = num_clusters(&world.opinions, 1e-3);
        assert!(
            clusters > 1,
            "expected fragmentation, got {} cluster(s)",
            clusters
        );
    }

    #[test]
    fn deffuant_contracts_and_conserves_mean_when_all_interact() {
        // With ε large, every pair interacts and μ=0.5 → the average opinion is
        // conserved (symmetric exchange) and total spread is non-increasing.
        let opinions: Vec<f64> = (0..11).map(|i| i as f64 / 10.0).collect();
        let mean0: f64 = opinions.iter().sum::<f64>() / opinions.len() as f64;
        let spread0 = spread(&opinions);
        let mut world = TestWorld::complete(opinions);
        let mut rng = SimRng::from_seed(9);
        let mut d = DeffuantMechanism::new(1.0, 0.5, 50);
        run(&mut d, &mut world, &mut rng, 100);
        let mean1: f64 = world.opinions.iter().sum::<f64>() / world.opinions.len() as f64;
        assert!((mean0 - mean1).abs() < 1e-9, "mean not conserved");
        assert!(
            spread(&world.opinions) <= spread0 + 1e-12,
            "spread increased"
        );
    }

    #[test]
    fn deffuant_is_deterministic_given_seed() {
        let opinions: Vec<f64> = (0..15).map(|i| i as f64 / 14.0).collect();
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut world = TestWorld::complete(opinions.clone());
            let mut rng = SimRng::from_seed(101);
            let mut d = DeffuantMechanism::new(0.3, 0.5, 30);
            run(&mut d, &mut world, &mut rng, 80);
            runs.push(world.opinions);
        }
        assert_eq!(runs[0], runs[1]);
    }

    // ── convergence helper ───────────────────────────────────────────────────

    #[test]
    fn max_abs_delta_basics() {
        assert_eq!(max_abs_delta(&[1.0, 2.0], &[1.0, 2.5]), 0.5);
        assert!(max_abs_delta(&[1.0], &[1.0, 2.0]).is_infinite());
        assert_eq!(max_abs_delta(&[1.0, 1.0], &[1.0, 1.0]), 0.0);
    }
}
