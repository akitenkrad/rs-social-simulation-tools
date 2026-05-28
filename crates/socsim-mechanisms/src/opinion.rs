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

/// Hegselmann–Krause bounded-confidence update (synchronous, symmetric or
/// asymmetric).
///
/// On each step this mechanism, for every agent `i`:
/// 1. takes a snapshot of all agents' opinions (so the update is synchronous);
/// 2. collects, from `neighbors_of(i)` ∪ `{i}`, those opinions `x_j` whose
///    signed gap `x_j − x_i` falls in the per-side window `[−ε_l, ε_r]` (the
///    confidence set `I(i)`);
/// 3. aggregates them with the configured [`MeanOperator`] via
///    [`apply_mean`](crate::means::apply_mean);
/// 4. batch-writes the new opinions.
///
/// Because every new opinion is computed from the *same* start-of-step
/// snapshot, the result is independent of agent activation order — exactly the
/// synchronous (simultaneous) update of the `hegselmann2005` reference.
///
/// # Symmetric vs asymmetric ε
///
/// By default the window is symmetric (`ε_l = ε_r = epsilon`), matching
/// Hegselmann & Krause (2002) §4.1 and the 2005 generalisation: an agent is
/// pulled toward neighbours within `|x_i − x_j| ≤ ε`.  Construct it with
/// [`HegselmannKrauseMechanism::new`].
///
/// For the **asymmetric** variant of HK 2002 §4.2 / Fig. 10–13 — where `ε_l`
/// (left tolerance for *smaller* opinions) and `ε_r` (right tolerance for
/// *larger* opinions) differ, producing one-sided splits and a final mean that
/// drifts toward the wider side — construct it with
/// [`HegselmannKrauseMechanism::with_asymmetric`].  The symmetric code path is
/// preserved bit-identically when both per-side overrides are unset, so
/// upgrading an existing call site is non-breaking.
#[derive(Clone, Copy, Debug)]
pub struct HegselmannKrauseMechanism {
    /// Symmetric confidence bound ε.  Used as the fallback when one or both
    /// of [`epsilon_left`](Self::epsilon_left) /
    /// [`epsilon_right`](Self::epsilon_right) are `None`.
    pub epsilon: f64,
    /// Override for the **left** tolerance (signed gap `x_j − x_i < 0`).  When
    /// `None`, the symmetric [`epsilon`](Self::epsilon) is used.  Setting this
    /// to `Some(ε_l)` activates the asymmetric variant of HK 2002 §4.2.
    pub epsilon_left: Option<f64>,
    /// Override for the **right** tolerance (signed gap `x_j − x_i > 0`).  When
    /// `None`, the symmetric [`epsilon`](Self::epsilon) is used.  Setting this
    /// to `Some(ε_r)` activates the asymmetric variant of HK 2002 §4.2.
    pub epsilon_right: Option<f64>,
    /// Averaging operator applied to the confidence set.
    pub mean: MeanOperator,
}

impl HegselmannKrauseMechanism {
    /// Create a **symmetric** Hegselmann–Krause mechanism with confidence
    /// bound `epsilon` and the given averaging operator.  Equivalent to
    /// `[−ε, ε]` on the signed gap; the canonical HK 2002 / 2005 form.
    pub fn new(epsilon: f64, mean: MeanOperator) -> Self {
        Self {
            epsilon,
            epsilon_left: None,
            epsilon_right: None,
            mean,
        }
    }

    /// Create an **asymmetric** Hegselmann–Krause mechanism (HK 2002 §4.2 /
    /// Fig. 10–13): the confidence window is `[−epsilon_left, epsilon_right]`
    /// on the signed gap `x_j − x_i`.  When `epsilon_left == epsilon_right`,
    /// the result is bit-identical to [`HegselmannKrauseMechanism::new`] with
    /// `epsilon = epsilon_left` (the same code path).
    ///
    /// Sets [`epsilon`](Self::epsilon) to `epsilon_left` so symmetric
    /// fallbacks and diagnostics still report a representative value.
    pub fn with_asymmetric(epsilon_left: f64, epsilon_right: f64, mean: MeanOperator) -> Self {
        Self {
            epsilon: epsilon_left,
            epsilon_left: Some(epsilon_left),
            epsilon_right: Some(epsilon_right),
            mean,
        }
    }

    /// `true` when the mechanism uses different left / right tolerances (i.e.
    /// at least one override is set and the resolved pair is unequal).
    pub fn is_asymmetric(&self) -> bool {
        let l = self.epsilon_left.unwrap_or(self.epsilon);
        let r = self.epsilon_right.unwrap_or(self.epsilon);
        l != r
    }
}

impl Default for HegselmannKrauseMechanism {
    /// ε = 0.2 (symmetric) with the arithmetic mean — the canonical HK setting.
    fn default() -> Self {
        Self {
            epsilon: 0.2,
            epsilon_left: None,
            epsilon_right: None,
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

        // Resolve the per-side window once per step.  When both overrides are
        // unset the window is `[−epsilon, epsilon]`, which makes the membership
        // test below `−ε ≤ x_j − x_i ≤ ε`, i.e. exactly `|x_i − x_j| ≤ ε`.
        let eps_l = self.epsilon_left.unwrap_or(self.epsilon);
        let eps_r = self.epsilon_right.unwrap_or(self.epsilon);

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
            // the per-side window of `x_i` in that order.  This makes
            // `apply_mean`'s floating-point summation order bit-identical to an
            // id-ordered implementation (e.g. the `hegselmann2005` reference);
            // the mean is mathematically the same, this only fixes ulp-level
            // summation order.
            conf_ids.clear();
            conf_ids.push(id);
            conf_ids.extend(ctx.world.neighbors_of(id));
            conf_ids.sort_unstable();
            conf_ids.dedup();
            for &cid in &conf_ids {
                // `x_i` itself is always within window of itself (gap 0), so it
                // is included at its natural id-ordered position.
                let xj = ctx.world.opinion(cid);
                let diff = xj - xi;
                if -eps_l <= diff && diff <= eps_r {
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
