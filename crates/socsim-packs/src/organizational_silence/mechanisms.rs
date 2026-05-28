//! Ten rule-based mechanisms for the organizational-silence ABM and the
//! [`OrganizationalSilencePack`] registration bundle.
//!
//! See §5.3 of `組織的沈黙のLLM-Agentシミュレーション設計.md` for the
//! phase placement and theoretical grounding of each mechanism.
//!
//! All mechanisms iterate agents in a fixed (sorted) order before any RNG
//! draw, so a fixed seed reproduces the run bit-identically.  Synchronous
//! within-phase updates are achieved by snapshotting `ρ_i` (the neighbour
//! silence ratio) at the end of each step's `SilenceSpiralMechanism`; the
//! next step's `VoiceDecisionRuleMechanism` reads that snapshot.

use rand::Rng;
use serde_json::json;

use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{AgentId, Mechanism, Phase, Result, StepContext, WorldState};

use crate::organizational_silence::{
    calibration::{
        BETA_0, BETA_CLIMATE, BETA_FEAR, BETA_IVT, BETA_PSAFETY, BETA_SALIENCE, BETA_SUP,
        EPSILON_SPIRAL, FEAR_SENSITIVITY, PSAFETY_LEARN, P_RETALIATE, SHOCK_DELTA, SHOCK_T,
        SIGMA_BASE,
    },
    world::{Expression, Motive, SilenceWorld},
};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Clamp a value to `[0, 1]`.
#[inline]
fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// Logistic function `σ(x) = 1 / (1 + e^{-x})`.
#[inline]
fn logistic(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

// ── 1. IssueSalienceMechanism (Environment) ──────────────────────────────────

/// Update `world.issue_salience` σ(t).
///
/// σ(t) = `sigma_base` for t < `shock_t`; σ(t) = `sigma_base + shock_delta`
/// for t ≥ `shock_t`.  This models a single triggering event (regulator
/// inquiry / public accusation / earnings shock) that raises the stakes of
/// withheld concerns mid-run.
pub struct IssueSalienceMechanism {
    sigma_base: f64,
    shock_t: u64,
    shock_delta: f64,
}

impl IssueSalienceMechanism {
    /// Construct from `[mechanism.params]`.  Keys: `sigma_base` (0.3),
    /// `shock_t` (24), `shock_delta` (0.4).
    pub fn from_params(p: &Params) -> Self {
        Self {
            sigma_base: p.get_f64("sigma_base", SIGMA_BASE),
            shock_t: p.get_u64("shock_t", SHOCK_T),
            shock_delta: p.get_f64("shock_delta", SHOCK_DELTA),
        }
    }
}

impl Mechanism<SilenceWorld> for IssueSalienceMechanism {
    fn name(&self) -> &str {
        "issue_salience"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Environment]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        let t = ctx.clock.t();
        let sigma = if t >= self.shock_t {
            self.sigma_base + self.shock_delta
        } else {
            self.sigma_base
        };
        ctx.world.issue_salience = sigma.clamp(0.0, 1.0);
        Ok(())
    }
}

// ── 2. RetaliationEventMechanism (Environment) ───────────────────────────────

/// Fire a stochastic retaliation event.
///
/// With probability `p_retaliate` per step the mechanism picks one of the most
/// recent voice-ers (fallback to a random agent when no voice-ers exist) and
/// marks all of their neighbours as having been retaliated against this step.
/// Downstream, `FearAppraisalMechanism` reads `retaliation_this_step` and
/// raises those agents' fear.
///
/// Deterministic: the candidate set is built from a sorted iteration of
/// employees before any RNG draw.
pub struct RetaliationEventMechanism {
    p_retaliate: f64,
}

impl RetaliationEventMechanism {
    /// Construct from `[mechanism.params]`.  Key: `p_retaliate` (0.05).
    pub fn from_params(p: &Params) -> Self {
        Self {
            p_retaliate: p.get_f64("p_retaliate", P_RETALIATE),
        }
    }
}

impl Mechanism<SilenceWorld> for RetaliationEventMechanism {
    fn name(&self) -> &str {
        "retaliation_event"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Environment]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        // Always clear last step's retaliation list — even when no event fires
        // this step — so PostStep aggregators see an accurate snapshot.
        ctx.world.retaliation_this_step.clear();

        if ctx.rng.gen::<f64>() >= self.p_retaliate {
            return Ok(());
        }

