//! Inter-rater **agreement / association** metrics over contingency tables.
//!
//! Like [`stats`](crate::stats) and [`distribution`](crate::distribution), this
//! module has **zero dependencies** and is always compiled (no crate feature
//! required), so it pulls in nothing beyond `std`.
//!
//! These are pure contingency-table statistics comparing two raters / two
//! categorizations (a human label vs a model label): tetrachoric correlation,
//! Cohen's κ, the intraclass correlation coefficient, Cramér's V, proportion
//! agreement, and a two-sample proportion test.  They are ported faithfully
//! from the `argyle2023` replication's `common::stats` module so that a later
//! migration of that crate onto `socsim-metrics` is numerically bit-compatible.
//! The two external-crate dependencies used there (`statrs` for the standard
//! normal and chi-square distributions) are replaced with the self-contained
//! [`std_normal_cdf`] / [`std_normal_inverse_cdf`] helpers below and the
//! [`chi_square_sf`](crate::distribution::chi_square_sf) already in this crate.
//!
//! 2×2 cells are always passed row-major from the top-left as
//! `(n00, n01, n10, n11)` where `n00 = (row 0, col 0)`, etc.

use crate::distribution::chi_square_sf;

// ── Standard normal helpers (zero-dependency replacements for statrs) ────────

/// Standard normal CDF `Φ(z) = P(Z ≤ z)` via the complementary error function.
///
/// Uses `Φ(z) = ½ · erfc(−z / √2)` with the Abramowitz & Stegun 7.1.26
/// rational approximation of `erfc` (absolute error `< 1.5e-7`), sufficient for
/// the bivariate-normal quadrature in [`bvn_cdf`].
pub fn std_normal_cdf(z: f64) -> f64 {
    0.5 * erfc(-z / std::f64::consts::SQRT_2)
}

/// Complementary error function `erfc(x) = 1 − erf(x)` (A&S 7.1.26).
fn erfc(x: f64) -> f64 {
    // erf is odd; compute for |x| and restore the sign.
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * ax);
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-ax * ax).exp();
    // y is erf(ax); erf(x) = sign * y, so erfc(x) = 1 - sign*y.
    1.0 - sign * y
}

/// Inverse standard normal CDF (probit) `Φ⁻¹(p)` for `p ∈ (0, 1)`.
///
/// Acklam's rational approximation (relative error `< 1.15e-9`), refined by one
/// Halley step against [`std_normal_cdf`].  Clamps to `±∞` at the boundaries.
pub fn std_normal_inverse_cdf(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }

    // Coefficients for Acklam's algorithm.
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383_577_518_672_69e2,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    let mut x = if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };

    // One Halley refinement step.
    let e = std_normal_cdf(x) - p;
    let u = e * (2.0 * std::f64::consts::PI).sqrt() * (0.5 * x * x).exp();
    x -= u / (1.0 + 0.5 * x * u);
    x
}

// ── Bivariate normal CDF (Drezner–Wesolowsky) ───────────────────────────────

