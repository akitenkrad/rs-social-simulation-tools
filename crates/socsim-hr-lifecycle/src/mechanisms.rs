//! All nine HR lifecycle mechanisms and the [`HrLifecyclePack`] registration
//! bundle.
//!
//! Each struct implements [`Mechanism<HrWorld>`] and is registered under its
//! canonical string name by [`HrLifecyclePack::register`].
//!
//! # Generalizability to `socsim-mechanisms`
//!
//! These mechanisms are calibrated against published empirical findings and
//! have deterministic, seeded tests, so any lift into the general
//! `socsim-mechanisms` crate must preserve the exact RNG-draw order. Current
//! status of each candidate:
//!
//! - `toxic_spread` — SI variant. **Not lifted.** It overlaps the general
//!   `si_contagion` kernel conceptually but draws RNG *source-first with no
//!   break-on-success* over sorted `AgentId`s, whereas `si_contagion` draws
//!   *target-first with break-on-first-success* over the scheduler order. The
//!   draw count/order differ, so delegating would change the seeded trajectory.
//!   `HrWorld` therefore does not impl `BinaryState`/`Neighbors`. Future
//!   candidate only if the kernel gains a source-first, no-break variant.
//! - `learning_curve` — temporal (tenure-driven) update. **Candidate** for a
//!   future shared temporal/decay kernel.
//! - `peer_effect` / `ocb` — both aggregate per-team state; lifting needs a
//!   future `GroupAggregates` capability trait in `socsim-core`.
//! - `knowledge_loss` — keys off employees removed this step; lifting needs a
//!   removal-event interface in `socsim-core`.

use rand::Rng;
use rand_distr::{Distribution, Normal};

use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{Mechanism, Phase, Result, StepContext};

use crate::{
    calibration::{
        ALPHA_K, ALPHA_PEER, BASE_QUIT_LOGIT, BETA_LOSS, KAPPA_LOSS, LAMBDA_LEARN, PHI_TACIT,
        P_SPREAD, P_TOXIC, QUIT_CASCADE_BUMP, QUIT_EMBED_SENS, QUIT_SAT_SENS, RHO_PJ, RHO_PO,
        RHO_PO_TURN, RHO_SI,
    },
    world::HrWorld,
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Clamp a value to [0, 1].
#[inline]
fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

// ── 1. LearningCurveMechanism (Environment) ───────────────────────────────────

/// Increment each employee's tenure and update their individual productivity
/// contribution via `π = θ · (1 − e^{−λ·tenure})`.
///
/// Calibration: Bahk & Gort (1993).
pub struct LearningCurveMechanism {
    lambda_learn: f64,
}

impl LearningCurveMechanism {
    /// Construct from params.  Param key: `lambda_learn` (default 0.15).
    pub fn from_params(p: &Params) -> Self {
        Self {
            lambda_learn: p.get_f64("lambda_learn", LAMBDA_LEARN),
        }
    }
}

impl Mechanism<HrWorld> for LearningCurveMechanism {
    fn name(&self) -> &str {
        "learning_curve"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Environment]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        for emp in ctx.world.employees.values_mut() {
            emp.tenure = emp.tenure.saturating_add(1);
            emp.productivity = emp.theta * (1.0 - (-self.lambda_learn * emp.tenure as f64).exp());
        }
        Ok(())
    }
}

// ── 2. PeerEffectMechanism (Interaction) ──────────────────────────────────────

/// Scale each employee's effective productivity by the team mean-θ ratio.
///
/// `π_eff = π · (1 + α_peer · (team_mean_θ / base_mean_θ))`
///
/// Calibration: Mas & Moretti (2009) — α_peer = 0.17.
pub struct PeerEffectMechanism {
    alpha_peer: f64,
}

impl PeerEffectMechanism {
    /// Construct from params.  Param key: `alpha_peer` (default 0.17).
    pub fn from_params(p: &Params) -> Self {
        Self {
            alpha_peer: p.get_f64("alpha_peer", ALPHA_PEER),
        }
    }
}