        // Sorted iteration before any RNG draw for determinism.
        let voicers: Vec<AgentId> = ctx
            .world
            .employees
            .iter()
            .filter(|(_, e)| e.expression == Expression::Voice)
            .map(|(id, _)| *id)
            .collect();
        let candidates: Vec<AgentId> = if voicers.is_empty() {
            // Fallback: any agent could be a target (e.g. retaliation against
            // perceived dissent that has not yet been publicly voiced).
            ctx.world.agent_ids()
        } else {
            voicers
        };

        if candidates.is_empty() {
            return Ok(());
        }

        let idx = ctx.rng.gen_range(0..candidates.len());
        let target = candidates[idx];
        let mut affected = ctx.world.network.neighbors(target);
        affected.sort();
        affected.push(target); // The target themselves is also marked.
        affected.sort();
        affected.dedup();
        ctx.world.retaliation_this_step = affected.clone();

        ctx.recorder.record_event(
            ctx.clock.t(),
            "retaliation",
            json!({
                "target": target.0,
                "n_affected": affected.len(),
            }),
        );
        Ok(())
    }
}

// ── 3. FearAppraisalMechanism (Decision) ─────────────────────────────────────

/// Update each employee's fear `f_i` from this step's retaliation set and
/// their supervisor's openness.
///
/// For each employee `i`:
/// `f ← clamp(f + fear_sensitivity · 1[i ∈ retaliation] − DECAY − OPEN_BONUS · max(0, u_k), 0, 1)`.
/// `DECAY = 0.02` is a small per-step return-to-baseline; `OPEN_BONUS = 0.05`
/// reduces fear under positive supervisor openness.
pub struct FearAppraisalMechanism {
    fear_sensitivity: f64,
}

impl FearAppraisalMechanism {
    /// Construct from `[mechanism.params]`.  Key: `fear_sensitivity` (0.4).
    pub fn from_params(p: &Params) -> Self {
        Self {
            fear_sensitivity: p.get_f64("fear_sensitivity", FEAR_SENSITIVITY),
        }
    }
}

impl Mechanism<SilenceWorld> for FearAppraisalMechanism {
    fn name(&self) -> &str {
        "fear_appraisal"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        const DECAY: f64 = 0.02;
        const OPEN_BONUS: f64 = 0.05;

        // Snapshot the retaliation set & team openness for read-only access
        // while we mutate employees.
        let retaliated: std::collections::HashSet<AgentId> =
            ctx.world.retaliation_this_step.iter().copied().collect();
        let team_openness: Vec<f64> = ctx
            .world
            .teams
            .iter()
            .map(|t| t.supervisor_openness)
            .collect();

        for (id, emp) in ctx.world.employees.iter_mut() {
            let retaliation_term = if retaliated.contains(id) {
                self.fear_sensitivity
            } else {
                0.0
            };
            let u_k = team_openness.get(emp.team).copied().unwrap_or(0.0);
            let openness_term = OPEN_BONUS * u_k.max(0.0);
            emp.fear = clamp01(emp.fear + retaliation_term - DECAY - openness_term);
        }

        Ok(())
    }
}

// ── 4. VoiceDecisionRuleMechanism (Decision) ─────────────────────────────────

/// Rule-based logistic voice-vs-silence decision (§4.3 of the design doc).
///
/// For each agent in `ctx.agent_order`:
///
/// ```text
/// logit = β_0
///       + β_ψ · ψ_i
///       + β_u · u_{k(i)}
///       + β_σ · σ
///       − β_f · f_i
///       − β_ι · ι_i
///       − β_C · ρ_i
/// p = logistic(logit)
/// ```
///
/// `ρ_i` is the snapshot stored on the [`Employee`] at the end of the
/// previous step's `SilenceSpiralMechanism`, so the read is synchronous
/// within the Decision phase.  Bernoulli(`p`) determines Voice vs Silence.
/// On a Silence, a [`Motive`] is assigned by ranking the three suppressors:
///
/// - `Defensive` if fear dominates;
/// - `Acquiescent` if IVT dominates;
/// - `Prosocial` if the agent's supervisor is positive AND the private
///   concern is negative (protective silence).
pub struct VoiceDecisionRuleMechanism {
    beta_0: f64,
    beta_psafety: f64,
    beta_fear: f64,
    beta_ivt: f64,
    beta_sup: f64,
    beta_salience: f64,
    beta_climate: f64,
}