/// Standard **bivariate normal CDF** `Φ₂(h, k; ρ) = P(X ≤ h, Y ≤ k)`.
///
/// Evaluated with the Drezner–Wesolowsky (1990) 20-point Gauss–Legendre
/// quadrature (accuracy `~1e-9` away from the boundaries); the boundaries
/// `|ρ| → 1` are handled in closed form.  Used as a helper by [`tetrachoric`].
///
/// Reference: Drezner, Z. & Wesolowsky, G. O. (1990). "On the computation of
/// the bivariate normal integral." *J. Statist. Comput. Simul.*, 35, 101–107.
pub fn bvn_cdf(h: f64, k: f64, rho: f64) -> f64 {
    if rho >= 1.0 {
        // Perfect positive correlation: P(X≤h, X≤k) = Φ(min(h,k)).
        return std_normal_cdf(h.min(k));
    }
    if rho <= -1.0 {
        // Perfect negative correlation: P(X≤h, −X≤k) = max(0, Φ(h)+Φ(k)−1).
        return (std_normal_cdf(h) + std_normal_cdf(k) - 1.0).max(0.0);
    }
    if rho == 0.0 {
        return std_normal_cdf(h) * std_normal_cdf(k);
    }

    const W: [f64; 10] = [
        0.0176140071391521,
        0.0406014298003869,
        0.0626720483341091,
        0.0832767415767047,
        0.1019301198172404,
        0.1181945319615184,
        0.1316886384491766,
        0.142096109318382,
        0.1491729864726037,
        0.1527533871307258,
    ];
    const X: [f64; 10] = [
        0.9931285991850949,
        0.9639719272779138,
        0.912234428251326,
        0.8391169718222188,
        0.7463319064601508,
        0.636053680726515,
        0.5108670019508271,
        0.3737060887154195,
        0.2277858511416451,
        0.0765265211334973,
    ];

    let hk = h * k;
    let mut bvn = 0.0_f64;
    let asr = rho.asin();
    for i in 0..10 {
        for &sign in &[-1.0_f64, 1.0_f64] {
            let sn = (asr * (1.0 + sign * X[i]) / 2.0).sin();
            let val = (sn * hk - 0.5 * (h * h + k * k)) / (1.0 - sn * sn);
            bvn += W[i] * val.exp();
        }
    }
    bvn = bvn * asr / (4.0 * std::f64::consts::PI) + std_normal_cdf(h) * std_normal_cdf(k);
    bvn.clamp(0.0, 1.0)
}

// ── Tetrachoric correlation ─────────────────────────────────────────────────

/// **Tetrachoric correlation** `ρ` from a 2×2 table (`psych::tetrachoric`
/// compatible, `correct = 0.5`).
///
/// Estimates the latent bivariate-normal correlation by ML: from the marginal
/// proportions it forms thresholds `h = Φ⁻¹(P(row = 0))`,
/// `k = Φ⁻¹(P(col = 0))` and solves `Φ₂(h, k; ρ) = n00 / N` for `ρ` by
/// bisection on `[−1, 1]`.  Reproduces psych's `correct = 0.5` empty-cell
/// correction: if any cell is `0`, `0.5` is added to all four cells.  Degenerate
/// marginals return `±1`; a total of `0` returns `NaN`.
pub fn tetrachoric(n00: f64, n01: f64, n10: f64, n11: f64) -> f64 {
    let (mut a, mut b, mut c, mut d) = (n00, n01, n10, n11);

    if a == 0.0 || b == 0.0 || c == 0.0 || d == 0.0 {
        a += 0.5;
        b += 0.5;
        c += 0.5;
        d += 0.5;
    }

    let n = a + b + c + d;
    if n <= 0.0 {
        return f64::NAN;
    }

    let p_row0 = (a + b) / n;
    let p_col0 = (a + c) / n;
    let p00 = a / n;

    if p_row0 <= 0.0 || p_row0 >= 1.0 || p_col0 <= 0.0 || p_col0 >= 1.0 {
        return if (a + d) >= (b + c) { 1.0 } else { -1.0 };
    }

    let h = std_normal_inverse_cdf(p_row0);
    let k = std_normal_inverse_cdf(p_col0);

    let f = |rho: f64| bvn_cdf(h, k, rho) - p00;

    let lo_val = f(-1.0);
    let hi_val = f(1.0);

    if lo_val > 0.0 {
        return -1.0;
    }
    if hi_val < 0.0 {
        return 1.0;
    }

    let mut lo = -1.0_f64;
    let mut hi = 1.0_f64;
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        let fm = f(mid);
        if fm.abs() < 1e-12 {
            return mid;
        }
        if fm > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
        if (hi - lo) < 1e-13 {
            break;
        }
    }
    0.5 * (lo + hi)
}