impl Mechanism<HrWorld> for PeerEffectMechanism {
    fn name(&self) -> &str {
        "peer_effect"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        let base = ctx.world.base_mean_theta;
        // Snapshot team mean thetas to avoid borrow issues.
        let team_means: Vec<f64> = ctx.world.teams.iter().map(|t| t.mean_theta).collect();

        for emp in ctx.world.employees.values_mut() {
            let team_mean = team_means.get(emp.team).copied().unwrap_or(base);
            let peer_factor = if base.abs() > 1e-9 {
                1.0 + self.alpha_peer * (team_mean / base)
            } else {
                1.0
            };
            emp.productivity *= peer_factor;
        }
        Ok(())
    }
}

// ── 3. OcbMechanism (Interaction) ─────────────────────────────────────────────

/// Organisational citizenship behaviour: adds knowledge to the team stock.
///
/// Per employee per month: `ΔK = α_k · satisfaction · po_fit`; summed over a
/// team this is the team's monthly OCB knowledge inflow.  `α_k` is a tunable
/// calibration scale (see [`crate::calibration::ALPHA_K`]), sized so the inflow
/// roughly balances the attrition outflow from `knowledge_loss` at steady
/// state — keeping `knowledge_stock` stable rather than collapsing.
pub struct OcbMechanism {
    alpha_k: f64,
}

impl OcbMechanism {
    /// Construct from params.  Param key: `alpha_k` (default 0.30).
    pub fn from_params(p: &Params) -> Self {
        Self {
            alpha_k: p.get_f64("alpha_k", ALPHA_K),
        }
    }
}

impl Mechanism<HrWorld> for OcbMechanism {
    fn name(&self) -> &str {
        "ocb"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        // Collect (team_idx, contribution) in sorted AgentId order for
        // deterministic f64 accumulation.
        let mut sorted: Vec<_> = ctx.world.employees.iter().collect();
        sorted.sort_by_key(|(id, _)| *id);
        let contribs: Vec<(usize, f64)> = sorted
            .iter()
            .map(|(_, emp)| {
                let contrib = self.alpha_k * emp.satisfaction * emp.po_fit;
                (emp.team, contrib)
            })
            .collect();

        for (team_idx, contrib) in contribs {
            if let Some(team) = ctx.world.teams.get_mut(team_idx) {
                team.knowledge_stock += contrib;
            }
        }
        Ok(())
    }
}

// ── 4. FitMechanism (Decision) ────────────────────────────────────────────────

/// Update each employee's satisfaction from P-O / P-J fit scores, and derive
/// a turnover-intent signal stored in `embeddedness` reduction.
///
/// Satisfaction update:
///   `sat_new = ρ_pj · pj_fit + ρ_po · po_fit`  (clamped to [0,1])
///
/// Calibration: Kristof-Brown et al. (2005) — ρ_PJ=0.20, ρ_PO=0.07.
pub struct FitMechanism {
    rho_pj: f64,
    rho_po: f64,
}

impl FitMechanism {
    /// Construct from params.
    pub fn from_params(p: &Params) -> Self {
        Self {
            rho_pj: p.get_f64("rho_pj", RHO_PJ),
            rho_po: p.get_f64("rho_po", RHO_PO),
        }
    }
}

impl Mechanism<HrWorld> for FitMechanism {
    fn name(&self) -> &str {
        "fit"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        for emp in ctx.world.employees.values_mut() {
            // Satisfaction is a weighted linear combination of fit dimensions.
            let new_sat = self.rho_pj * emp.pj_fit + self.rho_po * emp.po_fit;
            // Blend with previous satisfaction (moving average, weight 0.5).
            emp.satisfaction = clamp01(0.5 * emp.satisfaction + 0.5 * new_sat);
        }
        Ok(())
    }
}

// ── 5. TurnoverMechanism (Decision) ──────────────────────────────────────────

