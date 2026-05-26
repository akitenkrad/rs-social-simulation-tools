//! Pure numeric summary statistics over slices of `f64` / `u32`.
//!
//! This module has **zero dependencies** and is always compiled (even with no
//! crate features), so `cargo build -p socsim-metrics` with no features pulls
//! in nothing beyond `std`.
//!
//! Every function documents its exact formula and its behavior on the empty
//! (and other degenerate) input.  The guiding convention is: an empty slice
//! yields the "neutral" value of the metric (`0.0` for dispersion / inequality
//! / diversity, `0` for counts), never a panic and never a `NaN`.

/// Arithmetic mean `(Σ xᵢ) / N`.
///
/// Empty slice → `0.0`.
pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// **Population** variance `(Σ (xᵢ − μ)²) / N` (divides by `N`, not `N − 1`).
///
/// Empty slice → `0.0`.  A single element → `0.0`.
pub fn variance(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let m = mean(xs);
    xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / xs.len() as f64
}

/// Population standard deviation `√variance`.
///
/// Empty slice → `0.0`.
pub fn std_dev(xs: &[f64]) -> f64 {
    variance(xs).sqrt()
}

/// Range `max − min`.
///
/// Empty slice or a single element → `0.0`.
pub fn spread(xs: &[f64]) -> f64 {
    match min_max(xs) {
        Some((lo, hi)) => hi - lo,
        None => 0.0,
    }
}

/// The `(min, max)` pair, or `None` for an empty slice.
///
/// `NaN` values are ignored by the underlying `f64::min` / `f64::max` folds.
pub fn min_max(xs: &[f64]) -> Option<(f64, f64)> {
    if xs.is_empty() {
        return None;
    }
    let lo = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some((lo, hi))
}

/// **Gini coefficient** of a set of non-negative values.
///
/// Computed from the relative mean absolute difference:
///
/// ```text
/// G = ( Σ_i Σ_j |xᵢ − xⱼ| ) / ( 2 · N² · μ )
/// ```
///
/// where `μ` is the [`mean`].  For non-negative inputs `G ∈ [0, 1)`: `0` when
/// all values are equal (perfect equality) and approaching `1` as a single
/// value holds all the mass.
///
/// Edge cases: an empty slice, a slice whose values are all `0`, or any slice
/// with `μ = 0` → `0.0` (perfect equality / undefined-mass treated as equal).
/// Negative inputs are not meaningful for a Gini coefficient and are not
/// special-cased; pass non-negative data.
pub fn gini(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n == 0 {
        return 0.0;
    }
    let m = mean(xs);
    if m <= 0.0 {
        return 0.0;
    }
    let mut sum_abs_diff = 0.0;
    for &xi in xs {
        for &xj in xs {
            sum_abs_diff += (xi - xj).abs();
        }
    }
    sum_abs_diff / (2.0 * (n as f64).powi(2) * m)
}

/// **Shannon entropy** (natural-log base) of a distribution.
///
/// The input is treated as unnormalized weights (counts or shares); it is
/// normalized internally to a probability distribution `pᵢ = wᵢ / Σ w`, then
/// `H = − Σ pᵢ · ln pᵢ` (zero-probability terms contribute `0`).  Units are
/// **nats**.  `H = 0` for a point mass and `H = ln k` for `k` equal classes.
///
/// Edge cases: an empty slice, or weights summing to `≤ 0` → `0.0`.  Negative
/// weights are ignored (treated as `0`).
pub fn shannon_entropy(weights: &[f64]) -> f64 {
    let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let mut h = 0.0;
    for &w in weights {
        if w > 0.0 {
            let p = w / total;
            h -= p * p.ln();
        }
    }
    h
}

/// **Herfindahl–Hirschman Index** — concentration of a set of shares.
///
/// The input is treated as unnormalized weights (counts or shares), normalized
/// internally to shares `sᵢ = wᵢ / Σ w`, then `HHI = Σ sᵢ²`.  For `k` positive
/// classes `HHI ∈ [1/k, 1]`: `1/k` when all shares are equal (least
/// concentrated) and `1` for a single dominant class (most concentrated).
///
/// Edge cases: an empty slice, or weights summing to `≤ 0` → `0.0`.  Negative
/// weights are ignored (treated as `0`).
pub fn hhi(weights: &[f64]) -> f64 {
    let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
    if total <= 0.0 {
        return 0.0;
    }
    weights
        .iter()
        .filter(|w| **w > 0.0)
        .map(|&w| (w / total).powi(2))
        .sum()
}