// ── Cohen's kappa ───────────────────────────────────────────────────────────

/// **Cohen's κ** (chance-corrected agreement) from a 2×2 table
/// (`psych::cohen.kappa`, unweighted).
///
/// With observed agreement `p_o = (n00 + n11) / N` and chance agreement
/// `p_e = P(row=0)P(col=0) + P(row=1)P(col=1)`, returns
/// `κ = (p_o − p_e) / (1 − p_e)`.  A total of `0`, or `p_e = 1` (all mass in one
/// category, so the denominator vanishes), returns `NaN`.
pub fn cohen_kappa(n00: f64, n01: f64, n10: f64, n11: f64) -> f64 {
    let total = n00 + n01 + n10 + n11;
    if total == 0.0 {
        return f64::NAN;
    }

    let p_o = (n00 + n11) / total;

    let row0 = (n00 + n01) / total;
    let row1 = (n10 + n11) / total;
    let col0 = (n00 + n10) / total;
    let col1 = (n01 + n11) / total;

    let p_e = row0 * col0 + row1 * col1;

    if (1.0 - p_e).abs() < 1e-15 {
        return f64::NAN;
    }
    (p_o - p_e) / (1.0 - p_e)
}

// ── Intraclass correlation coefficient ──────────────────────────────────────

/// Average-measures ICC in its three forms (`ICC1k`, `ICC2k`, `ICC3k`),
/// matching `psych::ICC`'s `ICC[4:6]`.
#[derive(Debug, Clone, Copy)]
pub struct AverageIcc {
    /// ICC1k (absolute agreement, one-way).
    pub icc1k: f64,
    /// ICC2k (random raters, two-way absolute).
    pub icc2k: f64,
    /// ICC3k (fixed raters, two-way consistency).
    pub icc3k: f64,
}

impl AverageIcc {
    /// The minimum of the three forms (`min(ICC[4:6])`).
    pub fn min(&self) -> f64 {
        self.icc1k.min(self.icc2k).min(self.icc3k)
    }
}

/// **Average-measures ICC** from two rater series (`psych::ICC` on a 2-column
/// matrix; columns = raters, rows = subjects).
///
/// `human` and `model` are the two raters' values for each subject and must be
/// the same length.  Performs the two-way ANOVA decomposition and returns the
/// three average-measures ICCs.  Fewer than two subjects → all `NaN`.
///
/// # Panics
/// Panics if `human.len() != model.len()` (matches the ported implementation).
pub fn average_icc(human: &[f64], model: &[f64]) -> AverageIcc {
    assert_eq!(
        human.len(),
        model.len(),
        "the two rater series must have equal length"
    );
    let n = human.len();
    let nj = 2.0_f64;

    if n < 2 {
        return AverageIcc {
            icc1k: f64::NAN,
            icc2k: f64::NAN,
            icc3k: f64::NAN,
        };
    }
    let n_f = n as f64;

    let mut sum = 0.0;
    for i in 0..n {
        sum += human[i] + model[i];
    }
    let grand = sum / (n_f * nj);

    let col0_mean = human.iter().sum::<f64>() / n_f;
    let col1_mean = model.iter().sum::<f64>() / n_f;

    let mut sst = 0.0;
    let mut ssb = 0.0;
    for i in 0..n {
        let row_mean = 0.5 * (human[i] + model[i]);
        ssb += (row_mean - grand).powi(2);
        sst += (human[i] - grand).powi(2) + (model[i] - grand).powi(2);
    }
    ssb *= nj;

    let ssj = n_f * ((col0_mean - grand).powi(2) + (col1_mean - grand).powi(2));
    let sse = sst - ssb - ssj;

    let msb = ssb / (n_f - 1.0);
    let msj = ssj / (nj - 1.0);
    let df_e = (n_f - 1.0) * (nj - 1.0);
    let mse = if df_e > 0.0 { sse / df_e } else { f64::NAN };
    let msw = (ssj + sse) / (n_f * (nj - 1.0));

    let icc1k = (msb - msw) / msb;
    let icc2k = (msb - mse) / (msb + (msj - mse) / n_f);
    let icc3k = (msb - mse) / msb;

    AverageIcc {
        icc1k,
        icc2k,
        icc3k,
    }
}