/// Per-month voluntary turnover with a low baseline hazard, fit/embeddedness
/// modulation, and a Krackhardt cluster-turnover cascade.
///
/// The monthly quit probability for employee j is a logistic in a quit-logit:
///
/// ```text
/// logit = BASE_QUIT_LOGIT                                  // ≈ −4.82 ⇒ ~0.8% baseline
///       + QUIT_EMBED_SENS · (1 − embeddedness)            // less embedded ⇒ more likely
///       + QUIT_SAT_SENS   · (1 − satisfaction)            // less satisfied ⇒ more likely
///       + ρ_po_turn       · po_fit                        // better PO fit ⇒ less likely (ρ<0)
///       + QUIT_CASCADE_BUMP · recent_quit_neighbors       // Krackhardt cascade (additive bump)
/// p_quit = logistic(logit)
/// ```
///
/// The baseline intercept dominates; embeddedness/satisfaction/fit and the
/// cascade are smaller modulations.  The `ρ_po_turn = −0.35` term is the
/// empirical PO-fit→turnover-intent correlation (Kristof-Brown 2005) used as a
/// standardized influence strength; the other terms are tunable calibration
/// scales documented in [`crate::calibration`].
pub struct TurnoverMechanism {
    rho_po_turn: f64,
    base_logit: f64,
    embed_sens: f64,
    sat_sens: f64,
    cascade_bump: f64,
}

impl TurnoverMechanism {
    /// Construct from params.  Param keys: `rho_po_turn` (−0.35),
    /// `base_quit_logit` (−4.82), `quit_embed_sens` (1.0), `quit_sat_sens`
    /// (0.8), `quit_cascade_bump` (0.30).
    pub fn from_params(p: &Params) -> Self {
        Self {
            rho_po_turn: p.get_f64("rho_po_turn", RHO_PO_TURN),
            base_logit: p.get_f64("base_quit_logit", BASE_QUIT_LOGIT),
            embed_sens: p.get_f64("quit_embed_sens", QUIT_EMBED_SENS),
            sat_sens: p.get_f64("quit_sat_sens", QUIT_SAT_SENS),
            cascade_bump: p.get_f64("quit_cascade_bump", QUIT_CASCADE_BUMP),
        }
    }
}

