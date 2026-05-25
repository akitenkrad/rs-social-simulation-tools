//! Averaging operators for opinion aggregation.
//!
//! Hegselmann & Krause (2005) generalised the 2002 HK model along the axis of
//! *which kind of mean* is used to aggregate the opinions within an agent's
//! confidence set.  This module reproduces the paper's five means (A/G/H/P/R)
//! as an explicit [`MeanOperator`] enum and concentrates their application to
//! an opinion multiset in [`apply_mean`], so the paper's "various ways of
//! averaging" becomes expressible as a type.
//!
//! Systematic inequality (paper §2), for positive inputs:
//! `P_{-∞}(=min) ≤ H=P_{-1} ≤ G=P_0 ≤ A=P_1 ≤ P_p ≤ P_{∞}(=max)  (p ≥ 1)`
//!
//! Ported verbatim (math-identical) from the `hegselmann2005` replication's
//! `simulation/src/means.rs`.

use rand::Rng;
use socsim_core::SimRng;

/// An averaging strategy used to aggregate the opinions inside a confidence set.
///
/// A/G/H/P are all deterministic (do not touch the RNG).  Only R uses the RNG.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeanOperator {
    /// Arithmetic mean `A = P_1`.  The base rule of the 2002 HK model.
    Arithmetic,
    /// Geometric mean `G = P_0 = lim_{p→0} P_p` (requires `vals > 0`).
    Geometric,
    /// Harmonic mean `H = P_{-1}` (requires `vals > 0`).
    Harmonic,
    /// Power (Hölder) mean `P_p`, `p ≠ 0`.
    Power(f64),
    /// Random mean `R = Uniform(min S, max S)`.  The only RNG-using operator.
    Random,
}

impl MeanOperator {
    /// A short label for CLI / logging output.
    pub fn label(&self) -> String {
        match *self {
            MeanOperator::Arithmetic => "A".to_string(),
            MeanOperator::Geometric => "G".to_string(),
            MeanOperator::Harmonic => "H".to_string(),
            MeanOperator::Power(p) => format!("P{}", p),
            MeanOperator::Random => "R".to_string(),
        }
    }

    /// Whether this mean requires strictly positive inputs (`vals > 0`).
    ///
    /// Geometric and harmonic means are undefined / divergent at zero, so they
    /// require an open interval.
    pub fn requires_positive(&self) -> bool {
        matches!(self, MeanOperator::Geometric | MeanOperator::Harmonic)
    }

    /// Whether this mean is deterministic (does not use the RNG).
    ///
    /// Deterministic means reach a fixed point, so a driver can stop on a
    /// convergence test; the random mean cannot.
    pub fn is_deterministic(&self) -> bool {
        !matches!(self, MeanOperator::Random)
    }
}

impl Default for MeanOperator {
    /// Arithmetic mean — the canonical 2002 HK rule.
    fn default() -> Self {
        MeanOperator::Arithmetic
    }
}

/// Parse a [`MeanOperator`] from a string.
///
/// Accepted forms:
/// - `"A"` → Arithmetic
/// - `"G"` → Geometric
/// - `"H"` → Harmonic
/// - `"R"` → Random
/// - `"P<p>"` (e.g. `"P0.01"`, `"P100"`, `"P-1"`) → Power(p)
/// - `"P"` → Power(`p_fallback`), for a `--mean P --p 100` style split flag.
///
/// `p_fallback` is the default exponent used for a bare `"P"` (no exponent).
pub fn parse_mean(s: &str, p_fallback: f64) -> Result<MeanOperator, String> {
    let s = s.trim();
    match s {
        "A" | "a" => Ok(MeanOperator::Arithmetic),
        "G" | "g" => Ok(MeanOperator::Geometric),
        "H" | "h" => Ok(MeanOperator::Harmonic),
        "R" | "r" => Ok(MeanOperator::Random),
        "P" | "p" => {
            if p_fallback == 0.0 {
                return Err("power mean P requires p ≠ 0 (specify via --p)".to_string());
            }
            Ok(MeanOperator::Power(p_fallback))
        }
        _ => {
            // "P<p>" form.
            if let Some(rest) = s.strip_prefix('P').or_else(|| s.strip_prefix('p')) {
                let p: f64 = rest
                    .parse()
                    .map_err(|_| format!("failed to parse power-mean exponent: \"{}\"", s))?;
                if p == 0.0 {
                    return Err(
                        "power mean P must have p ≠ 0 (p=0 is the geometric mean G)".to_string()
                    );
                }
                Ok(MeanOperator::Power(p))
            } else {
                Err(format!(
                    "invalid mean spec: \"{}\" (one of A / G / H / P<p> / R)",
                    s
                ))
            }
        }
    }
}

