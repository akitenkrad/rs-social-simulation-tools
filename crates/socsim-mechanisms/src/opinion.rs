//! Scalar opinion-dynamics mechanisms.
//!
//! This module ships the **bounded-confidence** family and two **polarising**
//! variants, all operating over any world implementing
//! [`ScalarOpinions`](socsim_core::ScalarOpinions) +
//! [`Neighbors`](socsim_core::Neighbors):
//!
//! - [`HegselmannKrauseMechanism`] — Hegselmann–Krause (2002, generalised
//!   2005): a *synchronous* update where every agent moves to the (chosen) mean
//!   of all opinions within ε of its own.  Math ported from the `hegselmann2005`
//!   replication.
//! - [`DeffuantMechanism`] — Deffuant et al. (2000): a *pairwise / event-based*
//!   update where, on each interaction, two agents within ε move toward each
//!   other by a rate μ.
//! - [`SocialJudgementMechanism`] — Social Judgement: assimilation inside the ε
//!   acceptance region, **repulsion** outside the rejection region.  Math ported
//!   verbatim from the `mou2024` replication's `sj_update`.
//! - [`LorenzMechanism`] — Lorenz (2021): assimilation inside ε plus a
//!   polarisation term that amplifies extreme opinions.  Math ported verbatim
//!   from the `mou2024` replication's `lorenz_update`.

use socsim_core::{AgentId, Mechanism, Neighbors, Phase, Result, ScalarOpinions, StepContext};

use rand::Rng;

use crate::means::{apply_mean, MeanOperator};
use crate::updates::{clamp_attitude, lorenz_update, social_judgement_update};

/// Opinion range `A = [-1, 1]` used by the polarising (SJ / Lorenz) variants.
///
/// Matches the `mou2024` reference's attitude range; the bounded-confidence
/// mechanisms (HK / Deffuant) impose no range of their own.
pub const ATTITUDE_MIN: f64 = crate::updates::ATTITUDE_MIN;
/// Upper bound of the opinion range `A = [-1, 1]`.
pub const ATTITUDE_MAX: f64 = crate::updates::ATTITUDE_MAX;

// ── HegselmannKrauseMechanism ───────────────────────────────────────────────

/// Hegselmann–Krause bounded-confidence update (synchronous).
///
/// On each step this mechanism, for every agent `i`:
/// 1. takes a snapshot of all agents' opinions (so the update is synchronous);
/// 2. collects, from `neighbors_of(i)` ∪ `{i}`, those opinions `x_j` with
///    `|x_i − x_j| ≤ ε` (the confidence set `I(i)`);
/// 3. aggregates them with the configured [`MeanOperator`] via
///    [`apply_mean`](crate::means::apply_mean);
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

impl<W: ScalarOpinions + Neighbors> Mechanism<W> for HegselmannKrauseMechanism {
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
        let mut conf_ids: Vec<AgentId> = Vec::with_capacity(ids.len());
        let mut new_opinions: Vec<f64> = Vec::with_capacity(ids.len());