impl VoiceDecisionRuleMechanism {
    /// Construct from `[mechanism.params]`.  Keys: `beta_0` (-0.5),
    /// `beta_psafety` (1.2), `beta_fear` (1.5), `beta_ivt` (0.8),
    /// `beta_sup` (1.0), `beta_salience` (1.0), `beta_climate` (1.5).
    pub fn from_params(p: &Params) -> Self {
        Self {
            beta_0: p.get_f64("beta_0", BETA_0),
            beta_psafety: p.get_f64("beta_psafety", BETA_PSAFETY),
            beta_fear: p.get_f64("beta_fear", BETA_FEAR),
            beta_ivt: p.get_f64("beta_ivt", BETA_IVT),
            beta_sup: p.get_f64("beta_sup", BETA_SUP),
            beta_salience: p.get_f64("beta_salience", BETA_SALIENCE),
            beta_climate: p.get_f64("beta_climate", BETA_CLIMATE),
        }
    }

    /// Classify the dominant suppressor of voice into a [`Motive`].
    ///
    /// Pure function — testable in isolation.
    fn classify_motive(
        fear: f64,
        ivt: f64,
        supervisor_openness: f64,
        private_concern: f64,
    ) -> Motive {
        // Prosocial silence is a *characterised* state, not a strength score:
        // an agent is staying quiet because the supervisor is open AND their
        // private concern is negative (i.e. they have something protective to
        // withhold).  Detect it first.
        if supervisor_openness > 0.0 && private_concern < 0.0 {
            return Motive::Prosocial;
        }
        if fear >= ivt {
            Motive::Defensive
        } else {
            Motive::Acquiescent
        }
    }
}

impl Mechanism<SilenceWorld> for VoiceDecisionRuleMechanism {
    fn name(&self) -> &str {
        "voice_decision_rule"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        // Snapshot team openness so we can borrow `employees` mutably below.
        let team_openness: Vec<f64> = ctx
            .world
            .teams
            .iter()
            .map(|t| t.supervisor_openness)
            .collect();
        let sigma = ctx.world.issue_salience;

        // Iterate in `ctx.agent_order` for determinism.  Skip any id no longer
        // present (defensive: this pack does not remove employees, but future
        // hires/turnover could be added downstream).
        let order: Vec<AgentId> = ctx.agent_order.to_vec();
        for id in order {
            // Read the snapshot fields before mutating.
            let (psi, fear, ivt, u_k, rho) = match ctx.world.employees.get(&id) {
                Some(e) => (
                    e.psych_safety,
                    e.fear,
                    e.ivt_strength,
                    team_openness.get(e.team).copied().unwrap_or(0.0),
                    e.neighbor_silence_ratio,
                ),
                None => continue,
            };

            let logit = self.beta_0
                + self.beta_psafety * psi
                + self.beta_sup * u_k
                + self.beta_salience * sigma
                - self.beta_fear * fear
                - self.beta_ivt * ivt
                - self.beta_climate * rho;
            let p = logistic(logit);

            // Draw the Bernoulli after computing the logit (one RNG draw per
            // agent in `ctx.agent_order` order).
            let draw: f64 = ctx.rng.gen();
            if let Some(emp) = ctx.world.employees.get_mut(&id) {
                if draw < p {
                    emp.expression = Expression::Voice;
                    emp.silence_motive = None;
                } else {
                    emp.expression = Expression::Silence;
                    emp.silence_motive = Some(Self::classify_motive(
                        fear,
                        ivt,
                        u_k,
                        emp.private_concern,
                    ));
                }
            }
        }
        Ok(())
    }
}

// ── 5. SilenceSpiralMechanism (Interaction) ──────────────────────────────────

/// Snapshot each employee's neighbour silence ratio `ρ_i` and apply a small
/// downward nudge to `psych_safety` proportional to that ratio.
///
/// The snapshot is written **after** the Decision phase has run, so it
/// captures the silence ratio induced by *this step's* voice/silence
/// decisions.  Next step's `VoiceDecisionRuleMechanism` reads it; this is
/// the "synchronous within phase" update pattern used by the design doc.
pub struct SilenceSpiralMechanism {
    epsilon: f64,
}

impl SilenceSpiralMechanism {
    /// Construct from `[mechanism.params]`.  Key: `epsilon` (0.25).
    pub fn from_params(p: &Params) -> Self {
        Self {
            epsilon: p.get_f64("epsilon", EPSILON_SPIRAL),
        }
    }
}