/// **Simpson diversity index** `1 − Σ pᵢ²` of a set of counts.
///
/// The complement of [`hhi`]: the probability that two items drawn with
/// replacement belong to *different* classes.  `0` for a single class,
/// approaching `1 − 1/k` as `k` equal classes spread the mass.
///
/// Edge cases: an empty slice, or counts summing to `≤ 0` → `0.0`.
pub fn simpson_diversity(counts: &[f64]) -> f64 {
    let total: f64 = counts.iter().filter(|w| **w > 0.0).sum();
    if total <= 0.0 {
        return 0.0;
    }
    1.0 - hhi(counts)
}

/// Number of distinct value clusters at tolerance `tol`.
///
/// Greedy single-linkage on the sorted values: the slice is sorted ascending
/// and a new cluster starts whenever the gap to the previous value exceeds
/// `tol`, so two values count as the same cluster iff they are linked by a
/// chain of `≤ tol` gaps.
///
/// This **exactly matches** the `distinct_clusters` helper in
/// `socsim-packs/src/opinion.rs` (it is a drop-in replacement): empty slice →
/// `0`, and the gap test is strict (`> tol` starts a new cluster).
pub fn distinct_clusters(values: &[f64], tol: f64) -> usize {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut clusters = 1usize;
    let mut last = sorted[0];
    for &x in &sorted[1..] {
        if (x - last).abs() > tol {
            clusters += 1;
        }
        last = x;
    }
    clusters
}

/// **Sarle's bimodality coefficient** `BC = (skewness² + 1) / kurtosis`.
///
/// Uses population moments: with mean `μ` and standard deviation `σ`,
/// skewness `g₁ = m₃ / σ³` and (non-excess) kurtosis `g₂ = m₄ / σ⁴`, where
/// `mₖ = (Σ (xᵢ − μ)ᵏ) / N`.  Then `BC = (g₁² + 1) / g₂`.  Values above
/// `≈ 5/9 ≈ 0.555` (the value for a uniform distribution) suggest bimodality;
/// a single normal mode gives `BC ≈ 1/3`.
///
/// Guards on small / degenerate input: fewer than 2 elements, or zero variance
/// (all values equal, so `σ = 0` and kurtosis is undefined) → `0.0`.
pub fn bimodality_coefficient(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let var = variance(xs);
    if var <= 0.0 {
        return 0.0;
    }
    let sd = var.sqrt();
    let nf = n as f64;
    let m3 = xs.iter().map(|x| (x - m).powi(3)).sum::<f64>() / nf;
    let m4 = xs.iter().map(|x| (x - m).powi(4)).sum::<f64>() / nf;
    let skew = m3 / sd.powi(3);
    let kurt = m4 / sd.powi(4);
    if kurt <= 0.0 {
        return 0.0;
    }
    (skew * skew + 1.0) / kurt
}

/// **Extremeness**: mean absolute distance of each value from `center`,
/// `(Σ |xᵢ − center|) / N`.
///
/// A clear, well-defined primitive for "how far, on average, are opinions from
/// a neutral point".  Empty slice → `0.0`.
pub fn extremeness(xs: &[f64], center: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().map(|x| (x - center).abs()).sum::<f64>() / xs.len() as f64
}

/// **Polarization** of an opinion set, defined here as the **population
/// standard deviation** [`std_dev`] of the values.
///
/// **This is one convention, not a universal definition.** Polarization has
/// many operationalizations in the literature (bimodality, distance-to-mean,
/// pairwise distance, group-based indices, …).  We deliberately pick a single,
/// transparent one — dispersion around the mean — because it is monotone in
/// "how spread out opinions are" and needs no opinion-range argument.  Callers
/// who need a *bimodality*-sensitive measure should use
/// [`bimodality_coefficient`]; callers who need distance from a fixed neutral
/// point should use [`extremeness`].  For a dispersion measure normalized to a
/// known range, divide by the maximum attainable standard deviation for that
/// range yourself.
pub fn polarization(xs: &[f64]) -> f64 {
    std_dev(xs)
}