#[inline]
fn logistic(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

impl Mechanism<HrWorld> for TurnoverMechanism {
    fn name(&self) -> &str {
        "turnover"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        // Capture headcount before any removals for the turnover-rate denominator.
        ctx.world.headcount_at_step_start = ctx.world.employees.len();

        // Collect quit decisions before mutating world.
        let agent_order: Vec<_> = ctx.agent_order.to_vec();
        let mut quitters: Vec<_> = Vec::new();

        for &aid in &agent_order {
            if let Some(emp) = ctx.world.employees.get(&aid) {
                let logit = self.base_logit
                    + self.embed_sens * (1.0 - emp.embeddedness)
                    + self.sat_sens * (1.0 - emp.satisfaction)
                    + self.rho_po_turn * emp.po_fit
                    + self.cascade_bump * emp.recent_quit_neighbors as f64;
                let p_quit = logistic(logit);
                if ctx.rng.gen::<f64>() < p_quit {
                    quitters.push((aid, emp.theta, emp.tenure, emp.team));
                }
            }
        }

        // Reset the cascade counter for everyone; it is re-accumulated below for
        // this month's quitters' neighbours and consumed next month.
        for emp in ctx.world.employees.values_mut() {
            emp.recent_quit_neighbors = 0;
        }

        for (aid, theta, tenure, team) in &quitters {
            // Krackhardt cascade: each quit raises surviving neighbours' quit
            // intent next month (additive bump) and slightly lowers embeddedness.
            let neighbours = ctx.world.network.neighbors(*aid);
            for nb in neighbours {
                if let Some(ne) = ctx.world.employees.get_mut(&nb) {
                    ne.recent_quit_neighbors = ne.recent_quit_neighbors.saturating_add(1);
                    ne.embeddedness = clamp01(ne.embeddedness - 0.02);
                }
            }

            // Remove from world.
            ctx.world.employees.remove(aid);
            ctx.world.network.remove_node(*aid);
            ctx.world
                .departed_this_step
                .push((*aid, *theta, *tenure, *team));

            // Record turnover event.
            ctx.recorder.record_event(
                ctx.clock.t(),
                "turnover",
                serde_json::json!({
                    "agent_id": aid.0,
                    "theta": theta,
                    "tenure": tenure,
                }),
            );
        }

        Ok(())
    }
}

// ── 6. KnowledgeLossMechanism (PostStep) ─────────────────────────────────────

/// When an employee departs, decrement *their own team's* knowledge stock by
/// the tacit-knowledge they carried away.
///
/// ```text
/// years = tenure_months / 12
/// ΔK    = −KAPPA_LOSS · φ_tacit · θ · years^β
/// ```
///
/// Expressing tenure in years (not months) and scaling by the tunable
/// [`crate::calibration::KAPPA_LOSS`] keeps a typical leaver's drain comparable
/// to a few months of team OCB inflow, so `knowledge_stock` does not collapse.
/// `φ_tacit = 0.85` is the empirical tacit-knowledge ratio (Nonaka 1994).
pub struct KnowledgeLossMechanism {
    phi_tacit: f64,
    beta: f64,
    kappa: f64,
}

impl KnowledgeLossMechanism {
    /// Construct from params.  Keys: `phi_tacit` (0.85), `beta_loss` (1.0),
    /// `kappa_loss` (0.40).
    pub fn from_params(p: &Params) -> Self {
        Self {
            phi_tacit: p.get_f64("phi_tacit", PHI_TACIT),
            beta: p.get_f64("beta_loss", BETA_LOSS),
            kappa: p.get_f64("kappa_loss", KAPPA_LOSS),
        }
    }
}

impl Mechanism<HrWorld> for KnowledgeLossMechanism {
    fn name(&self) -> &str {
        "knowledge_loss"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        // departed_this_step: (id, theta, tenure_months, team_idx).
        let departed: Vec<(f64, u32, usize)> = ctx
            .world
            .departed_this_step
            .iter()
            .map(|(_, theta, tenure, team)| (*theta, *tenure, *team))
            .collect();

        for (theta, tenure, team_idx) in departed {
            let years = tenure as f64 / 12.0;
            let loss = self.kappa * self.phi_tacit * theta.abs() * years.powf(self.beta);
            if let Some(team) = ctx.world.teams.get_mut(team_idx) {
                team.knowledge_stock = (team.knowledge_stock - loss).max(0.0);
            }
        }

        // Clear the departed list — PostStep is the last phase to run.
        ctx.world.departed_this_step.clear();

        Ok(())
    }
}

// ── 7. ToxicSpreadMechanism (Interaction) ────────────────────────────────────

/// Toxic contagion along network edges.
///
/// For each toxic employee, each non-toxic neighbour becomes toxic with
/// probability `p_spread`.
///
/// Calibration: Housman & Minor (2015) — p_spread = 0.46.
pub struct ToxicSpreadMechanism {
    p_spread: f64,
}

impl ToxicSpreadMechanism {
    /// Construct from params.  Key: `p_spread` (0.46).
    pub fn from_params(p: &Params) -> Self {
        Self {
            p_spread: p.get_f64("p_spread", P_SPREAD),
        }
    }
}

impl Mechanism<HrWorld> for ToxicSpreadMechanism {
    fn name(&self) -> &str {
        "toxic_spread"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        // Collect toxic employees in sorted AgentId order for deterministic RNG use.
        let mut toxic_ids: Vec<_> = ctx
            .world
            .employees
            .iter()
            .filter(|(_, e)| e.is_toxic)
            .map(|(id, _)| *id)
            .collect();
        toxic_ids.sort();

        let mut to_infect: Vec<_> = Vec::new();

        for tid in toxic_ids {
            // Sort neighbours for deterministic RNG consumption.
            let mut neighbours = ctx.world.network.neighbors(tid);
            neighbours.sort();
            for nb in neighbours {
                if let Some(ne) = ctx.world.employees.get(&nb) {
                    if !ne.is_toxic && ctx.rng.gen::<f64>() < self.p_spread {
                        to_infect.push(nb);
                    }
                }
            }
        }

        for aid in to_infect {
            if let Some(emp) = ctx.world.employees.get_mut(&aid) {
                emp.is_toxic = true;
            }
        }

        Ok(())
    }
}

// ── 8. HiringMechanism (Decision) ─────────────────────────────────────────────

/// Refill teams to target headcount with positive-scale ability draws.
///
/// True ability is drawn on the positive scale `θ ~ N(THETA_MEAN, THETA_SD)`
/// (floored at `THETA_FLOOR`).  The selection process observes θ through a
/// validity-`ρ_SI` signal `signal = ρ·z_θ + √(1−ρ²)·ε` (a standardized
/// influence strength, Schmidt & Hunter 1998); the candidate is currently
/// hired unconditionally (the signal is recorded for future selection gates).
pub struct HiringMechanism {
    rho_si: f64,
    p_toxic: f64,
}

impl HiringMechanism {
    /// Construct from params.  Keys: `rho_si` (0.51), `p_toxic` (0.04).
    pub fn from_params(p: &Params) -> Self {
        Self {
            rho_si: p.get_f64("rho_si", RHO_SI),
            p_toxic: p.get_f64("p_toxic", P_TOXIC),
        }
    }
}

impl Mechanism<HrWorld> for HiringMechanism {
    fn name(&self) -> &str {
        "hiring"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        let normal = Normal::new(0.0, 1.0).map_err(|e| {
            socsim_core::SocsimError::Mechanism(format!("normal distribution error: {e}"))
        })?;

        let n_teams = ctx.world.teams.len();
        let target = ctx.world.target_team_size;

        // Count current headcount per team.
        let mut team_counts = vec![0usize; n_teams];
        for emp in ctx.world.employees.values() {
            if emp.team < n_teams {
                team_counts[emp.team] += 1;
            }
        }

        let mut new_ids: Vec<(crate::world::Employee, socsim_core::AgentId)> = Vec::new();

        for (team_idx, count) in team_counts.iter_mut().enumerate() {
            while *count < target {
                // Draw true ability on the positive scale.
                let true_theta = crate::world::Employee::draw_theta(&normal, ctx.rng);
                // Selection signal with validity ρ (standardized; recorded for
                // future selection gates, hire is unconditional for now).
                let z_theta =
                    (true_theta - crate::calibration::THETA_MEAN) / crate::calibration::THETA_SD;
                let noise: f64 = normal.sample(ctx.rng);
                let noise_scale = (1.0 - self.rho_si * self.rho_si).sqrt();
                let _signal = self.rho_si * z_theta + noise_scale * noise;

                let is_toxic = ctx.rng.gen::<f64>() < self.p_toxic;
                let id = ctx.world.alloc_id();
                let emp = crate::world::Employee::new(true_theta, team_idx, is_toxic, ctx.rng);
                new_ids.push((emp, id));
                *count += 1;
            }
        }

        for (emp, id) in new_ids {
            ctx.world.network.add_node(id);
            // Connect new hire to up to 2 existing team members.
            let team_members = ctx.world.team_members(emp.team);
            for &member in team_members.iter().take(2) {
                ctx.world.network.add_edge(id, member);
            }
            ctx.world.new_hires_this_step.push(id);
            ctx.world.employees.insert(id, emp);
            ctx.recorder.record_event(
                ctx.clock.t(),
                "hiring",
                serde_json::json!({ "agent_id": id.0 }),
            );
        }

        Ok(())
    }
}

// ── 9. SocializationMechanism (PostStep) ─────────────────────────────────────

/// For new hires admitted this step, initialise socialisation quality and
/// adjust embeddedness.
///
/// Socialisation quality is a function of po_fit and a random draw representing
/// supervisor/team support.
pub struct SocializationMechanism;

impl SocializationMechanism {
    /// Construct (no params needed).
    pub fn from_params(_p: &Params) -> Self {
        Self
    }
}

impl Mechanism<HrWorld> for SocializationMechanism {
    fn name(&self) -> &str {
        "socialization"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        let new_hires: Vec<_> = ctx.world.new_hires_this_step.drain(..).collect();

        for id in new_hires {
            if let Some(emp) = ctx.world.employees.get_mut(&id) {
                // Supervisor / team support: random draw in [0.4, 1.0].
                let support: f64 = ctx.rng.gen_range(0.4..1.0);
                emp.socialization = clamp01(0.5 * emp.po_fit + 0.5 * support);
                emp.embeddedness = clamp01(emp.embeddedness + 0.1 * emp.socialization);
            }
        }

        Ok(())
    }
}

// ── 10. OrgPerformanceMechanism (Reward) ────────────────────────────────────

/// Aggregate effective productivity and record the key metrics.
///
/// Metrics recorded each step:
/// - `org_performance`: sum of effective productivity across all employees.
/// - `avg_tenure`: mean tenure in months.
/// - `turnover_rate`: fraction of agents who departed this step.
/// - `knowledge_stock`: sum of team knowledge stocks.
pub struct OrgPerformanceMechanism;

impl OrgPerformanceMechanism {
    /// Construct (no params).
    pub fn from_params(_p: &Params) -> Self {
        Self
    }
}

impl Mechanism<HrWorld> for OrgPerformanceMechanism {
    fn name(&self) -> &str {
        "org_performance"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Reward]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, HrWorld>) -> Result<()> {
        // Sort by AgentId for deterministic f64 summation order.
        let mut sorted: Vec<_> = ctx.world.employees.iter().collect();
        sorted.sort_by_key(|(id, _)| *id);
        let perf: f64 = sorted.iter().map(|(_, e)| e.productivity).sum();
        ctx.world.org_performance = perf;

        // turnover_rate = quits this month / headcount at the start of the month
        // (captured by TurnoverMechanism before any removals).
        let departed = ctx.world.departed_this_step.len();
        let denom = ctx.world.headcount_at_step_start.max(1) as f64;
        let turnover_rate = departed as f64 / denom;

        let avg_tenure = ctx.world.avg_tenure();
        let knowledge = ctx.world.total_knowledge_stock();

        let t = ctx.clock.t();
        ctx.recorder.record_metric(t, "org_performance", perf);
        ctx.recorder.record_metric(t, "avg_tenure", avg_tenure);
        ctx.recorder
            .record_metric(t, "turnover_rate", turnover_rate);
        ctx.recorder.record_metric(t, "knowledge_stock", knowledge);

        // Recompute team means after all hiring/turnover for next step.
        ctx.world.recompute_team_means();

        Ok(())
    }
}

