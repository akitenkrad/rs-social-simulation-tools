//! Network-contagion mechanisms (binary state diffusion).
//!
//! Both mechanisms operate over any world implementing
//! [`BinaryState`](socsim_core::BinaryState) +
//! [`Neighbors`](socsim_core::Neighbors), advancing a binary
//! active/informed/infected flag along the topology in **synchronous rounds**:
//! the active set is snapshotted at the start of the step, every inactive agent
//! is evaluated against that snapshot, and newly-activated agents are batch
//! written (an agent activated mid-round is not a source until the next round).
//!
//! - [`SiContagionMechanism`] — SI per-edge infection: each active neighbour
//!   infects an inactive agent independently with probability β.
//! - [`ThresholdContagionMechanism`] — Granovetter (1978) threshold with a single
//!   **global** θ: an inactive agent activates once its fraction of active
//!   neighbours reaches θ.
//! - [`PerAgentThresholdContagionMechanism`] — the Granovetter (1978)
//!   **heterogeneous**-threshold variant: each agent uses its *own* θ_i, read
//!   from the world's [`ActivationThreshold`](socsim_core::ActivationThreshold)
//!   capability, instead of a shared global θ.
//!
//! All call [`StepContext::request_stop`] on **saturation** (a round in which
//! no new agent activates, or everyone is active), matching the `granovetter1973`
//! reference's convergence rule.

use socsim_core::{
    ActivationThreshold, AgentId, BinaryState, Mechanism, Neighbors, Phase, Result, StepContext,
};

use rand::Rng;

// ── SiContagionMechanism ────────────────────────────────────────────────────

/// SI (Susceptible–Infected) contagion with per-edge infection probability β
/// (synchronous rounds).
///
/// Each step:
/// 1. snapshot the active (infected) set;
/// 2. for every inactive agent, draw one independent Bernoulli(β) trial per
///    *active* neighbour (from the snapshot) and infect it if **any** trial
///    succeeds;
/// 3. batch-activate the newly-infected agents;
/// 4. `request_stop` once no new agent was infected or everyone is active.
///
/// All randomness flows through `ctx.rng`, so a fixed seed yields a
/// deterministic trajectory.  β is clamped to `[0, 1]`.  Ported from the
/// `granovetter1973` reference's SI branch.
#[derive(Clone, Copy, Debug)]
pub struct SiContagionMechanism {
    /// Per-edge infection probability β ∈ [0, 1].
    pub beta: f64,
}

impl SiContagionMechanism {
    /// Create an SI mechanism with per-edge infection probability `beta`.
    pub fn new(beta: f64) -> Self {
        Self { beta }
    }
}

impl Default for SiContagionMechanism {
    /// β = 0.5.
    fn default() -> Self {
        Self { beta: 0.5 }
    }
}

impl<W: BinaryState + Neighbors> Mechanism<W> for SiContagionMechanism {
    fn name(&self) -> &str {
        "si_contagion"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let ids = ctx.world.agent_ids();
        // Start-of-round active snapshot (the canonical copy for the
        // synchronous update), keyed by the world's (sorted) id roster.
        let prev: Vec<bool> = ids.iter().map(|&id| ctx.world.is_active(id)).collect();
        let active_of = |id: AgentId| -> bool {
            ids.iter()
                .position(|&x| x == id)
                .map(|p| prev[p])
                .unwrap_or(false)
        };

        // Per-agent infection loop visits agents in the **scheduler activation
        // order** (`ctx.agent_order`), falling back to the world roster when the
        // engine supplied no order.  SI draws one RNG per inactive-agent ×
        // active-neighbour with break-on-first-success, so the visit order is
        // part of the RNG→agent mapping: a faithful Mechanism must respect the
        // scheduler order rather than the sorted id roster.  Snapshot semantics
        // are unchanged (active set frozen at start, newly-infected batch-applied).
        let order: Vec<AgentId> = if ctx.agent_order.is_empty() {
            ids.clone()
        } else {
            ctx.agent_order.to_vec()
        };

        let p = self.beta.clamp(0.0, 1.0);
        let mut newly: Vec<AgentId> = Vec::new();

        for &id in &order {
            if active_of(id) {
                continue; // already active.
            }
            let mut infected = false;
            for nb in ctx.world.neighbors_of(id) {
                if active_of(nb) && ctx.rng.gen::<f64>() < p {
                    infected = true;
                    break; // independent trials; any success infects.
                }
            }
            if infected {
                newly.push(id);
            }
        }

        for &id in &newly {
            ctx.world.set_active(id, true);
        }

        // Saturation: no new infection this round, or everyone active.
        let total_active = prev.iter().filter(|&&a| a).count() + newly.len();
        if newly.is_empty() || total_active >= ids.len() {
            ctx.request_stop();
        }
        Ok(())
    }
}

// ── shared threshold cascade ────────────────────────────────────────────────

