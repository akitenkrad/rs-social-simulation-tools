//! Calibration constants for the HR lifecycle ABM.
//!
//! Every constant carries a doc-comment citing the empirical source so that
//! researchers can trace each parameter back to the literature.

/// Structured-interview selection validity (correlation with job performance).
///
/// Source: Schmidt & Hunter (1998).
pub const RHO_SI: f64 = 0.51;

/// General mental ability (GMA) correlation with job performance.
///
/// Source: Schmidt & Hunter (1998).
pub const RHO_GMA: f64 = 0.51;

/// Unstructured-interview selection validity.
///
/// Source: Schmidt & Hunter (1998).
pub const RHO_UI: f64 = 0.38;

/// Peer-effect multiplier on effective productivity.
///
/// Source: Mas & Moretti (2009).
pub const ALPHA_PEER: f64 = 0.17;

/// Baseline proportion of toxic workers in the labour market.
///
/// Source: Housman & Minor (2015).
pub const P_TOXIC: f64 = 0.04;

/// Per-step probability that a non-toxic employee adjacent to a toxic
/// employee becomes toxic.
///
/// Source: Housman & Minor (2015).
pub const P_SPREAD: f64 = 0.46;

/// Tacit-knowledge ratio: fraction of an employee's knowledge stock that is
/// tacit (lost on departure).
///
/// Source: Nonaka (1994).
pub const PHI_TACIT: f64 = 0.85;

/// PJ-fit correlation with job performance / satisfaction.
///
/// Source: Kristof-Brown et al. (2005).
pub const RHO_PJ: f64 = 0.20;

/// PO-fit correlation with job performance / satisfaction.
///
/// Source: Kristof-Brown et al. (2005).
pub const RHO_PO: f64 = 0.07;

/// PO-fit correlation with turnover intent (negative = better fit → less
/// likely to quit).
///
/// Source: Kristof-Brown et al. (2005).
pub const RHO_PO_TURN: f64 = -0.35;

/// Tenure-based learning-curve growth rate (industry-average default).
///
/// Source: Bahk & Gort (1993).  Set to the midpoint of the reported range.
pub const LAMBDA_LEARN: f64 = 0.15;

/// OCB knowledge-stock contribution coefficient.
///
/// **Calibration scale (tunable), NOT an empirical correlation.** Controls how
/// much each unit of `satisfaction * po_fit` an employee contributes to their
/// team's knowledge stock per month.  Summed over a team, this is the monthly
/// OCB inflow.  Tuned so that at steady state the inflow roughly matches the
/// attrition outflow from `knowledge_loss`, keeping `knowledge_stock` stable at
/// the same order of magnitude as its initial value.
pub const ALPHA_K: f64 = 0.30;

/// Knowledge-loss power parameter (exponent on tenure-in-years).
///
/// **Calibration scale (tunable).** Applied to tenure expressed in *years*
/// (`tenure_months / 12`) so a departing veteran removes more tacit knowledge
/// than a recent hire, but on a sane scale (years, not months).
pub const BETA_LOSS: f64 = 1.0;

/// Knowledge-loss magnitude coefficient.
///
/// **Calibration scale (tunable), NOT an empirical correlation.** Scales the
/// per-leaver knowledge drain `KAPPA_LOSS · φ_tacit · θ · years^β`.  Tuned so a
/// typical leaver removes an amount comparable to a few months of team OCB
/// inflow — not orders of magnitude more.
pub const KAPPA_LOSS: f64 = 0.40;

/// Baseline monthly voluntary-quit hazard (probability), before any modulation
/// by embeddedness / satisfaction / fit.
///
/// **Calibration scale (tunable), NOT an empirical correlation.** Set to
/// ~0.8%/month; together with the (positive) embeddedness/satisfaction/fit
/// modulation around it this yields a realised mean monthly hazard of roughly
/// 1.5–2%/month (≈ 15–22%/year, `1 − (1 − p)^12`).  Converted to a logit
/// intercept in the turnover logistic via [`BASE_QUIT_LOGIT`].
pub const BASE_MONTHLY_QUIT_HAZARD: f64 = 0.008;

/// Logit intercept corresponding to [`BASE_MONTHLY_QUIT_HAZARD`].
///
/// **Calibration scale (tunable).** `logit(0.008) ≈ −4.82`.  This is the
/// dominant (baseline) term of the turnover logistic; embeddedness,
/// satisfaction, fit and the Krackhardt cascade act as smaller modulations
/// around it.
pub const BASE_QUIT_LOGIT: f64 = -4.82;

/// Sensitivity of the monthly quit logit to (1 − embeddedness).
///
/// **Calibration scale (tunable).** A fully un-embedded employee (embeddedness
/// = 0) gets at most `+QUIT_EMBED_SENS` added to the baseline logit.
pub const QUIT_EMBED_SENS: f64 = 1.0;

/// Sensitivity of the monthly quit logit to (1 − satisfaction).
///
/// **Calibration scale (tunable).** A fully dissatisfied employee gets at most
/// `+QUIT_SAT_SENS` added to the baseline logit.
pub const QUIT_SAT_SENS: f64 = 0.8;

/// Per-quit-neighbour additive bump to a colleague's quit logit (Krackhardt
/// cluster-turnover cascade).
///
/// **Calibration scale (tunable).** Each colleague who quit in the *previous*
/// month nudges an employee's quit logit upward by this amount — a smaller,
/// additive contagion term rather than the dominant driver.
pub const QUIT_CASCADE_BUMP: f64 = 0.30;

/// Mean true ability `θ` at hiring (positive scale).
///
/// **Calibration scale (tunable).** Employees are drawn `θ ~ N(THETA_MEAN,
/// THETA_SD)` and floored at [`THETA_FLOOR`] so productivity and the
/// peer-effect ratio `team_mean/base_mean` are well-behaved and positive.
pub const THETA_MEAN: f64 = 1.0;

/// Standard deviation of true ability `θ` at hiring.
///
/// **Calibration scale (tunable).**
pub const THETA_SD: f64 = 0.2;

/// Lower floor for true ability `θ` (keeps it strictly positive).
///
/// **Calibration scale (tunable).**
pub const THETA_FLOOR: f64 = 0.1;

/// Turnover cost as a fraction of one annual salary (midpoint of range).
///
/// Source: Allen (2008).
pub const C_TURN: f64 = 1.25;