impl Mechanism<SilenceWorld> for SilenceSpiralMechanism {
    fn name(&self) -> &str {
        "silence_spiral"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        // Sort by AgentId for deterministic f64 accumulation order.
        let mut sorted_ids: Vec<AgentId> = ctx.world.employees.keys().copied().collect();
        sorted_ids.sort();
        let ratios: Vec<(AgentId, f64)> = sorted_ids
            .iter()
            .map(|&id| (id, ctx.world.neighbor_silence_ratio(id)))
            .collect();

        for (id, rho) in ratios {
            if let Some(emp) = ctx.world.employees.get_mut(&id) {
                emp.neighbor_silence_ratio = rho;
                // Spiral effect: high local silence erodes perceived psychological safety.
                emp.psych_safety = clamp01(emp.psych_safety - self.epsilon * rho * 0.05);
            }
        }
        Ok(())
    }
}

// ── 6. PreferenceFalsificationCascadeMechanism (Interaction) ─────────────────

/// Granovetter / Kuran threshold cascade on the public expression channel.
///
/// Repeatedly flip any silent-with-negative-concern agent to `Voice` when
/// the neighbour voice ratio exceeds their personal `voice_threshold`, until
/// no further flips happen.  If the total flipped exceeds 5% of the active
/// employee population, record a `cascade` event with the cascade size.
///
/// Determinism: each inner pass iterates a sorted snapshot of silent-with-
/// negative-concern ids and applies all flips at the end of the pass (so a
/// single pass acts as a synchronous round).
pub struct PreferenceFalsificationCascadeMechanism {
    cascade_threshold: f64,
}

impl PreferenceFalsificationCascadeMechanism {
    /// Construct from `[mechanism.params]`.  Key: `cascade_threshold` (0.05).
    pub fn from_params(p: &Params) -> Self {
        Self {
            cascade_threshold: p.get_f64("cascade_threshold", 0.05),
        }
    }
}

impl Mechanism<SilenceWorld> for PreferenceFalsificationCascadeMechanism {
    fn name(&self) -> &str {
        "prefalse_cascade"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        let total_n = ctx.world.employees.len();
        let mut total_flipped: usize = 0;

        loop {
            // Snapshot candidates (sorted) and their neighbour voice ratios.
            let mut candidates: Vec<AgentId> = ctx
                .world
                .employees
                .iter()
                .filter(|(_, e)| {
                    e.expression == Expression::Silence && e.private_concern < 0.0
                })
                .map(|(id, _)| *id)
                .collect();
            candidates.sort();

            let mut to_flip: Vec<AgentId> = Vec::new();
            for id in &candidates {
                let theta = match ctx.world.employees.get(id) {
                    Some(e) => e.voice_threshold,
                    None => continue,
                };
                let rho_v = ctx.world.neighbor_voice_ratio(*id);
                if rho_v > theta {
                    to_flip.push(*id);
                }
            }

            if to_flip.is_empty() {
                break;
            }

            for id in &to_flip {
                if let Some(emp) = ctx.world.employees.get_mut(id) {
                    emp.expression = Expression::Voice;
                    emp.silence_motive = None;
                }
            }
            total_flipped += to_flip.len();
        }

        if total_n > 0 && (total_flipped as f64 / total_n as f64) > self.cascade_threshold {
            ctx.recorder.record_event(
                ctx.clock.t(),
                "cascade",
                json!({
                    "size": total_flipped,
                    "fraction": total_flipped as f64 / total_n as f64,
                }),
            );
        }
        Ok(())
    }
}

// ── 7. OrgPerformanceMechanism (Reward) ──────────────────────────────────────

/// Aggregate the macro variables and record all required metrics each step.
///
/// Records:
/// - `silence_rate` — fraction in `Silence`.
/// - `climate_of_silence` — fraction in `Silence ∧ private_concern < 0`.
/// - `voice_volume` — fraction in `Voice`.
/// - `knowledge_stock` — sum of team knowledge stocks `K(t)`.
/// - `org_performance` — `Π(t) = K(t) · (1 − C(t))`.
/// - `opinion_clusters` — count of distinct private-concern clusters within
///   tolerance `cluster_tol` (default 0.05) via [`socsim_metrics::stats::distinct_clusters`].
/// - `n_employees` — current active headcount.
///
/// Also fires a `motive_mix` event each step with the (acquiescent, defensive,
/// prosocial) breakdown over currently-silent agents.
pub struct OrgPerformanceMechanism {
    cluster_tol: f64,
}

impl OrgPerformanceMechanism {
    /// Construct from `[mechanism.params]`.  Key: `cluster_tol` (0.05).
    pub fn from_params(p: &Params) -> Self {
        Self {
            cluster_tol: p.get_f64("cluster_tol", 0.05),
        }
    }
}