        for (idx, &id) in ids.iter().enumerate() {
            let xi = prev[idx];
            conf_set.clear();
            // Build the confidence set in **agent-id order** with `x_i` at its
            // natural position: collect the agent's own id together with its
            // neighbour ids, deduplicate, sort, then include each opinion within
            // ε of `x_i` in that order.  This makes `apply_mean`'s floating-point
            // summation order bit-identical to an id-ordered implementation
            // (e.g. the `hegselmann2005` reference); the mean is mathematically
            // the same, this only fixes ulp-level summation order.
            conf_ids.clear();
            conf_ids.push(id);
            conf_ids.extend(ctx.world.neighbors_of(id));
            conf_ids.sort_unstable();
            conf_ids.dedup();
            for &cid in &conf_ids {
                // `x_i` itself is always within ε of itself, so it is included
                // at its natural id-ordered position.
                let xj = ctx.world.opinion(cid);
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
/// neighbour `j` from `neighbors_of(i)`.  If `|x_i − x_j| ≤ ε`, both move toward
/// each other by the convergence rate μ:
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

impl<W: ScalarOpinions + Neighbors> Mechanism<W> for DeffuantMechanism {
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
            let neighbors = ctx.world.neighbors_of(i);
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

// ── SocialJudgementMechanism ────────────────────────────────────────────────

/// Social Judgement opinion update (synchronous).
///
/// For each agent `i`, every neighbour message `m_j` is classified by the
/// signed gap `diff = m_j − x_i`:
///
/// - **acceptance region** (`|diff| < ε`): assimilate, `Δ += α · diff`;
/// - **rejection region** (`|diff| > rejection`): repel, `Δ −= repulsion ·
///   sign(diff)` (move *away* from the message);
/// - **non-commitment region** (`ε ≤ |diff| ≤ rejection`): no contribution.
///
/// The per-agent delta is the mean over the contributing neighbours; the new
/// opinion `x_i + Δ` is clamped to `[-1, 1]` and batch-written (synchronous
/// update from a start-of-step snapshot).  Ported verbatim from the `mou2024`
/// reference's `sj_update`.  The repulsion term is what drives polarisation.
#[derive(Clone, Copy, Debug)]
pub struct SocialJudgementMechanism {
    /// Acceptance region half-width ε: `|diff| < ε` ⇒ assimilate.
    pub epsilon: f64,
    /// Assimilation rate α applied to the in-region gap.
    pub alpha: f64,
    /// Rejection threshold: `|diff| > rejection` ⇒ repel.
    pub rejection: f64,
    /// Repulsion strength (magnitude of the away-from-message push).
    pub repulsion: f64,
}

impl SocialJudgementMechanism {
    /// Create a Social Judgement mechanism with the given acceptance bound `ε`,
    /// assimilation rate `α`, rejection threshold, and repulsion strength.
    pub fn new(epsilon: f64, alpha: f64, rejection: f64, repulsion: f64) -> Self {
        Self {
            epsilon,
            alpha,
            rejection,
            repulsion,
        }
    }
}

impl Default for SocialJudgementMechanism {
    /// ε = 0.4, α = 0.5, rejection = 0.8, repulsion = 0.2 (the `mou2024`
    /// reference defaults).
    fn default() -> Self {
        Self {
            epsilon: 0.4,
            alpha: 0.5,
            rejection: 0.8,
            repulsion: 0.2,
        }
    }
}

impl<W: ScalarOpinions + Neighbors> Mechanism<W> for SocialJudgementMechanism {
    fn name(&self) -> &str {
        "social_judgement"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let ids = ctx.world.agent_ids();
        let prev: Vec<f64> = ids.iter().map(|&id| ctx.world.opinion(id)).collect();

        let mut messages: Vec<f64> = Vec::with_capacity(ids.len());
        let mut new_opinions: Vec<f64> = Vec::with_capacity(ids.len());

        for (idx, &id) in ids.iter().enumerate() {
            let xi = prev[idx];
            messages.clear();
            for nb in ctx.world.neighbors_of(id) {
                if nb == id {
                    continue;
                }
                messages.push(ctx.world.opinion(nb)); // f_message(a_j) = a_j
            }
            let delta = social_judgement_update(
                xi,
                &messages,
                self.epsilon,
                self.alpha,
                self.rejection,
                self.repulsion,
            );
            new_opinions.push(clamp_attitude(xi + delta));
        }

        for (idx, &id) in ids.iter().enumerate() {
            ctx.world.set_opinion(id, new_opinions[idx]);
        }
        Ok(())
    }
}

// ── LorenzMechanism ─────────────────────────────────────────────────────────

/// Lorenz (2021) opinion update — assimilation + polarisation (synchronous).
///
/// For each agent `i`: assimilate the mean in-region gap (`α · mean_{|diff| < ε}
/// diff`), then add a **polarisation** term `repulsion · sign(x_i) · |x_i|` that
/// pushes the current opinion further out in its own direction (extreme
/// opinions are amplified more).  The new opinion `x_i + Δ` is clamped to
/// `[-1, 1]` and batch-written.  Ported verbatim from the `mou2024` reference's
/// `lorenz_update`.
#[derive(Clone, Copy, Debug)]
pub struct LorenzMechanism {
    /// Acceptance region half-width ε for the assimilation term.
    pub epsilon: f64,
    /// Assimilation rate α applied to the in-region gap.
    pub alpha: f64,
    /// Polarisation strength (the `repulsion` field of the `mou2024` reference).
    pub repulsion: f64,
}

impl LorenzMechanism {
    /// Create a Lorenz mechanism with acceptance bound `ε`, assimilation rate
    /// `α`, and polarisation strength `repulsion`.
    pub fn new(epsilon: f64, alpha: f64, repulsion: f64) -> Self {
        Self {
            epsilon,
            alpha,
            repulsion,
        }
    }
}

impl Default for LorenzMechanism {
    /// ε = 0.4, α = 0.5, repulsion = 0.2 (the `mou2024` reference defaults).
    fn default() -> Self {
        Self {
            epsilon: 0.4,
            alpha: 0.5,
            repulsion: 0.2,
        }
    }
}

impl<W: ScalarOpinions + Neighbors> Mechanism<W> for LorenzMechanism {
    fn name(&self) -> &str {
        "lorenz"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let ids = ctx.world.agent_ids();
        let prev: Vec<f64> = ids.iter().map(|&id| ctx.world.opinion(id)).collect();

        let mut messages: Vec<f64> = Vec::with_capacity(ids.len());
        let mut new_opinions: Vec<f64> = Vec::with_capacity(ids.len());

        for (idx, &id) in ids.iter().enumerate() {
            let xi = prev[idx];
            messages.clear();
            for nb in ctx.world.neighbors_of(id) {
                if nb == id {
                    continue;
                }
                messages.push(ctx.world.opinion(nb));
            }
            let delta = lorenz_update(xi, &messages, self.epsilon, self.alpha, self.repulsion);
            new_opinions.push(clamp_attitude(xi + delta));
        }

        for (idx, &id) in ids.iter().enumerate() {
            ctx.world.set_opinion(id, new_opinions[idx]);
        }
        Ok(())
    }
}

// ── Initial profiles ────────────────────────────────────────────────────────

/// Equispaced "ε-profile" initializer for bounded-confidence opinion models:
/// returns `x_i = i / (n − 1)` for `i = 0, …, n−1`.
///
/// This is the canonical *regular profile* used throughout the BC literature
/// (Hegselmann & Krause 2002 §3 Property IV / Fig. 4–8): a deterministic,
/// equispaced sweep of `[0, 1]` with no randomness, useful for the analytic
/// "consensus iff ε-profile" experiments and for sweeps that need a noise-free
/// baseline.  Both the `hegselmann2002` and `hegselmann2005` replications
/// would otherwise re-implement these five lines.
///
/// Edge cases:
/// - `n == 0` ⇒ empty vector.
/// - `n == 1` ⇒ `[0.5]` (centre of `[0, 1]`; avoids dividing by `n − 1 = 0`).
/// - `n >= 2` ⇒ `[0/(n-1), 1/(n-1), …, (n-1)/(n-1)]`, spanning `[0.0, 1.0]`.
///
/// Random profiles (`Uniform(0, 1)`, `Normal`, `polarized`, …) are
/// scenario-specific (rng-driven, often with bound-clamping for `MeanOperator::H`/
/// `MeanOperator::G`) and intentionally left to each callsite.  This helper
/// covers the one shape that is bit-for-bit identical across BC studies.
///
/// # Example
///
/// ```
/// use socsim_mechanisms::regular_profile;
/// let xs = regular_profile(5);
/// assert_eq!(xs, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
/// ```
pub fn regular_profile(n: usize) -> Vec<f64> {
    match n {
        0 => Vec::new(),
        1 => vec![0.5],
        _ => {
            let denom = (n - 1) as f64;
            (0..n).map(|i| i as f64 / denom).collect()
        }
    }
}

#[cfg(test)]
mod profile_tests {
    use super::regular_profile;

    #[test]
    fn regular_profile_empty() {
        assert!(regular_profile(0).is_empty());
    }

    #[test]
    fn regular_profile_single_agent_is_centred() {
        assert_eq!(regular_profile(1), vec![0.5]);
    }

    #[test]
    fn regular_profile_two_agents_span_the_unit_interval() {
        assert_eq!(regular_profile(2), vec![0.0, 1.0]);
    }

    #[test]
    fn regular_profile_five_agents_match_canonical_values() {
        let xs = regular_profile(5);
        assert_eq!(xs, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn regular_profile_endpoints_are_exact() {
        // For any n >= 2 the first and last entries must be exactly 0.0 and 1.0
        // (no floating-point drift on the boundaries).
        for &n in &[2usize, 3, 10, 100, 625, 1_000] {
            let xs = regular_profile(n);
            assert_eq!(xs.len(), n);
            assert_eq!(xs.first().copied(), Some(0.0), "n = {}", n);
            assert_eq!(xs.last().copied(), Some(1.0), "n = {}", n);
        }
    }

    #[test]
    fn regular_profile_is_monotone_and_in_range() {
        let xs = regular_profile(100);
        assert_eq!(xs.len(), 100);
        for w in xs.windows(2) {
            assert!(w[0] < w[1], "non-monotone at {:?}", w);
        }
        assert!(xs.iter().all(|&x| (0.0..=1.0).contains(&x)));
    }

    #[test]
    fn regular_profile_spacing_is_uniform() {
        let xs = regular_profile(11);
        let step = 1.0 / 10.0;
        for w in xs.windows(2) {
            assert!((w[1] - w[0] - step).abs() < 1e-12);
        }
    }
}
