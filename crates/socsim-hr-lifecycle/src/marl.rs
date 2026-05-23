//! Learnable turnover policy for the HR lifecycle ABM (design §14.1.7).
//!
//! This module makes the **stay/quit decision** learnable: it supplies the
//! [`ObsEncoder`], [`ActionApplier`] and [`RewardFn`] that let a
//! [`DiscretePolicyNet`](socsim_marl::DiscretePolicyNet) replace the fixed
//! [`turnover`](crate::HrLifecyclePack) logit heuristic, plus a small
//! [`TurnoverPrepMechanism`] for the per-step bookkeeping the heuristic used to
//! do inline.
//!
//! ## Reward framing — individual rationality
//!
//! Each employee is treated as a self-interested agent choosing between staying
//! and taking an outside option:
//!
//! - **stay** ⇒ utility `0.5·satisfaction + 0.5·embeddedness` (∈ [0, 1]);
//! - **quit** ⇒ a fixed [`OUTSIDE_OPTION`].
//!
//! A satisfied, embedded employee learns to stay; an unhappy, un-embedded one
//! learns to quit — reproducing rational voluntary turnover as an *emergent*
//! learned policy rather than a hand-tuned logit.  Quitting also lowers
//! network neighbours' embeddedness, so departures propagate a Krackhardt-style
//! cascade through the embeddedness channel.
//!
//! Enabled by the `marl` crate feature.

use socsim_core::{AgentId, Mechanism, Phase, Result, SimRng, StepContext};
use socsim_marl::{ActionApplier, ObsEncoder, RewardFn};

use crate::HrWorld;

/// Number of observation features fed to the turnover policy.
pub const TURNOVER_OBS_DIM: usize = 5;

/// Action index meaning "quit"; `0` means "stay".
pub const ACTION_QUIT: usize = 1;

/// Utility of the outside option (quitting), in the same [0, 1] scale as the
/// stay utility.  An employee whose stay utility falls below this learns to leave.
pub const OUTSIDE_OPTION: f32 = 0.55;

/// Per-departure embeddedness penalty applied to a quitter's neighbours
/// (emergent turnover cascade).
const CASCADE_EMBED_DROP: f64 = 0.03;

#[inline]
fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

// ── Observation encoder ──────────────────────────────────────────────────────

/// Encodes an employee's retention-relevant state into a feature vector:
/// `[embeddedness, satisfaction, po_fit, pj_fit, tenure/120]`.
pub struct TurnoverObsEncoder;

impl ObsEncoder<HrWorld> for TurnoverObsEncoder {
    fn obs_dim(&self) -> usize {
        TURNOVER_OBS_DIM
    }

    fn encode(&self, world: &HrWorld, agent: AgentId) -> Option<Vec<f32>> {
        let e = world.employees.get(&agent)?;
        Some(vec![
            e.embeddedness as f32,
            e.satisfaction as f32,
            e.po_fit as f32,
            e.pj_fit as f32,
            (e.tenure as f32 / 120.0).min(1.0),
        ])
    }
}

// ── Action applier ───────────────────────────────────────────────────────────

/// Applies a stay/quit decision.  On `ACTION_QUIT` the employee is removed, its
/// departure is recorded for downstream mechanisms (`knowledge_loss`,
/// `org_performance`), and its neighbours lose a little embeddedness.
pub struct TurnoverActionApplier;

impl ActionApplier<HrWorld> for TurnoverActionApplier {
    fn n_actions(&self) -> usize {
        2
    }

    fn apply(&self, world: &mut HrWorld, agent: AgentId, action: usize, _rng: &mut SimRng) {
        if action != ACTION_QUIT {
            return; // stay
        }
        let Some(emp) = world.employees.get(&agent) else {
            return; // already gone
        };
        let (theta, tenure, team) = (emp.theta, emp.tenure, emp.team);

        // Emergent cascade: each departure nudges neighbours toward leaving.
        for nb in world.network.neighbors(agent) {
            if let Some(ne) = world.employees.get_mut(&nb) {
                ne.embeddedness = clamp01(ne.embeddedness - CASCADE_EMBED_DROP);
            }
        }

        world.employees.remove(&agent);
        world.network.remove_node(agent);
        world.departed_this_step.push((agent, theta, tenure, team));
    }
}

// ── Reward ───────────────────────────────────────────────────────────────────

/// Individual-rationality reward: stay utility for survivors, the fixed
/// [`OUTSIDE_OPTION`] for an employee who left (and is no longer in `world`).
pub struct TurnoverReward;

impl RewardFn<HrWorld> for TurnoverReward {
    fn reward(&self, world: &HrWorld, agent: AgentId) -> f32 {
        match world.employees.get(&agent) {
            Some(e) => (0.5 * e.satisfaction + 0.5 * e.embeddedness) as f32,
            None => OUTSIDE_OPTION,
        }
    }
}

// ── Per-step bookkeeping ─────────────────────────────────────────────────────

/// `PreStep` companion to the learned turnover policy.
///
/// Replaces the inline bookkeeping the [`turnover`](crate::HrLifecyclePack)
/// heuristic performed: it advances every employee's tenure and captures the
/// head-count used as the `turnover_rate` denominator before any departures.
pub struct TurnoverPrepMechanism;

impl Mechanism<HrWorld> for TurnoverPrepMechanism {
    fn name(&self) -> &str {
        "turnover_prep"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PreStep]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        ctx.world.headcount_at_step_start = ctx.world.employees.len();
        for emp in ctx.world.employees.values_mut() {
            emp.tenure = emp.tenure.saturating_add(1);
        }
        Ok(())
    }
}