impl Mechanism<SilenceWorld> for OrgPerformanceMechanism {
    fn name(&self) -> &str {
        "org_performance"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Reward]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        // Refresh aggregate caches first so the recorded numbers are
        // consistent with the world state.
        ctx.world.recompute_macro_aggregates();
        let silence_rate = ctx.world.silence_rate();
        let voice_volume = ctx.world.voice_volume;
        let climate = ctx.world.climate_of_silence;
        let knowledge = ctx.world.total_knowledge_stock();
        let perf = knowledge * (1.0 - climate).max(0.0);
        ctx.world.org_performance = perf;

        // Opinion clusters on private_concern values (sorted-greedy single
        // linkage on a strict `> tol` gap — see socsim_metrics::stats).
        let mut concerns: Vec<f64> = ctx
            .world
            .employees
            .values()
            .map(|e| e.private_concern)
            .collect();
        // Sort for determinism (distinct_clusters sorts internally, but
        // sorting upstream makes the `concerns` Vec itself deterministic).
        concerns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let clusters = socsim_metrics::stats::distinct_clusters(&concerns, self.cluster_tol)
            as f64;

        // Motive mix over silent agents.
        let mut acq = 0usize;
        let mut def = 0usize;
        let mut pro = 0usize;
        let mut none_motive = 0usize;
        for emp in ctx.world.employees.values() {
            if emp.expression == Expression::Silence {
                match emp.silence_motive {
                    Some(Motive::Acquiescent) => acq += 1,
                    Some(Motive::Defensive) => def += 1,
                    Some(Motive::Prosocial) => pro += 1,
                    None => none_motive += 1,
                }
            }
        }

        let t = ctx.clock.t();
        ctx.recorder.record_metric(t, "silence_rate", silence_rate);
        ctx.recorder.record_metric(t, "climate_of_silence", climate);
        ctx.recorder.record_metric(t, "voice_volume", voice_volume);
        ctx.recorder.record_metric(t, "knowledge_stock", knowledge);
        ctx.recorder.record_metric(t, "org_performance", perf);
        ctx.recorder.record_metric(t, "opinion_clusters", clusters);
        ctx.recorder
            .record_metric(t, "n_employees", ctx.world.employees.len() as f64);

        ctx.recorder.record_event(
            t,
            "motive_mix",
            json!({
                "acquiescent": acq,
                "defensive": def,
                "prosocial": pro,
                "no_motive": none_motive,
            }),
        );
        Ok(())
    }
}

// ── 8. PsafetyUpdateMechanism (PostStep) ─────────────────────────────────────

/// Update each employee's perceived psychological safety `ψ_i` from this
/// step's voice / retaliation experience (Edmondson 1999).
///
/// `ψ ← clamp(ψ + psafety_learn · [voiced] − psafety_learn · [retaliated], 0, 1)`.
/// Voicing without negative consequence raises perceived safety; being
/// retaliated against lowers it.  Both effects are scaled by the same learning
/// rate `psafety_learn`.
pub struct PsafetyUpdateMechanism {
    psafety_learn: f64,
}

impl PsafetyUpdateMechanism {
    /// Construct from `[mechanism.params]`.  Key: `psafety_learn` (0.1).
    pub fn from_params(p: &Params) -> Self {
        Self {
            psafety_learn: p.get_f64("psafety_learn", PSAFETY_LEARN),
        }
    }
}

impl Mechanism<SilenceWorld> for PsafetyUpdateMechanism {
    fn name(&self) -> &str {
        "psafety_update"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        let retaliated: std::collections::HashSet<AgentId> =
            ctx.world.retaliation_this_step.iter().copied().collect();

        for (id, emp) in ctx.world.employees.iter_mut() {
            let voiced = emp.expression == Expression::Voice;
            let hit = retaliated.contains(id);
            let delta = if voiced { self.psafety_learn } else { 0.0 }
                - if hit { self.psafety_learn } else { 0.0 };
            emp.psych_safety = clamp01(emp.psych_safety + delta);
        }
        Ok(())
    }
}

// ── 9. ClimateSilenceMechanism (PostStep) ────────────────────────────────────

/// Recompute and re-cache the climate of silence `C(t)`.
///
/// `OrgPerformanceMechanism` already calls `recompute_macro_aggregates` in
/// the Reward phase, so by the time this PostStep runs `climate_of_silence`
/// is already current.  We still call `recompute_macro_aggregates` here to
/// reflect any changes the Reward-phase mechanisms or the cascade made after
/// the original computation, keeping the published value of `C(t)` consistent
/// with the end-of-step world state.
pub struct ClimateSilenceMechanism;