/// Maximum per-index absolute change `maxᵢ |currᵢ − prevᵢ|`.
///
/// A convergence diagnostic: the largest single-element move between two
/// snapshots.  Only the first `min(prev.len(), curr.len())` indices are
/// compared, so **mismatched lengths are tolerated** (extra elements in the
/// longer slice are ignored).  Empty overlap → `0.0`.
///
/// Note: `socsim-mechanisms` currently ships its own copy of this function.
/// This is an independent (temporarily duplicated) implementation; once this
/// crate is adopted, `socsim-mechanisms` is expected to re-export this one.
pub fn max_abs_delta(prev: &[f64], curr: &[f64]) -> f64 {
    prev.iter()
        .zip(curr.iter())
        .map(|(p, c)| (c - p).abs())
        .fold(0.0, f64::max)
}

/// Mean per-index absolute change `(Σᵢ |currᵢ − prevᵢ|) / n` over the
/// overlapping prefix.
///
/// Only the first `min(prev.len(), curr.len())` indices are compared (mismatched
/// lengths tolerated; extra trailing elements ignored).  Empty overlap → `0.0`.
pub fn mean_abs_delta(prev: &[f64], curr: &[f64]) -> f64 {
    let n = prev.len().min(curr.len());
    if n == 0 {
        return 0.0;
    }
    prev.iter()
        .zip(curr.iter())
        .map(|(p, c)| (c - p).abs())
        .sum::<f64>()
        / n as f64
}

/// Number of distinct values in a slice of categorical labels.
///
/// Useful for "how many cultural regions / distinct traits survive".  Empty
/// slice → `0`.
pub fn num_distinct(labels: &[u32]) -> usize {
    let mut v = labels.to_vec();
    v.sort_unstable();
    v.dedup();
    v.len()
}