/// `min(ICC[4:6])` — the minimum of the three average-measures ICCs from two
/// rater series.  See [`average_icc`].
pub fn icc(human: &[f64], model: &[f64]) -> f64 {
    average_icc(human, model).min()
}

// ── Cramér's V ──────────────────────────────────────────────────────────────

/// **Cramér's V** from an `r × c` contingency table (row-major `counts[i][j]`).
///
/// Matches `DescTools::CramerV` defaults (`correct = FALSE`, no bias
/// correction): `V = √(χ² / (N · min(r − 1, c − 1)))` with the Pearson χ²
/// (no continuity correction).  Returns `NaN` for an empty table, a zero total,
/// or `min(r − 1, c − 1) = 0` (only one row or column).
pub fn cramers_v(counts: &[Vec<f64>]) -> f64 {
    let r = counts.len();
    if r == 0 {
        return f64::NAN;
    }
    let c = counts[0].len();
    if c == 0 {
        return f64::NAN;
    }

    let row_sums: Vec<f64> = counts.iter().map(|row| row.iter().sum()).collect();
    let mut col_sums = vec![0.0_f64; c];
    for row in counts {
        for (j, &v) in row.iter().enumerate() {
            col_sums[j] += v;
        }
    }
    let n: f64 = row_sums.iter().sum();
    if n <= 0.0 {
        return f64::NAN;
    }

    let mut chi2 = 0.0;
    for i in 0..r {
        for j in 0..c {
            let e = row_sums[i] * col_sums[j] / n;
            if e > 0.0 {
                let o = counts[i][j];
                chi2 += (o - e) * (o - e) / e;
            }
        }
    }

    let m = (r.min(c)) as f64 - 1.0;
    if m <= 0.0 {
        return f64::NAN;
    }
    (chi2 / (n * m)).sqrt()
}

// ── Proportion agreement & two-sample proportion test ───────────────────────

/// **Proportion agreement** `(n00 + n11) / N` (the diagonal share of a 2×2
/// table).  A total of `0` → `NaN`.
pub fn prop_agree(n00: f64, n01: f64, n10: f64, n11: f64) -> f64 {
    let total = n00 + n01 + n10 + n11;
    if total == 0.0 {
        return f64::NAN;
    }
    (n00 + n11) / total
}