/// Apply an averaging operator to the confidence-set opinions `vals`.
///
/// `vals` is the multiset of opinions of the agents in the confidence set
/// `I(i, x)` (including the agent itself).  It is assumed non-empty (in a
/// bounded-confidence model an agent is always in its own confidence set).
/// Only the random mean R uses `rng`; all others ignore it (deterministic).
///
/// # Panics
/// If `vals` is empty.  (In debug builds, also if a geometric/harmonic mean is
/// given a non-positive value.)
pub fn apply_mean(op: MeanOperator, vals: &[f64], rng: &mut SimRng) -> f64 {
    debug_assert!(
        !vals.is_empty(),
        "the confidence set must be non-empty (it includes the agent itself)"
    );
    let m = vals.len() as f64;
    match op {
        MeanOperator::Arithmetic => vals.iter().sum::<f64>() / m,
        MeanOperator::Geometric => {
            // G = (Π s)^{1/m} = exp( (1/m) Σ ln s ).  Computed in log space to
            // avoid underflow.
            let sum_ln: f64 = vals.iter().map(|&s| s.ln()).sum();
            (sum_ln / m).exp()
        }
        MeanOperator::Harmonic => {
            // H = m / Σ (1/s)
            let sum_inv: f64 = vals.iter().map(|&s| 1.0 / s).sum();
            m / sum_inv
        }
        MeanOperator::Power(p) => {
            // P_p = ( (1/m) Σ s^p )^{1/p}  (p ≠ 0)
            let sum_pow: f64 = vals.iter().map(|&s| s.powf(p)).sum();
            (sum_pow / m).powf(1.0 / p)
        }
        MeanOperator::Random => {
            // R = Uniform(min S, max S).  Returns the value when min == max
            // (degenerate interval).
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for &s in vals {
                if s < lo {
                    lo = s;
                }
                if s > hi {
                    hi = s;
                }
            }
            if hi <= lo {
                lo
            } else {
                rng.gen_range(lo..hi)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use socsim_core::SimRng;

    fn rng() -> SimRng {
        SimRng::from_seed(0)
    }

    #[test]
    fn arithmetic_mean_is_average() {
        let v = [1.0, 2.0, 3.0];
        assert!((apply_mean(MeanOperator::Arithmetic, &v, &mut rng()) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn geometric_mean_of_equal_values() {
        let v = [0.5, 0.5, 0.5];
        assert!((apply_mean(MeanOperator::Geometric, &v, &mut rng()) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn harmonic_mean_known_value() {
        // H(1, 2) = 2 / (1 + 0.5) = 4/3
        let v = [1.0, 2.0];
        assert!((apply_mean(MeanOperator::Harmonic, &v, &mut rng()) - 4.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn power_one_equals_arithmetic() {
        let v = [0.1, 0.4, 0.7];
        let a = apply_mean(MeanOperator::Arithmetic, &v, &mut rng());
        let p1 = apply_mean(MeanOperator::Power(1.0), &v, &mut rng());
        assert!((a - p1).abs() < 1e-12);
    }

    #[test]
    fn systematic_inequality_holds() {
        // H ≤ G ≤ A ≤ P_2 for positive values.
        let v = [0.2, 0.5, 0.9];
        let h = apply_mean(MeanOperator::Harmonic, &v, &mut rng());
        let g = apply_mean(MeanOperator::Geometric, &v, &mut rng());
        let a = apply_mean(MeanOperator::Arithmetic, &v, &mut rng());
        let p2 = apply_mean(MeanOperator::Power(2.0), &v, &mut rng());
        assert!(h <= g + 1e-12);
        assert!(g <= a + 1e-12);
        assert!(a <= p2 + 1e-12);
    }

    #[test]
    fn random_mean_within_minmax() {
        let v = [0.2, 0.5, 0.9];
        let mut r = rng();
        for _ in 0..1000 {
            let x = apply_mean(MeanOperator::Random, &v, &mut r);
            assert!((0.2..=0.9).contains(&x), "R out of range: {}", x);
        }
    }

    #[test]
    fn parse_mean_variants() {
        assert_eq!(parse_mean("A", 0.0).unwrap(), MeanOperator::Arithmetic);
        assert_eq!(parse_mean("G", 0.0).unwrap(), MeanOperator::Geometric);
        assert_eq!(parse_mean("H", 0.0).unwrap(), MeanOperator::Harmonic);
        assert_eq!(parse_mean("R", 0.0).unwrap(), MeanOperator::Random);
        assert_eq!(parse_mean("P0.01", 0.0).unwrap(), MeanOperator::Power(0.01));
        assert_eq!(parse_mean("P100", 0.0).unwrap(), MeanOperator::Power(100.0));
        assert_eq!(parse_mean("P", 100.0).unwrap(), MeanOperator::Power(100.0));
        assert!(parse_mean("P", 0.0).is_err());
        assert!(parse_mean("P0", 0.0).is_err());
        assert!(parse_mean("X", 0.0).is_err());
    }
}
