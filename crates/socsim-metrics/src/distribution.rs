//! Distribution-comparison metrics over slices of `f64`.
//!
//! Like [`stats`](crate::stats), this module has **zero dependencies** and is
//! always compiled (no crate feature required), so it pulls in nothing beyond
//! `std`.  The p-value of [`chi_square_homogeneity`] is computed from a
//! hand-rolled regularized upper incomplete gamma function rather than adding a
//! heavyweight statistics crate.
//!
//! Inputs are treated as **counts or weights**: distributions are normalized
//! internally where a probability interpretation is required, and degenerate
//! inputs (empty, non-positive totals, length mismatches) return a documented
//! neutral value rather than panicking.

/// Smoothing constant added to every probability before a logarithm or
/// division in [`kl_divergence`], guarding against `log(0)` and division by
/// zero when a class has zero probability under `p` or `q`.
const KL_EPSILON: f64 = 1e-12;

/// **Kullback–Leibler divergence** `D(p ‖ q) = Σ pᵢ · ln(pᵢ / qᵢ)` (nats).
///
/// Both inputs are treated as unnormalized weights (counts or shares) and are
/// **normalized internally** to probability distributions summing to `1`
/// (negative weights are ignored, treated as `0`).  To avoid `log(0)` and
/// division-by-zero, a small epsilon ([`KL_EPSILON`] = `1e-12`) is added to
/// every probability *after* normalization and the smoothed `q` is
/// renormalized; the smoothing is negligible for well-populated bins and only
/// matters when a class has zero mass under `p` or `q`.
///
/// For valid (matching-length, positive-total) inputs the result is `≥ 0`, and
/// `D(p ‖ p) = 0` up to the epsilon perturbation.  The divergence is **not**
/// symmetric: `D(p ‖ q) ≠ D(q ‖ p)` in general.
///
/// Edge cases: mismatched lengths, an empty slice, or either side summing to
/// `≤ 0` → `0.0`.
pub fn kl_divergence(p: &[f64], q: &[f64]) -> f64 {
    if p.len() != q.len() || p.is_empty() {
        return 0.0;
    }
    let p_total: f64 = p.iter().filter(|w| **w > 0.0).sum();
    let q_total: f64 = q.iter().filter(|w| **w > 0.0).sum();
    if p_total <= 0.0 || q_total <= 0.0 {
        return 0.0;
    }

    // Normalize, then smooth with epsilon and renormalize so both stay valid
    // distributions with strictly positive entries.
    let n = p.len() as f64;
    let pe: Vec<f64> = p
        .iter()
        .map(|&w| (w.max(0.0) / p_total) + KL_EPSILON)
        .collect();
    let qe: Vec<f64> = q
        .iter()
        .map(|&w| (w.max(0.0) / q_total) + KL_EPSILON)
        .collect();
    let pe_total: f64 = 1.0 + n * KL_EPSILON; // Σ (pᵢ + ε)
    let qe_total: f64 = 1.0 + n * KL_EPSILON;

    let mut d = 0.0;
    for (pi, qi) in pe.into_iter().zip(qe) {
        let pp = pi / pe_total;
        let qq = qi / qe_total;
        d += pp * (pp / qq).ln();
    }
    d
}

/// **Pearson chi-square homogeneity test** comparing observed counts against
/// expected counts.
///
/// Returns `(statistic, p_value)` where the statistic is
/// `χ² = Σ (Oᵢ − Eᵢ)² / Eᵢ` and the p-value is the upper-tail survival of the
/// chi-square distribution with `df = len − 1` degrees of freedom, i.e.
/// `P(χ²_df > statistic)`.
///
/// The p-value is computed from the regularized upper incomplete gamma
/// function `Q(df/2, statistic/2)` (see [`chi_square_sf`]).  Terms with a
/// non-positive expected count are skipped (a `0` expected count would make
/// `(O − E)² / E` undefined); pass strictly positive expected counts for a
/// meaningful test.
///
/// Edge cases: mismatched lengths, fewer than two cells (so `df < 1`), or no
/// usable cells → `(0.0, 1.0)` (zero statistic, no evidence against
/// homogeneity).
pub fn chi_square_homogeneity(observed: &[f64], expected: &[f64]) -> (f64, f64) {
    if observed.len() != expected.len() || observed.len() < 2 {
        return (0.0, 1.0);
    }
    let mut stat = 0.0;
    let mut used = 0usize;
    for (&o, &e) in observed.iter().zip(expected.iter()) {
        if e > 0.0 {
            let d = o - e;
            stat += d * d / e;
            used += 1;
        }
    }
    if used < 2 {
        return (0.0, 1.0);
    }
    let df = (observed.len() - 1) as f64;
    let p = chi_square_sf(stat, df);
    (stat, p)
}

