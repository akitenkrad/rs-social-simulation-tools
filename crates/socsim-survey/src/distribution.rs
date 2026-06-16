//! Empirical category distributions over recoded survey records.
//!
//! Ported from sun2024's `rss/distribution.rs` (`CategoryDist`,
//! `DemoDistributions::estimate`, `VoteDistribution::estimate`). Each
//! demographic variable's distribution is normalized over *its own* non-missing
//! sample; outcome counts only count non-missing outcomes.

use std::collections::HashMap;

use crate::schema::{actual_outcome, recode_row, SurveySchema};
use crate::Record;

/// One variable's empirical distribution (category label -> probability).
///
/// `labels` preserves a stable order (sorted, matching sun2024's
/// `from_counts`); `probs` is aligned with `labels` and sums to 1 (or all-zero
/// when the variable has no non-missing observations).
#[derive(Debug, Clone)]
pub struct CategoryDist {
    /// Category labels in stable (sorted) order.
    pub labels: Vec<String>,
    /// Per-label probability, aligned with `labels`, summing to 1.
    pub probs: Vec<f64>,
}

impl CategoryDist {
    /// Build an empirical distribution from a label -> count map.
    ///
    /// Labels are sorted for stability (sun2024 parity); an empty total yields
    /// all-zero probs.
    pub fn from_counts(counts: &HashMap<String, u64>) -> Self {
        let mut labels: Vec<String> = counts.keys().cloned().collect();
        labels.sort();
        let total: u64 = counts.values().sum();
        let probs = if total == 0 {
            vec![0.0; labels.len()]
        } else {
            labels
                .iter()
                .map(|l| counts[l] as f64 / total as f64)
                .collect()
        };
        CategoryDist { labels, probs }
    }
}

/// Empirical outcome (vote) distribution: per-outcome-label counts.
///
/// Ports sun2024's `VoteDistribution`, but generalized to arbitrary outcome
/// labels declared by the schema (not hard-coded Biden/Trump).
#[derive(Debug, Clone, Default)]
pub struct OutcomeDistribution {
    /// Outcome label -> count (non-missing outcomes only).
    pub counts: HashMap<String, u64>,
}

impl OutcomeDistribution {
    /// Total non-missing outcomes.
    pub fn total(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Count for one outcome label (0 if absent).
    pub fn count_of(&self, label: &str) -> u64 {
        self.counts.get(label).copied().unwrap_or(0)
    }

    /// Share of one outcome label over the non-missing total (0 if total is 0).
    pub fn rate_of(&self, label: &str) -> f64 {
        let t = self.total();
        if t == 0 {
            0.0
        } else {
            self.count_of(label) as f64 / t as f64
        }
    }
}

/// The full set of empirical distributions for a survey-year: one
/// [`CategoryDist`] per demographic variable, plus the outcome distribution.
#[derive(Debug, Clone)]
pub struct Distributions {
    /// Variable key (e.g. `"race"`) -> empirical category distribution.
    pub demos: HashMap<String, CategoryDist>,
    /// Empirical outcome (vote) distribution.
    pub outcome: OutcomeDistribution,
}

impl Distributions {
    /// The category distribution for a variable key, if present.
    pub fn demo(&self, key: &str) -> Option<&CategoryDist> {
        self.demos.get(key)
    }
}

/// Estimate the empirical [`Distributions`] over a slice of raw records.
///
/// Generic port of sun2024's `DemoDistributions::estimate` +
/// `VoteDistribution::estimate`: each variable is recoded per record, counts
/// are accumulated per variable (missing values skipped), and each variable is
/// normalized over its own non-missing sample. Outcome counts use
/// [`actual_outcome`].
pub fn estimate_distributions(records: &[Record], schema: &SurveySchema) -> Distributions {
    let mut per_var: HashMap<String, HashMap<String, u64>> = HashMap::new();
    for v in &schema.vars {
        per_var.insert(v.key.to_string(), HashMap::new());
    }
    let mut outcome_counts: HashMap<String, u64> = HashMap::new();

    for rec in records {
        let row = recode_row(rec, schema);
        for (key, label) in row.attrs {
            *per_var.get_mut(&key).unwrap().entry(label).or_insert(0) += 1;
        }
        if let Some(label) = actual_outcome(rec, schema) {
            *outcome_counts.entry(label.to_string()).or_insert(0) += 1;
        }
    }

    let demos = per_var
        .into_iter()
        .map(|(key, counts)| (key, CategoryDist::from_counts(&counts)))
        .collect();

    Distributions {
        demos,
        outcome: OutcomeDistribution {
            counts: outcome_counts,
        },
    }
}
