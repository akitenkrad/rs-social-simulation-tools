//! Calibration constants for the organizational-silence ABM.
//!
//! Every constant carries a doc-comment citing the empirical / theoretical
//! source so researchers can trace each parameter back to the literature.
//! The numbers here are taken from §9 of the design document
//! `組織的沈黙のLLM-Agentシミュレーション設計.md`.
//!
//! Two kinds of constants live here:
//!
//! 1. **Logistic coefficients** (`BETA_*`) of the voice-decision logit
//!    `P(VOICE) = logit^{-1}(β_0 + β_ψ·ψ + β_u·u + β_σ·σ − β_f·f − β_ι·ι − β_C·ρ)`,
//!    each cited to its source paper.
//! 2. **Initial-distribution / update-rate scales** that govern the prior
//!    distributions of `fear`, `psych_safety`, `voice_threshold` and the
//!    update rates of `retaliation`, `psych_safety`, `silence_spiral`.
//!
//! The two empirical anchors at the bottom (`EVER_SILENT_TARGET`,
//! `HICO_SILENCE_TARGET`) are *calibration targets*, not parameters: a baseline
//! run should reproduce them within reasonable error.

// ── Logistic coefficients for the voice-decision logit ───────────────────────

/// Coefficient on perceived psychological safety ψ in the voice logit.
///
/// Source: Edmondson (1999).  Psychological safety is the canonical positive
/// predictor of voice / learning behaviour in work teams.
pub const BETA_PSAFETY: f64 = 1.2;

/// Coefficient on fear f in the voice logit (negated on use).
///
/// Source: Kish-Gephart et al. (2009).  Fear is the central driver of
/// silence; the logit subtracts `β_f · f`.
pub const BETA_FEAR: f64 = 1.5;

/// Coefficient on implicit-voice-theory strength ι in the voice logit
/// (negated on use).
///
/// Source: Detert & Edmondson (2011).  IVT is an automatic, lifelong-internalised
/// premise that "speaking up is risky", suppressing voice independently of
/// situational fear.
pub const BETA_IVT: f64 = 0.8;

/// Coefficient on supervisor openness u in the voice logit.
///
/// Source: Detert & Burris (2007) / Morrison (2014) — supervisor / leader
/// openness is the dominant proximal cue agents read before deciding to voice.
pub const BETA_SUP: f64 = 1.0;

/// Coefficient on issue salience σ in the voice logit.
///
/// Source: Morrison (2014).  A more salient / serious issue raises the
/// expected utility of voicing it.
pub const BETA_SALIENCE: f64 = 1.0;

/// Coefficient on neighbour silence ratio ρ in the voice logit (negated on use).
///
/// Source: Noelle-Neumann (1974).  The spiral of silence: agents who perceive
/// themselves in a silent majority become more reluctant to voice.  The
/// coefficient is the largest negative term to reflect the gravity of climate
/// effects observed in organisational ABM studies (Sohn 2022).
pub const BETA_CLIMATE: f64 = 1.5;

/// Baseline (intercept) logit for voicing.
///
/// **Calibration scale (tunable).**  Set slightly negative so a wholly average
/// agent in a neutral environment skews very mildly toward silence — matching
/// Milliken et al.'s (2003) finding that "85% of employees withheld at least
/// one concern".
pub const BETA_0: f64 = -0.5;

// ── Prior distributions for per-agent initial state ──────────────────────────

/// Mean of the initial fear-traitedness draw `f_i ~ N(F_MEAN, F_SD)`.
///
/// Source: Kish-Gephart et al. (2009).  Empirical fear-at-work scales centre
/// near the lower-middle of the [0, 1] range in healthy organisations.
pub const F_MEAN: f64 = 0.4;

/// Standard deviation of the initial fear-traitedness draw.
///
/// Source: Kish-Gephart et al. (2009).
pub const F_SD: f64 = 0.2;