/// One synchronous, deterministic threshold round shared by the global-θ
/// [`ThresholdContagionMechanism`] and the per-agent
/// [`PerAgentThresholdContagionMechanism`].
///
/// `theta_of(id)` supplies the activation threshold for agent `id` — a constant
/// for the global path, or `world.activation_threshold(id)` for the per-agent
/// path.  Everything else (the start-of-step snapshot, the inactive-agent scan
/// in the world's `agent_ids()` order, the `a_i / max(d_i, 1) ≥ θ` test, the
/// batch write, and the saturation `request_stop`) is identical regardless of
/// where θ comes from, so the two mechanisms differ *only* in the θ source.
fn threshold_round<W, F>(ctx: &mut StepContext<'_, W>, theta_of: F)
where
    W: BinaryState + Neighbors,
    F: Fn(AgentId) -> f64,
{
    let ids = ctx.world.agent_ids();
    let prev: Vec<bool> = ids.iter().map(|&id| ctx.world.is_active(id)).collect();
    let active_of = |id: AgentId| -> bool {
        ids.iter()
            .position(|&x| x == id)
            .map(|p| prev[p])
            .unwrap_or(false)
    };

    let mut newly: Vec<AgentId> = Vec::new();

    for (idx, &id) in ids.iter().enumerate() {
        if prev[idx] {
            continue;
        }
        let mut deg = 0usize;
        let mut active_nb = 0usize;
        for nb in ctx.world.neighbors_of(id) {
            deg += 1;
            if active_of(nb) {
                active_nb += 1;
            }
        }
        let denom = deg.max(1) as f64;
        if (active_nb as f64) / denom >= theta_of(id) {
            newly.push(id);
        }
    }

    for &id in &newly {
        ctx.world.set_active(id, true);
    }

    let total_active = prev.iter().filter(|&&a| a).count() + newly.len();
    if newly.is_empty() || total_active >= ids.len() {
        ctx.request_stop();
    }
}

// ── ThresholdContagionMechanism ─────────────────────────────────────────────

/// Granovetter (1978) threshold contagion (synchronous, deterministic).
///
/// Each step:
/// 1. snapshot the active set;
/// 2. an inactive agent activates iff `active_neighbours / max(degree, 1) ≥ θ`
///    (evaluated against the snapshot);
/// 3. batch-activate the newly-active agents;
/// 4. `request_stop` once no new agent activated or everyone is active.
///
/// Fully deterministic (the RNG is untouched).  Ported from the
/// `granovetter1973` reference's threshold branch.
#[derive(Clone, Copy, Debug)]
pub struct ThresholdContagionMechanism {
    /// Activation threshold θ: fraction of active neighbours required.
    pub theta: f64,
}

impl ThresholdContagionMechanism {
    /// Create a threshold mechanism with activation threshold `theta`.
    pub fn new(theta: f64) -> Self {
        Self { theta }
    }
}

impl Default for ThresholdContagionMechanism {
    /// θ = 0.5.
    fn default() -> Self {
        Self { theta: 0.5 }
    }
}

impl<W: BinaryState + Neighbors> Mechanism<W> for ThresholdContagionMechanism {
    fn name(&self) -> &str {
        "threshold_contagion"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let theta = self.theta;
        threshold_round(ctx, |_id| theta);
        Ok(())
    }
}

// ── PerAgentThresholdContagionMechanism ─────────────────────────────────────

/// Granovetter (1978) threshold contagion with **per-agent (heterogeneous)**
/// thresholds (synchronous, deterministic).
///
/// Identical to [`ThresholdContagionMechanism`] except that each inactive agent
/// `i` is tested against its *own* threshold θ_i read from the world's
/// [`ActivationThreshold`] capability, rather than against a single global θ:
///
/// 1. snapshot the active set;
/// 2. an inactive agent `i` activates iff
///    `active_neighbours / max(degree, 1) ≥ θ_i` (evaluated against the snapshot,
///    with `θ_i = world.activation_threshold(i)`);
/// 3. batch-activate the newly-active agents;
/// 4. `request_stop` once no new agent activated or everyone is active.
///
/// This is the variant that expresses Granovetter's central point — that a
/// *distribution* of individual thresholds, not a single global one, governs
/// whether a cascade ignites.  Fully deterministic (the RNG is untouched); the
/// cascade logic is shared verbatim with the global-θ mechanism, so the two
/// agree whenever every θ_i equals the global θ.  This is a zero-sized unit
/// struct: the thresholds live in the world, not the mechanism.
#[derive(Clone, Copy, Debug, Default)]
pub struct PerAgentThresholdContagionMechanism;

impl PerAgentThresholdContagionMechanism {
    /// Create a per-agent threshold mechanism.  Thresholds are supplied by the
    /// world via [`ActivationThreshold`], so the mechanism itself is stateless.
    pub fn new() -> Self {
        Self
    }
}

impl<W: BinaryState + Neighbors + ActivationThreshold> Mechanism<W>
    for PerAgentThresholdContagionMechanism
{
    fn name(&self) -> &str {
        "per_agent_threshold_contagion"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        // Precompute each agent's θ_i *before* the cascade so the shared
        // `threshold_round` (which holds `&mut ctx.world`) can look it up without
        // aliasing the world borrow.  Keying by agent id keeps the lookup correct
        // regardless of roster ordering; thresholds are read once per step from
        // the start-of-step world, matching the snapshot semantics.
        let thetas: std::collections::HashMap<AgentId, f64> = ctx
            .world
            .agent_ids()
            .into_iter()
            .map(|id| (id, ctx.world.activation_threshold(id)))
            .collect();
        threshold_round(ctx, |id| thetas.get(&id).copied().unwrap_or(f64::INFINITY));
        Ok(())
    }
}