impl ClimateSilenceMechanism {
    /// Construct (no params).
    pub fn from_params(_p: &Params) -> Self {
        Self
    }
}

impl Mechanism<SilenceWorld> for ClimateSilenceMechanism {
    fn name(&self) -> &str {
        "climate_silence"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        ctx.world.recompute_macro_aggregates();
        Ok(())
    }
}

// ── 10. OrgLearningMechanism (PostStep) ──────────────────────────────────────

/// Argyris (1977) double-loop-learning increment / decay of team knowledge.
///
/// When at least one employee voiced this step *and* `issue_salience > 0.3`,
/// each voicer's team gets a `learning_rate` increment to its
/// `knowledge_stock`.  Otherwise the entire knowledge stock decays by a small
/// `decay_rate` (≈ 1%/step) reflecting unrenewed tacit knowledge in a silent
/// climate.
pub struct OrgLearningMechanism {
    learning_rate: f64,
    decay_rate: f64,
    salience_floor: f64,
}

impl OrgLearningMechanism {
    /// Construct from `[mechanism.params]`.  Keys: `learning_rate` (0.05),
    /// `decay_rate` (0.01), `salience_floor` (0.3).
    pub fn from_params(p: &Params) -> Self {
        Self {
            learning_rate: p.get_f64("learning_rate", 0.05),
            decay_rate: p.get_f64("decay_rate", 0.01),
            salience_floor: p.get_f64("salience_floor", 0.3),
        }
    }
}

impl Mechanism<SilenceWorld> for OrgLearningMechanism {
    fn name(&self) -> &str {
        "org_learning"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, SilenceWorld>) -> Result<()> {
        let sigma = ctx.world.issue_salience;
        // Count voicers per team in a sorted iteration for determinism.
        let mut team_voicers = vec![0usize; ctx.world.teams.len()];
        let mut total_voicers = 0usize;
        // BTreeMap iteration is already sorted by AgentId.
        for emp in ctx.world.employees.values() {
            if emp.expression == Expression::Voice && emp.team < team_voicers.len() {
                team_voicers[emp.team] += 1;
                total_voicers += 1;
            }
        }

        if total_voicers > 0 && sigma > self.salience_floor {
            // Voicing under salient conditions: bump each team's knowledge
            // proportional to the number of its voicers (Argyris: double-loop
            // learning is *triggered* by surfaced concerns).
            for (i, team) in ctx.world.teams.iter_mut().enumerate() {
                team.knowledge_stock += self.learning_rate * team_voicers[i] as f64;
            }
        } else {
            // Silent climate: tacit knowledge drains slowly.
            for team in ctx.world.teams.iter_mut() {
                team.knowledge_stock =
                    (team.knowledge_stock * (1.0 - self.decay_rate)).max(0.0);
            }
        }
        Ok(())
    }
}

// ── OrganizationalSilencePack ────────────────────────────────────────────────

/// [`ModulePack`] that registers all organizational-silence mechanisms into a
/// [`Registry<SilenceWorld>`].
///
/// Always registers the 10 rule-based mechanisms.  When the
/// `organizational-silence-llm` feature is enabled, an additional
/// `voice_decision` mechanism (the LLM variant) is registered alongside the
/// rule-based `voice_decision_rule`.
pub struct OrganizationalSilencePack;

impl ModulePack<SilenceWorld> for OrganizationalSilencePack {
    fn pack_name(&self) -> &str {
        "organizational-silence"
    }