// ── HrLifecyclePack ───────────────────────────────────────────────────────────

/// [`ModulePack`] that registers all HR lifecycle mechanisms into a
/// [`Registry<HrWorld>`].
///
/// Call [`HrLifecyclePack.register`] to make all mechanisms available by name,
/// then build them individually or iterate over all names:
///
/// ```rust,no_run
/// use socsim_config::{Registry, Params, ModulePack};
/// use socsim_hr_lifecycle::HrLifecyclePack;
/// use socsim_hr_lifecycle::HrWorld;
///
/// let mut reg: Registry<HrWorld> = Registry::new();
/// HrLifecyclePack.register(&mut reg);
/// // reg.build("hiring", &Params::empty()) …
/// ```
pub struct HrLifecyclePack;

impl ModulePack<HrWorld> for HrLifecyclePack {
    fn pack_name(&self) -> &str {
        "hr-lifecycle"
    }

    fn register(&self, reg: &mut Registry<HrWorld>) {
        reg.register("learning_curve", |p| {
            Ok(Box::new(LearningCurveMechanism::from_params(p)))
        });
        reg.register("peer_effect", |p| {
            Ok(Box::new(PeerEffectMechanism::from_params(p)))
        });
        reg.register("ocb", |p| Ok(Box::new(OcbMechanism::from_params(p))));
        reg.register("fit", |p| Ok(Box::new(FitMechanism::from_params(p))));
        reg.register("turnover", |p| {
            Ok(Box::new(TurnoverMechanism::from_params(p)))
        });
        reg.register("knowledge_loss", |p| {
            Ok(Box::new(KnowledgeLossMechanism::from_params(p)))
        });
        reg.register("toxic_spread", |p| {
            Ok(Box::new(ToxicSpreadMechanism::from_params(p)))
        });
        reg.register("hiring", |p| Ok(Box::new(HiringMechanism::from_params(p))));
        reg.register("socialization", |p| {
            Ok(Box::new(SocializationMechanism::from_params(p)))
        });
        reg.register("org_performance", |p| {
            Ok(Box::new(OrgPerformanceMechanism::from_params(p)))
        });
    }
}