/// **Two-sample proportion test** (R `prop.test`, two-sided, Yates-corrected).
///
/// Given successes `x1` of `n1` and `x2` of `n2`, computes the χ² statistic for
/// the difference in proportions with Yates' continuity correction
/// `Δ = min(0.5, |x1 − n1·p̄|)` (where `p̄ = (x1 + x2)/(n1 + n2)` is the pooled
/// proportion) and the two-sided p-value from the χ² distribution with 1 degree
/// of freedom.  Returns `(chi_squared, p_value)`.
pub fn prop_test(x1: u64, n1: u64, x2: u64, n2: u64) -> (f64, f64) {
    let (x1, n1, x2, n2) = (x1 as f64, n1 as f64, x2 as f64, n2 as f64);
    let p1 = x1 / n1;
    let p2 = x2 / n2;
    let p_bar = (x1 + x2) / (n1 + n2);

    let yates = 0.5_f64.min((x1 - n1 * p_bar).abs());
    let inv = 1.0 / n1 + 1.0 / n2;

    let num = ((p1 - p2).abs() - yates * inv).max(0.0);
    let chi = (num * num) / (p_bar * (1.0 - p_bar) * inv);

    // Two-sided p-value = upper-tail of χ²₁ = survival function at `chi`.
    let p_value = chi_square_sf(chi, 1.0);
    (chi, p_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() < tol, "expected {b}, got {a}");
    }

    // ── normal helpers ──────────────────────────────────────────────────────

    #[test]
    fn normal_cdf_known() {
        approx(std_normal_cdf(0.0), 0.5, 1e-7);
        approx(std_normal_cdf(1.96), 0.975, 1e-3);
        approx(std_normal_cdf(-1.96), 0.025, 1e-3);
    }

    #[test]
    fn normal_inverse_cdf_known() {
        approx(std_normal_inverse_cdf(0.5), 0.0, 1e-8);
        // The Halley refinement uses the A&S erfc (≈1.5e-7 error), so the
        // probit is accurate to ~1e-5 here rather than to full double precision.
        approx(std_normal_inverse_cdf(0.975), 1.959_963_98, 1e-5);
        approx(std_normal_inverse_cdf(0.025), -1.959_963_98, 1e-5);
        assert_eq!(std_normal_inverse_cdf(0.0), f64::NEG_INFINITY);
        assert_eq!(std_normal_inverse_cdf(1.0), f64::INFINITY);
    }

    // ── bvn_cdf (ported argyle tests) ───────────────────────────────────────

    #[test]
    fn bvn_independence_factorizes() {
        let got = bvn_cdf(0.5, -0.3, 0.0);
        let exp = std_normal_cdf(0.5) * std_normal_cdf(-0.3);
        approx(got, exp, 1e-9);
    }

    #[test]
    fn bvn_zero_zero_closed_form() {
        // Φ₂(0,0;ρ) = 1/4 + asin(ρ)/(2π).
        for &rho in &[-0.8, -0.3, 0.0, 0.4, 0.9] {
            let got = bvn_cdf(0.0, 0.0, rho);
            let exp = 0.25 + rho.asin() / (2.0 * std::f64::consts::PI);
            approx(got, exp, 1e-9);
        }
    }

    #[test]
    fn bvn_perfect_correlations() {
        approx(bvn_cdf(0.3, 0.7, 1.0), std_normal_cdf(0.3), 1e-9);
        let exp = (std_normal_cdf(0.3) + std_normal_cdf(0.7) - 1.0).max(0.0);
        approx(bvn_cdf(0.3, 0.7, -1.0), exp, 1e-9);
    }

    // ── tetrachoric (ported argyle tests) ───────────────────────────────────

    #[test]
    fn tetrachoric_independence_near_zero() {
        let rho = tetrachoric(25.0, 25.0, 25.0, 25.0);
        assert!(rho.abs() < 1e-6, "rho={rho}");
    }

    #[test]
    fn tetrachoric_strong_concordance() {
        let rho = tetrachoric(40.0, 10.0, 10.0, 40.0);
        assert!(rho > 0.6 && rho < 0.95, "rho={rho}");
    }

    #[test]
    fn tetrachoric_negative_correlation() {
        let rho = tetrachoric(10.0, 40.0, 40.0, 10.0);
        assert!(rho < -0.6, "rho={rho}");
    }

    // ── cohen_kappa (ported argyle tests) ───────────────────────────────────

    #[test]
    fn kappa_perfect_agreement_is_one() {
        let k = cohen_kappa(40.0, 0.0, 0.0, 40.0);
        approx(k, 1.0, 1e-12);
    }

    #[test]
    fn kappa_independence_is_zero() {
        let k = cohen_kappa(25.0, 25.0, 25.0, 25.0);
        approx(k, 0.0, 1e-12);
    }

    #[test]
    fn kappa_known_2x2_textbook() {
        // n00=20, n01=5, n10=10, n11=15: p_o=0.70, p_e=0.50 → κ=0.40.
        let k = cohen_kappa(20.0, 5.0, 10.0, 15.0);
        approx(k, 0.40, 1e-12);
    }

    #[test]
    fn kappa_diagonal_dominant_high() {
        let k = cohen_kappa(400.0, 70.0, 80.0, 450.0);
        assert!(k > 0.6 && k < 0.8, "k={k}");
    }

    #[test]
    fn kappa_degenerate_nan() {
        assert!(cohen_kappa(0.0, 0.0, 0.0, 0.0).is_nan());
        assert!(cohen_kappa(10.0, 0.0, 0.0, 0.0).is_nan()); // all in one cell → p_e=1
    }

    // ── icc (ported argyle tests) ───────────────────────────────────────────

    #[test]
    fn icc_perfect_agreement_is_one() {
        let a = [0.0, 1.0, 0.0, 1.0, 1.0, 0.0];
        approx(icc(&a, &a), 1.0, 1e-12);
    }

    #[test]
    fn icc_diagonal_dominant_high() {
        let mut human = Vec::new();
        let mut model = Vec::new();
        for _ in 0..400 {
            human.push(0.0);
            model.push(0.0);
        }
        for _ in 0..450 {
            human.push(1.0);
            model.push(1.0);
        }
        for _ in 0..80 {
            human.push(1.0);
            model.push(0.0);
        }
        for _ in 0..70 {
            human.push(0.0);
            model.push(1.0);
        }
        let v = icc(&human, &model);
        assert!(v > 0.7 && v < 0.9, "icc={v}");
    }

    #[test]
    fn icc_continuous_perfect() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let r = average_icc(&a, &a);
        approx(r.icc1k, 1.0, 1e-9);
        approx(r.icc3k, 1.0, 1e-9);
        approx(r.icc2k, 1.0, 1e-9);
    }

    #[test]
    fn icc_too_few_subjects_nan() {
        assert!(icc(&[1.0], &[1.0]).is_nan());
    }

    // ── cramers_v ───────────────────────────────────────────────────────────

    #[test]
    fn cramers_v_perfect_association() {
        // Diagonal 2×2 → perfect association → V = 1.
        let table = vec![vec![10.0, 0.0], vec![0.0, 10.0]];
        approx(cramers_v(&table), 1.0, 1e-12);
    }

    #[test]
    fn cramers_v_independence_is_zero() {
        // Uniform table → χ² = 0 → V = 0.
        let table = vec![vec![5.0, 5.0], vec![5.0, 5.0]];
        approx(cramers_v(&table), 0.0, 1e-12);
    }

    #[test]
    fn cramers_v_known_value() {
        // 2×2 [[10,20],[30,40]]: expected [[12,18],[28,42]],
        // χ² = 4/12 + 4/18 + 4/28 + 4/42 = 0.793651.
        // N=100, min(r-1,c-1)=1 → V = √(0.793651/100) = 0.089087.
        let table = vec![vec![10.0, 20.0], vec![30.0, 40.0]];
        approx(cramers_v(&table), 0.089_087, 1e-5);
    }

    #[test]
    fn cramers_v_degenerate_nan() {
        assert!(cramers_v(&[]).is_nan());
        assert!(cramers_v(&[vec![1.0, 2.0, 3.0]]).is_nan()); // single row → min dim 0
    }

    // ── prop_agree ──────────────────────────────────────────────────────────

    #[test]
    fn prop_agree_basic() {
        approx(prop_agree(40.0, 10.0, 10.0, 40.0), 0.8, 1e-12);
        assert!(prop_agree(0.0, 0.0, 0.0, 0.0).is_nan());
    }

    // ── prop_test (ported argyle golden) ────────────────────────────────────

    #[test]
    fn prop_test_turing_golden() {
        // R prop.test(c(6617,7463), c(10721,12191)): X²=0.58755, p=0.4434.
        let (chi, p) = prop_test(6617, 10721, 7463, 12191);
        approx(chi, 0.5875, 1e-3);
        approx(p, 0.4434, 1e-3);
        assert!(p >= 0.05, "should be non-significant: p={p}");
    }
}