    fn register(&self, reg: &mut Registry<SilenceWorld>) {
        reg.register("issue_salience", |p| {
            Ok(Box::new(IssueSalienceMechanism::from_params(p)))
        });
        reg.register("retaliation_event", |p| {
            Ok(Box::new(RetaliationEventMechanism::from_params(p)))
        });
        reg.register("fear_appraisal", |p| {
            Ok(Box::new(FearAppraisalMechanism::from_params(p)))
        });
        reg.register("voice_decision_rule", |p| {
            Ok(Box::new(VoiceDecisionRuleMechanism::from_params(p)))
        });
        reg.register("silence_spiral", |p| {
            Ok(Box::new(SilenceSpiralMechanism::from_params(p)))
        });
        reg.register("prefalse_cascade", |p| {
            Ok(Box::new(PreferenceFalsificationCascadeMechanism::from_params(p)))
        });
        reg.register("org_performance", |p| {
            Ok(Box::new(OrgPerformanceMechanism::from_params(p)))
        });
        reg.register("psafety_update", |p| {
            Ok(Box::new(PsafetyUpdateMechanism::from_params(p)))
        });
        reg.register("climate_silence", |p| {
            Ok(Box::new(ClimateSilenceMechanism::from_params(p)))
        });
        reg.register("org_learning", |p| {
            Ok(Box::new(OrgLearningMechanism::from_params(p)))
        });

        // The LLM variant of voice_decision is registered under the canonical
        // name `voice_decision`, while the rule-based path keeps
        // `voice_decision_rule`.  Both can coexist in a registry so a scenario
        // selects between them by name.
        #[cfg(feature = "organizational-silence-llm")]
        reg.register("voice_decision", |p| {
            let m = crate::organizational_silence::mechanisms_llm::VoiceDecisionLlmMechanism::from_params(p)?;
            Ok(Box::new(m))
        });
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use socsim_core::{Recorder, SimRng};
    use socsim_engine::{SequentialScheduler, SimulationBuilder};
    use socsim_log::InMemoryRecorder;

    use crate::organizational_silence::world::SilenceWorld;

    use super::*;

    /// Shared in-memory recorder so we can hand it to `SimulationBuilder` and
    /// still inspect rows after the run.
    struct SharedRecorder(Arc<Mutex<InMemoryRecorder>>);

    impl SharedRecorder {
        fn new() -> (Self, Arc<Mutex<InMemoryRecorder>>) {
            let inner = Arc::new(Mutex::new(InMemoryRecorder::new()));
            (Self(Arc::clone(&inner)), inner)
        }
    }

    impl Recorder for SharedRecorder {
        fn record_metric(&mut self, t: u64, key: &str, value: f64) {
            self.0.lock().unwrap().record_metric(t, key, value);
        }
        fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value) {
            self.0.lock().unwrap().record_event(t, kind, payload);
        }
    }

    fn build_world(seed: u64, n_teams: usize, team_size: usize) -> SilenceWorld {
        let mut rng = SimRng::from_seed(seed);
        SilenceWorld::new(n_teams, team_size, 2, 4, 0.1, 0.5, &mut rng)
    }