/// Share of the most common label: `(count of the modal value) / N`.
///
/// Useful for "what fraction of the population shares the dominant culture /
/// market leader's choice".  In `[1/k, 1]` for `k` distinct present labels.
/// Empty slice → `0.0`.
pub fn largest_share(labels: &[u32]) -> f64 {
    let n = labels.len();
    if n == 0 {
        return 0.0;
    }
    let mut v = labels.to_vec();
    v.sort_unstable();
    let mut best = 0usize;
    let mut run = 0usize;
    let mut last: Option<u32> = None;
    for x in v {
        if Some(x) == last {
            run += 1;
        } else {
            run = 1;
            last = Some(x);
        }
        if run > best {
            best = run;
        }
    }
    best as f64 / n as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn mean_variance_std_basic() {
        approx(mean(&[1.0, 1.0, 1.0]), 1.0);
        approx(variance(&[1.0, 1.0, 1.0]), 0.0);
        approx(std_dev(&[1.0, 1.0, 1.0]), 0.0);
        // Population variance of [1,2,3] = ((1)+0+(1))/3 = 2/3.
        approx(variance(&[1.0, 2.0, 3.0]), 2.0 / 3.0);
        // Empty → 0.
        approx(mean(&[]), 0.0);
        approx(variance(&[]), 0.0);
    }

    #[test]
    fn spread_and_min_max() {
        approx(spread(&[3.0, 1.0, 4.0, 1.0]), 3.0);
        approx(spread(&[]), 0.0);
        approx(spread(&[7.0]), 0.0);
        assert_eq!(min_max(&[]), None);
        assert_eq!(min_max(&[2.0, -1.0, 5.0]), Some((-1.0, 5.0)));
    }

    #[test]
    fn gini_equal_is_zero() {
        approx(gini(&[5.0, 5.0, 5.0, 5.0]), 0.0);
        approx(gini(&[]), 0.0);
        approx(gini(&[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn gini_one_holds_all() {
        // [0,0,0,1]: Σ_i Σ_j |xi-xj| = 6 (3 pairs *2 directions *1), N=4, mean=0.25.
        // G = 6 / (2 * 16 * 0.25) = 6 / 8 = 0.75.
        approx(gini(&[0.0, 0.0, 0.0, 1.0]), 0.75);
    }

    #[test]
    fn shannon_entropy_known() {
        // Two equal classes → ln 2.
        approx(shannon_entropy(&[1.0, 1.0]), std::f64::consts::LN_2);
        // Point mass → 0.
        approx(shannon_entropy(&[5.0, 0.0, 0.0]), 0.0);
        // Four equal classes → ln 4.
        approx(shannon_entropy(&[2.0, 2.0, 2.0, 2.0]), (4.0_f64).ln());
        approx(shannon_entropy(&[]), 0.0);
    }

    #[test]
    fn hhi_equal_shares_is_one_over_k() {
        approx(hhi(&[1.0, 1.0, 1.0, 1.0]), 0.25);
        approx(hhi(&[3.0, 3.0]), 0.5);
        approx(hhi(&[10.0]), 1.0);
        approx(hhi(&[]), 0.0);
    }

    #[test]
    fn simpson_complements_hhi() {
        approx(simpson_diversity(&[1.0, 1.0, 1.0, 1.0]), 0.75);
        approx(simpson_diversity(&[10.0]), 0.0);
        approx(simpson_diversity(&[]), 0.0);
    }

    #[test]
    fn distinct_clusters_matches_packs_cases() {
        // Two tight groups at ~0 and ~1 (same as socsim-packs test).
        let ops = vec![0.0, 0.001, 0.002, 1.0, 1.001];
        assert_eq!(distinct_clusters(&ops, 0.01), 2);
        // All distinct beyond tol.
        let ops = vec![0.0, 0.5, 1.0];
        assert_eq!(distinct_clusters(&ops, 0.01), 3);
        // Empty.
        assert_eq!(distinct_clusters(&[], 0.01), 0);
    }

    #[test]
    fn bimodality_guards_and_orders() {
        // Degenerate.
        approx(bimodality_coefficient(&[1.0]), 0.0);
        approx(bimodality_coefficient(&[2.0, 2.0, 2.0]), 0.0);
        // A sharply bimodal set scores higher than a unimodal-ish one.
        let bimodal = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let unimodal = vec![0.4, 0.5, 0.5, 0.5, 0.5, 0.6];
        assert!(
            bimodality_coefficient(&bimodal) > bimodality_coefficient(&unimodal),
            "bimodal {} should exceed unimodal {}",
            bimodality_coefficient(&bimodal),
            bimodality_coefficient(&unimodal)
        );
        // Perfectly split two-point set → BC = 1 (skew 0, kurtosis 1).
        approx(bimodality_coefficient(&[0.0, 1.0]), 1.0);
    }

    #[test]
    fn extremeness_and_polarization() {
        approx(extremeness(&[0.0, 1.0], 0.5), 0.5);
        approx(extremeness(&[], 0.5), 0.0);
        // polarization == std_dev.
        approx(polarization(&[1.0, 2.0, 3.0]), std_dev(&[1.0, 2.0, 3.0]));
        approx(polarization(&[4.0, 4.0]), 0.0);
    }

    #[test]
    fn deltas() {
        approx(max_abs_delta(&[0.0, 0.0, 0.0], &[0.1, -0.3, 0.2]), 0.3);
        approx(mean_abs_delta(&[0.0, 0.0], &[0.2, 0.4]), 0.3);
        // Mismatched lengths: compare overlapping prefix only.
        approx(max_abs_delta(&[0.0, 0.0], &[1.0]), 1.0);
        approx(mean_abs_delta(&[1.0], &[1.0, 99.0]), 0.0);
        approx(max_abs_delta(&[], &[]), 0.0);
        approx(mean_abs_delta(&[], &[]), 0.0);
    }

    #[test]
    fn categorical_helpers() {
        assert_eq!(num_distinct(&[1, 1, 2, 3, 3, 3]), 3);
        assert_eq!(num_distinct(&[]), 0);
        approx(largest_share(&[1, 1, 2, 3, 3, 3]), 0.5); // three 3's of six.
        approx(largest_share(&[7, 7, 7]), 1.0);
        approx(largest_share(&[]), 0.0);
    }
}