/// Survival function (upper-tail probability) `P(χ²_df > x)` of the chi-square
/// distribution with `df` degrees of freedom.
///
/// Equals the regularized upper incomplete gamma `Q(df/2, x/2)`.  Returns `1.0`
/// for `x ≤ 0` and clamps the result to `[0, 1]`.
pub fn chi_square_sf(x: f64, df: f64) -> f64 {
    if x <= 0.0 || df <= 0.0 {
        return 1.0;
    }
    gamma_q(df / 2.0, x / 2.0).clamp(0.0, 1.0)
}

/// Regularized **upper** incomplete gamma function `Q(s, x) = 1 − P(s, x)`.
///
/// Uses the series expansion for the lower regularized gamma `P(s, x)` when
/// `x < s + 1` and the continued-fraction expansion for `Q(s, x)` otherwise,
/// the standard "Numerical Recipes" split for numerical stability.
fn gamma_q(s: f64, x: f64) -> f64 {
    if x < s + 1.0 {
        1.0 - gamma_p_series(s, x)
    } else {
        gamma_q_continued_fraction(s, x)
    }
}

/// Lower regularized gamma `P(s, x)` via its power series (good for `x < s+1`).
fn gamma_p_series(s: f64, x: f64) -> f64 {
    const MAX_ITER: usize = 1000;
    const EPS: f64 = 1e-15;
    if x <= 0.0 {
        return 0.0;
    }
    let mut ap = s;
    let mut sum = 1.0 / s;
    let mut del = sum;
    for _ in 0..MAX_ITER {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * EPS {
            break;
        }
    }
    sum * (-x + s * x.ln() - ln_gamma(s)).exp()
}