    fn mechanism_names() -> &'static [&'static str] {
        &[
            "issue_salience",
            "retaliation_event",
            "fear_appraisal",
            "voice_decision_rule",
            "silence_spiral",
            "prefalse_cascade",
            "org_performance",
            "psafety_update",
            "climate_silence",
            "org_learning",
        ]
    }

    #[test]
    fn world_builds_deterministically() {
        // Two worlds built from the same seed must serialise byte-identically.
        let w1 = build_world(7, 3, 4);
        let w2 = build_world(7, 3, 4);
        let s1 = serde_json::to_string(&w1.employees).unwrap();
        let s2 = serde_json::to_string(&w2.employees).unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn pack_registers_all_ten_mechanisms() {
        let mut reg: Registry<SilenceWorld> = Registry::new();
        OrganizationalSilencePack.register(&mut reg);
        let mut names: Vec<String> = reg.names().into_iter().map(|s| s.to_owned()).collect();
        names.sort();
        // Exactly the 10 rule-based mechanisms.
        let mut expected: Vec<&str> = mechanism_names().to_vec();
        expected.sort();
        let expected: Vec<String> = expected.into_iter().map(|s| s.to_owned()).collect();
        // When the LLM feature is off there should be exactly 10 mechanisms.
        // When on, there's an extra `voice_decision`.
        #[cfg(not(feature = "organizational-silence-llm"))]
        assert_eq!(names, expected);
        #[cfg(feature = "organizational-silence-llm")]
        {
            assert_eq!(names.len(), expected.len() + 1);
            for name in &expected {
                assert!(names.contains(name), "missing mechanism: {name}");
            }
            assert!(names.contains(&"voice_decision".to_string()));
        }
    }

    #[test]
    fn tick_runs_three_steps() {
        let world = build_world(0, 2, 4);
        let mut reg = Registry::new();
        OrganizationalSilencePack.register(&mut reg);
        let p = Params::empty();

        let mut builder = SimulationBuilder::new(world)
            .scheduler(Box::new(SequentialScheduler))
            .seed(0);
        for name in mechanism_names() {
            let m = reg.build(name, &p).expect("mechanism registered");
            builder = builder.add_mechanism(m);
        }
        let mut sim = builder.build();
        for _ in 0..3 {
            sim.step().expect("step should succeed");
        }
        assert_eq!(sim.world().clock().t(), 3);
    }

    #[test]
    fn silence_spiral_increases_silence_under_hostile_supervisors() {
        // Build two worlds: one with all-hostile supervisors (u_k = −1) and
        // high fear; the other with all-open supervisors (u_k = +1).  After
        // 10 ticks, the hostile world should have a strictly higher silence
        // rate.
        fn run(supervisor_sign: f64, fear_floor: f64, seed: u64) -> f64 {
            let mut rng = SimRng::from_seed(seed);
            let mut world = SilenceWorld::new(2, 5, 2, 4, 0.1, 1.0, &mut rng);
            // Override supervisors and amplify per-agent fear directly.
            for team in world.teams.iter_mut() {
                team.supervisor_openness = supervisor_sign;
            }
            for emp in world.employees.values_mut() {
                emp.fear = (emp.fear + fear_floor).clamp(0.0, 1.0);
                // Slightly negative private concern for the silent-with-
                // concern climate to register.
                emp.private_concern = -0.4;
            }

            let mut reg = Registry::new();
            OrganizationalSilencePack.register(&mut reg);
            let p = Params::empty();
            let (rec, handle) = SharedRecorder::new();
            let mut builder = SimulationBuilder::new(world)
                .scheduler(Box::new(SequentialScheduler))
                .seed(seed)
                .recorder(Box::new(rec));
            for name in mechanism_names() {
                builder = builder.add_mechanism(reg.build(name, &p).unwrap());
            }
            let mut sim = builder.build();
            for _ in 0..10 {
                sim.step().unwrap();
            }
            // Average silence_rate across the run.
            let rec = handle.lock().unwrap();
            let series: Vec<f64> = rec
                .metrics()
                .iter()
                .filter(|r| r.key == "silence_rate")
                .map(|r| r.value)
                .collect();
            series.iter().sum::<f64>() / series.len().max(1) as f64
        }

        let hostile = run(-1.0, 0.5, 1);
        let open = run(1.0, 0.0, 1);
        assert!(
            hostile > open,
            "expected hostile silence_rate ({hostile}) > open silence_rate ({open})",
        );
    }

    #[test]
    fn cascade_event_records_when_flipping_mass_exceeds_threshold() {
        // Hand-craft a world where many agents are silent with negative
        // concern and low thresholds, and a few are already voicing — so the
        // cascade should flip a large fraction in one Interaction phase.
        let mut rng = SimRng::from_seed(99);
        let mut world = SilenceWorld::new(1, 10, 1, 4, 0.0, 1.0, &mut rng);
        for emp in world.employees.values_mut() {
            emp.expression = Expression::Silence;
            emp.private_concern = -0.5;
            emp.voice_threshold = 0.01;
        }
        // Pick the first two agents as initial voicers so neighbour-voice
        // ratios are nonzero for the rest.
        let voicer_ids: Vec<AgentId> = world.employees.keys().copied().take(2).collect();
        for id in &voicer_ids {
            if let Some(e) = world.employees.get_mut(id) {
                e.expression = Expression::Voice;
            }
        }

        let mut reg = Registry::new();
        OrganizationalSilencePack.register(&mut reg);
        let p = Params::empty();
        let (rec, handle) = SharedRecorder::new();
        let mech = reg.build("prefalse_cascade", &p).unwrap();
        let mut builder = SimulationBuilder::new(world)
            .scheduler(Box::new(SequentialScheduler))
            .seed(0)
            .recorder(Box::new(rec));
        builder = builder.add_mechanism(mech);
        let mut sim = builder.build();
        sim.step().unwrap();

        let rec = handle.lock().unwrap();
        let cascade_events: Vec<_> = rec
            .events()
            .iter()
            .filter(|e| e.kind == "cascade")
            .collect();
        assert!(
            !cascade_events.is_empty(),
            "expected at least one cascade event"
        );
    }

    #[test]
    fn classify_motive_picks_prosocial_when_open_supervisor_and_critical_concern() {
        let m = VoiceDecisionRuleMechanism::classify_motive(
            /* fear */ 0.9,
            /* ivt */ 0.1,
            /* supervisor_openness */ 0.5,
            /* private_concern */ -0.3,
        );
        assert_eq!(m, Motive::Prosocial);
    }

    #[test]
    fn classify_motive_picks_defensive_when_fear_dominates() {
        let m = VoiceDecisionRuleMechanism::classify_motive(0.8, 0.2, -0.5, -0.1);
        assert_eq!(m, Motive::Defensive);
    }

    #[test]
    fn classify_motive_picks_acquiescent_when_ivt_dominates() {
        let m = VoiceDecisionRuleMechanism::classify_motive(0.1, 0.7, -0.5, -0.1);
        assert_eq!(m, Motive::Acquiescent);
    }
}