/// Mean of the initial psychological-safety draw `ψ_i ~ N(PSAFETY_MEAN, PSAFETY_SD)`.
///
/// Source: Edmondson (1999) — Likert-style psychological-safety scales
/// typically centre around the midpoint in mixed-climate samples.
pub const PSAFETY_MEAN: f64 = 0.5;

/// Standard deviation of the initial psychological-safety draw.
///
/// Source: Edmondson (1999).
pub const PSAFETY_SD: f64 = 0.2;

/// Mean of the initial voice-threshold draw `θ_i ~ N(THETA_VOICE_MEAN,
/// THETA_VOICE_SD)`.
///
/// Source: Kuran (1995).  Granovetter-style threshold models populate the
/// threshold distribution near the lower-middle of [0, 1] to allow cascades.
pub const THETA_VOICE_MEAN: f64 = 0.4;

/// Standard deviation of the initial voice-threshold draw.
///
/// Source: Kuran (1995).
pub const THETA_VOICE_SD: f64 = 0.15;

// ── Update rates / event probabilities ───────────────────────────────────────

/// Per-step probability that a retaliation event is fired.
///
/// Source: Kish-Gephart et al. (2009).  Calibrated low so retaliation acts as
/// a punctuated shock rather than a constant drag.
pub const P_RETALIATE: f64 = 0.05;

/// Additive fear update for an agent whose neighbour was retaliated against
/// this step.
///
/// **Calibration scale (tunable).**  Combined with a small per-step decay
/// (configured inside `FearAppraisalMechanism`), this yields a slow climb in
/// fear under repeated retaliation events.
pub const FEAR_SENSITIVITY: f64 = 0.4;

/// Half-radius of the perceived-silence sliding window (the "spiral
/// perception" parameter).
///
/// Source: Noelle-Neumann (1974) — the spiral acts via agents *misperceiving*
/// majority opinion; this is the magnitude of the local-perception nudge to
/// `psych_safety` per silent neighbour ratio.
pub const EPSILON_SPIRAL: f64 = 0.25;

/// Learning rate for the psychological-safety update.
///
/// Source: Edmondson (1999).  Each step, an employee who voices nudges ψ up;
/// each step an employee is retaliated against nudges ψ down.
pub const PSAFETY_LEARN: f64 = 0.1;

// ── Issue-salience trajectory ────────────────────────────────────────────────

/// Baseline issue salience σ(t) at t < `SHOCK_T`.
///
/// **Calibration scale (tunable).**  At baseline salience the issue is mildly
/// noticeable; the t=`SHOCK_T` jump represents a triggering event (e.g. a
/// public accusation, regulator inquiry).
pub const SIGMA_BASE: f64 = 0.3;

/// Default tick at which the salience shock fires.
///
/// **Calibration scale (tunable).**  Set to month 24 of a 60-month run so the
/// system has had time to settle into a climate equilibrium first.
pub const SHOCK_T: u64 = 24;

/// Magnitude of the salience shock at `SHOCK_T`.
///
/// **Calibration scale (tunable).**  +0.4 lifts σ from `SIGMA_BASE` ≈ 0.3 to
/// ≈ 0.7, large enough to flip many marginal voice-vs-silence agents.
pub const SHOCK_DELTA: f64 = 0.4;

// ── Empirical calibration anchors (targets, not parameters) ──────────────────

/// Target fraction of employees who stayed silent on at least one issue
/// during a 6-month window.
///
/// Source: Milliken, Morrison & Hewlin (2003), Table 1.  The 85% figure is the
/// canonical face-validity check for any silence ABM.
pub const EVER_SILENT_TARGET: f64 = 0.85;

/// Target silence rate among agents high on implicit voice theory (HiCo).
///
/// Source: Detert & Edmondson (2011).  About half of HiCo employees report
/// withholding voice on at least one concern in a typical month.
pub const HICO_SILENCE_TARGET: f64 = 0.50;