/// Upper regularized gamma `Q(s, x)` via its continued fraction (good for
/// `x ≥ s+1`), evaluated with the modified Lentz algorithm.
fn gamma_q_continued_fraction(s: f64, x: f64) -> f64 {
    const MAX_ITER: usize = 1000;
    const EPS: f64 = 1e-15;
    const TINY: f64 = 1e-300;
    let mut b = x + 1.0 - s;
    let mut c = 1.0 / TINY;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..=MAX_ITER {
        let an = -(i as f64) * (i as f64 - s);
        b += 2.0;
        d = an * d + b;
        if d.abs() < TINY {
            d = TINY;
        }
        c = b + an / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    (-x + s * x.ln() - ln_gamma(s)).exp() * h
}

// ── Distributional comparison over an ordinal scale ─────────────────────────
//
// The functions below compare a "human" distribution `p` against a "predicted"
// distribution `q` defined over a **common ordinal support** — the integer
// category positions `0, 1, …, k − 1` of a `k`-point (e.g. Likert) scale, in
// ascending order.  Both slices must have the same length `k` and are read as
// **probability masses** over those positions: entry `i` is the mass on
// category `i`.  They are normalized internally (by their own positive total)
// so unnormalized counts work too; negative entries are treated as `0`.
//
// These port the three distribution-matching metrics from the gong2026
// replication (MD / SDD / NEMD): a human poll distribution `H` versus an LLM
// predicted distribution `P`, standardized by the question's `range` so they
// are comparable across questions of different cardinality.

/// Normalize a slice of masses to a probability distribution summing to `1`.
///
/// Negative entries are clamped to `0`; returns `None` if the positive total is
/// `≤ 0` (an empty or all-zero/all-negative slice).
fn normalized(p: &[f64]) -> Option<Vec<f64>> {
    let total: f64 = p.iter().filter(|w| **w > 0.0).sum();
    if total <= 0.0 {
        return None;
    }
    Some(p.iter().map(|&w| w.max(0.0) / total).collect())
}

/// Expectation `Σ i · pᵢ` of the ordinal position under probabilities `p`
/// (i.e. the mean category index, `0`-based).  `p` must already be normalized.
fn ordinal_mean(p: &[f64]) -> f64 {
    p.iter()
        .enumerate()
        .map(|(i, &pi)| i as f64 * pi)
        .sum::<f64>()
}

/// Population standard deviation of the ordinal position under probabilities
/// `p` (mass-weighted over indices `0..k`).  `p` must already be normalized.
fn ordinal_std(p: &[f64]) -> f64 {
    let mu = ordinal_mean(p);
    let var = p
        .iter()
        .enumerate()
        .map(|(i, &pi)| pi * (i as f64 - mu).powi(2))
        .sum::<f64>();
    var.max(0.0).sqrt()
}

/// **1-D Wasserstein (Earth-Mover's) distance** `W₁(p, q)` between two
/// distributions on a **shared ordered support**.
///
/// For distributions over the ordinal positions `0, 1, …, k − 1` with unit
/// spacing, the continuous `W₁ = ∫ |F_p(x) − F_q(x)| dx` reduces to the sum of
/// absolute differences of the cumulative distributions:
///
/// ```text
/// W₁(p, q) = Σⱼ | cumsum(p)ⱼ − cumsum(q)ⱼ |
/// ```
///
/// Both inputs are treated as probability masses over the **same** ordered
/// categories (entry `i` is the mass on category `i`) and are normalized
/// internally, so passing raw counts is fine.  The support is assumed to be
/// equally spaced with unit step; the result is therefore in units of "category
/// steps" and is symmetric (`W₁(p, q) = W₁(q, p)`).
///
/// Edge cases: mismatched lengths, an empty slice, or either side summing to
/// `≤ 0` → `0.0`.
pub fn wasserstein_1d(p: &[f64], q: &[f64]) -> f64 {
    if p.len() != q.len() || p.is_empty() {
        return 0.0;
    }
    let (pn, qn) = match (normalized(p), normalized(q)) {
        (Some(pn), Some(qn)) => (pn, qn),
        _ => return 0.0,
    };
    let mut cum_p = 0.0;
    let mut cum_q = 0.0;
    let mut dist = 0.0;
    for (pi, qi) in pn.into_iter().zip(qn) {
        cum_p += pi;
        cum_q += qi;
        dist += (cum_p - cum_q).abs();
    }
    dist
}

/// **Normalized Earth-Mover's Distance** `NEMD = W₁(p, q) / Z`.
///
/// The [`wasserstein_1d`] distance divided by a normalizer `Z = range`, the
/// cardinality-based scale of the question (per gong2026: standardize by the
/// question's range so distances are comparable across scales of different
/// cardinality).  For a `k`-point ordinal scale the natural `range` is
/// `k − 1` (the maximum possible `W₁`, attained when all mass shifts from one
/// end of the scale to the other), which maps `NEMD` onto `[0, 1]`.
///
/// `range` must be `> 0`; a non-positive `range` (or any degenerate input that
/// makes [`wasserstein_1d`] return `0`) → `0.0`.
pub fn nemd(p: &[f64], q: &[f64], range: f64) -> f64 {
    if range <= 0.0 {
        return 0.0;
    }
    wasserstein_1d(p, q) / range
}

/// **Mean Difference** `MD = |μ_p − μ_q| / range`.
///
/// `μ` is the expectation of the ordinal position (`0`-based category index)
/// under each distribution, i.e. the mass-weighted mean over indices `0..k`.
/// The absolute difference is standardized by `range` (the question's
/// cardinality-based scale, typically `k − 1`).  Always `≥ 0`.
///
/// Edge cases: mismatched lengths, an empty slice, either side summing to
/// `≤ 0`, or `range ≤ 0` → `0.0`.
pub fn mean_diff(p: &[f64], q: &[f64], range: f64) -> f64 {
    if p.len() != q.len() || p.is_empty() || range <= 0.0 {
        return 0.0;
    }
    let (pn, qn) = match (normalized(p), normalized(q)) {
        (Some(pn), Some(qn)) => (pn, qn),
        _ => return 0.0,
    };
    (ordinal_mean(&pn) - ordinal_mean(&qn)).abs() / range
}

/// **Standard Deviation Difference** `SDD = (σ_p − σ_q) / range` (**signed**).
///
/// `σ` is the population standard deviation of the ordinal position under each
/// distribution (mass-weighted over indices `0..k`).  The difference is
/// standardized by `range`.  Following gong2026, the sign is meaningful when
/// `p` is the human distribution and `q` the LLM prediction: `SDD < 0` means
/// the LLM **overestimates homogeneity** (its distribution is too concentrated,
/// `σ_q > σ_p`), `SDD > 0` means it underestimates it.
///
/// Edge cases: mismatched lengths, an empty slice, either side summing to
/// `≤ 0`, or `range ≤ 0` → `0.0`.
pub fn sd_diff(p: &[f64], q: &[f64], range: f64) -> f64 {
    if p.len() != q.len() || p.is_empty() || range <= 0.0 {
        return 0.0;
    }
    let (pn, qn) = match (normalized(p), normalized(q)) {
        (Some(pn), Some(qn)) => (pn, qn),
        _ => return 0.0,
    };
    (ordinal_std(&pn) - ordinal_std(&qn)) / range
}

/// Natural log of the gamma function via the Lanczos approximation.
fn ln_gamma(x: f64) -> f64 {
    // Lanczos coefficients (g = 7, n = 9).
    const G: f64 = 7.0;
    const COEF: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection formula: Γ(x)Γ(1−x) = π / sin(πx).
        (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x)
    } else {
        let x = x - 1.0;
        let mut a = COEF[0];
        let t = x + G + 0.5;
        for (i, &c) in COEF.iter().enumerate().skip(1) {
            a += c / (x + i as f64);
        }
        0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "expected {b}, got {a}");
    }

    #[test]
    fn kl_of_identical_is_zero() {
        // D(p ‖ p) = 0 (up to epsilon perturbation, well under 1e-6).
        approx(kl_divergence(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]), 0.0, 1e-6);
        // Different scale, same shape → still 0 (normalized internally).
        approx(kl_divergence(&[1.0, 1.0], &[5.0, 5.0]), 0.0, 1e-6);
    }

    #[test]
    fn kl_hand_computed_example() {
        // p = [0.5, 0.5], q = [0.25, 0.75].
        // D = 0.5*ln(0.5/0.25) + 0.5*ln(0.5/0.75)
        //   = 0.5*ln(2) + 0.5*ln(2/3) = 0.5*(0.693147) + 0.5*(-0.405465)
        //   = 0.143841.
        approx(kl_divergence(&[0.5, 0.5], &[0.25, 0.75]), 0.143841, 1e-5);
    }

    #[test]
    fn kl_is_nonnegative_and_asymmetric() {
        let p = [0.7, 0.2, 0.1];
        let q = [0.1, 0.2, 0.7];
        let dpq = kl_divergence(&p, &q);
        let dqp = kl_divergence(&q, &p);
        assert!(dpq > 0.0, "KL should be positive, got {dpq}");
        assert!(dqp > 0.0);
        // Generally asymmetric (here symmetric only by the mirror; check both >0
        // and that the function does not panic on zero bins).
        approx(
            kl_divergence(&[1.0, 0.0], &[0.5, 0.5]),
            std::f64::consts::LN_2,
            1e-4,
        );
    }

    #[test]
    fn kl_edge_cases() {
        approx(kl_divergence(&[], &[]), 0.0, 1e-12);
        approx(kl_divergence(&[1.0, 2.0], &[1.0]), 0.0, 1e-12); // length mismatch
        approx(kl_divergence(&[0.0, 0.0], &[1.0, 1.0]), 0.0, 1e-12); // zero total
    }

    #[test]
    fn chi_square_statistic_known() {
        // observed = [10, 10, 10, 10], expected = [10, 10, 10, 10] → χ² = 0.
        let (stat, p) = chi_square_homogeneity(&[10.0; 4], &[10.0; 4]);
        approx(stat, 0.0, 1e-12);
        approx(p, 1.0, 1e-12);

        // Textbook-style: observed [90, 10], expected [80, 20].
        // χ² = (10)²/80 + (−10)²/20 = 1.25 + 5.0 = 6.25.
        let (stat, _p) = chi_square_homogeneity(&[90.0, 10.0], &[80.0, 20.0]);
        approx(stat, 6.25, 1e-12);
    }

    #[test]
    fn chi_square_pvalue_critical_value_df1() {
        // df = 1: the 0.05 critical value is 3.841 → p ≈ 0.05.
        // Use a 2-cell table whose statistic is exactly 3.841459.
        let (stat, p) = chi_square_homogeneity(&[3.841_459, 0.0], &[1.0, 1.0]);
        // statistic: (3.841459-1)²/1 + (0-1)²/1 = 8.0738.. — not what we want, so
        // instead test the survival function directly at the known critical value.
        let _ = (stat, p);
        approx(chi_square_sf(3.841_459, 1.0), 0.05, 1e-4);
        // df = 1, the median ~0.4549 → p = 0.5.
        approx(chi_square_sf(0.454_936, 1.0), 0.5, 1e-3);
    }

    #[test]
    fn chi_square_pvalue_known_df2() {
        // df = 2: 0.05 critical value is 5.991 → p ≈ 0.05.
        approx(chi_square_sf(5.991_465, 2.0), 0.05, 1e-4);
        // df = 2, statistic 9.210 → p ≈ 0.01.
        approx(chi_square_sf(9.210_340, 2.0), 0.01, 1e-4);
    }

    #[test]
    fn chi_square_edge_cases() {
        assert_eq!(chi_square_homogeneity(&[1.0], &[1.0]), (0.0, 1.0)); // df < 1
        assert_eq!(chi_square_homogeneity(&[1.0, 2.0], &[1.0]), (0.0, 1.0)); // mismatch
        assert_eq!(chi_square_sf(-1.0, 2.0), 1.0); // x <= 0
    }

    #[test]
    fn wasserstein_identical_is_zero() {
        approx(
            wasserstein_1d(&[0.2, 0.3, 0.5], &[0.2, 0.3, 0.5]),
            0.0,
            1e-12,
        );
        // Different scale, same shape → still 0 (normalized internally).
        approx(
            wasserstein_1d(&[1.0, 1.0, 2.0], &[5.0, 5.0, 10.0]),
            0.0,
            1e-12,
        );
    }

    #[test]
    fn wasserstein_shifted_point_masses() {
        // Point mass at position 0 vs point mass at position 3.
        // cumsum(p) = [1,1,1,1], cumsum(q) = [0,0,0,1];
        // Σ|diff| = 1+1+1+0 = 3 (the number of unit steps the mass moved).
        approx(
            wasserstein_1d(&[1.0, 0.0, 0.0, 0.0], &[0.0, 0.0, 0.0, 1.0]),
            3.0,
            1e-12,
        );
        // Adjacent point masses → distance 1.
        approx(
            wasserstein_1d(&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0]),
            1.0,
            1e-12,
        );
        // Symmetry.
        approx(
            wasserstein_1d(&[0.0, 0.0, 0.0, 1.0], &[1.0, 0.0, 0.0, 0.0]),
            3.0,
            1e-12,
        );
    }

    #[test]
    fn wasserstein_half_split_example() {
        // p = [0.5, 0.5, 0], q = [0, 0.5, 0.5].
        // cumsum(p) = [0.5, 1.0, 1.0], cumsum(q) = [0, 0.5, 1.0];
        // Σ|diff| = 0.5 + 0.5 + 0 = 1.0.
        approx(
            wasserstein_1d(&[0.5, 0.5, 0.0], &[0.0, 0.5, 0.5]),
            1.0,
            1e-12,
        );
    }

    #[test]
    fn nemd_normalizes_by_range() {
        // Endpoint shift on a 4-point scale (k=4, range = k-1 = 3): W1 = 3,
        // NEMD = 3/3 = 1 (maximal).
        approx(
            nemd(&[1.0, 0.0, 0.0, 0.0], &[0.0, 0.0, 0.0, 1.0], 3.0),
            1.0,
            1e-12,
        );
        // Identical → 0; range guard → 0.
        approx(nemd(&[0.5, 0.5], &[0.5, 0.5], 1.0), 0.0, 1e-12);
        approx(nemd(&[1.0, 0.0], &[0.0, 1.0], 0.0), 0.0, 1e-12);
    }

    #[test]
    fn mean_diff_known() {
        // p mean = 0, q mean = 3 on a 4-point scale, range 3 → |0-3|/3 = 1.
        approx(
            mean_diff(&[1.0, 0.0, 0.0, 0.0], &[0.0, 0.0, 0.0, 1.0], 3.0),
            1.0,
            1e-12,
        );
        // p = [0.5,0.5] mean = 0.5, q = [0,1] mean = 1, range 1 → 0.5.
        approx(mean_diff(&[0.5, 0.5], &[0.0, 1.0], 1.0), 0.5, 1e-12);
        // Identical → 0; range guard → 0.
        approx(mean_diff(&[0.2, 0.8], &[0.2, 0.8], 1.0), 0.0, 1e-12);
        approx(mean_diff(&[1.0, 0.0], &[0.0, 1.0], 0.0), 0.0, 1e-12);
    }

    #[test]
    fn sd_diff_sign_and_value() {
        // p = uniform over {0,1}: σ_p = 0.5. q = point mass: σ_q = 0.
        // SDD = (0.5 - 0)/1 = +0.5 (LLM underestimates spread).
        approx(sd_diff(&[0.5, 0.5], &[1.0, 0.0], 1.0), 0.5, 1e-12);
        // Swap: p concentrated, q spread → SDD negative (overestimates homogeneity).
        approx(sd_diff(&[1.0, 0.0], &[0.5, 0.5], 1.0), -0.5, 1e-12);
        // Identical → 0; range guard → 0.
        approx(sd_diff(&[0.3, 0.7], &[0.3, 0.7], 1.0), 0.0, 1e-12);
        approx(sd_diff(&[1.0, 0.0], &[0.0, 1.0], 0.0), 0.0, 1e-12);
    }

    #[test]
    fn distribution_metrics_edge_cases() {
        approx(wasserstein_1d(&[], &[]), 0.0, 1e-12);
        approx(wasserstein_1d(&[1.0, 2.0], &[1.0]), 0.0, 1e-12); // length mismatch
        approx(wasserstein_1d(&[0.0, 0.0], &[1.0, 1.0]), 0.0, 1e-12); // zero total
        approx(mean_diff(&[1.0, 2.0], &[1.0], 1.0), 0.0, 1e-12); // mismatch
        approx(sd_diff(&[0.0, 0.0], &[1.0, 1.0], 1.0), 0.0, 1e-12); // zero total
    }

    #[test]
    fn ln_gamma_known_values() {
        // Γ(1) = 1 → ln = 0; Γ(5) = 24 → ln = ln 24.
        approx(ln_gamma(1.0), 0.0, 1e-9);
        approx(ln_gamma(5.0), (24.0_f64).ln(), 1e-9);
        // Γ(1/2) = √π → ln = 0.5 ln π.
        approx(ln_gamma(0.5), 0.5 * std::f64::consts::PI.ln(), 1e-9);
    }
}
