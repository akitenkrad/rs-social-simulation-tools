//! Paper-anchor PASS/off reproduction harness for socsim (engine-free).
//!
//! A **paper-agnostic** offline-verification harness: a reproduction run does
//! **not** re-run generation; it reads cached observed values and compares them
//! against a paper's reference values ("anchors"), emitting a machine-readable
//! PASS / off / NO_DATA summary.
//!
//! The crate ships the *mechanics* — the [`Anchor`] shape, the
//! [`compare_anchor`] classification (tolerance / upper-bound), the
//! [`ReproduceRow`] output rows, the [`build_rows`] join, the CSV writers, and a
//! generic [`find_latest`] directory scanner — but it ships **no** anchor
//! values. Each paper supplies its own `&[Anchor]` slice plus an observation
//! lookup closure, so a paper's reference values stay in the paper's own crate.
//!
//! Originally extracted from the sun2024 replication; the comparison and CSV
//! semantics are preserved so a replication can migrate onto this crate without
//! changing its outputs.
//!
//! Depends on [`socsim_results`] for the CSV writer (and shares its
//! timestamped-results-dir conventions); it pulls in no engine crate.
//!
//! # Example
//!
//! ```no_run
//! use socsim_reproduce::{Anchor, build_rows, write_reproduce_summary, write_paper_anchors};
//!
//! // A paper declares its own anchors (these live in the paper's crate).
//! static ANCHORS: &[Anchor] = &[Anchor {
//!     study: "A",
//!     table_or_fig: "Table 1",
//!     condition: "overall",
//!     metric: "outcome_rate",
//!     paper_value: 0.5743,
//!     tolerance: 0.02,
//!     upper_bound: false,
//!     note: "mean outcome rate over 10 iterations",
//! }];
//!
//! // The paper supplies an observation lookup (here: a fixed value).
//! let rows = build_rows(ANCHORS, |a| match a.metric {
//!     "outcome_rate" => Some(0.574),
//!     _ => None,
//! });
//!
//! write_paper_anchors(ANCHORS, "paper_anchors.csv")?;
//! write_reproduce_summary(&rows, "reproduce_summary.csv")?;
//! # Ok::<(), socsim_results::WriteError>(())
//! ```

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use serde::Serialize;
use socsim_results::{write_csv, WriteError};

/// One paper reference value to verify against an observed value.
///
/// `tolerance` is "observed within this band of `paper_value` passes" for
/// centred metrics; for upper-bound metrics (KL gates etc.) set
/// [`Anchor::upper_bound`] and `tolerance` holds the upper bound itself.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    /// Study label (e.g. `"A"`).
    pub study: &'static str,
    /// Table/figure label (e.g. `"Table 1"`).
    pub table_or_fig: &'static str,
    /// Condition (e.g. `"overall"` / `"2020"`).
    pub condition: &'static str,
    /// Metric name (e.g. `"rss_biden_rate"`).
    pub metric: &'static str,
    /// The paper's reference value.
    pub paper_value: f64,
    /// Tolerance (`paper_value ± tolerance` passes; or the upper bound itself
    /// when `upper_bound` is `true`).
    pub tolerance: f64,
    /// `true`: passes when `observed < tolerance` (KL-gate style upper bound).
    /// `false`: passes when `|observed - paper_value| <= tolerance` (centred).
    pub upper_bound: bool,
    /// Provenance / note (human-readable; preserved in the anchors CSV).
    pub note: &'static str,
}

/// Outcome of comparing one anchor against an observed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorStatus {
    /// Observed value within tolerance.
    Pass,
    /// Observed value outside tolerance.
    Off,
    /// No observed value available (reproduction allows partial runs).
    NoData,
}

impl AnchorStatus {
    /// CSV/log tag (`"PASS"` / `"off"` / `"NO_DATA"`).
    pub fn tag(&self) -> &'static str {
        match self {
            AnchorStatus::Pass => "PASS",
            AnchorStatus::Off => "off",
            AnchorStatus::NoData => "NO_DATA",
        }
    }
}

/// Classify one anchor against an observed value (`None` = no data).
///
/// - `upper_bound`: PASS when `observed < tolerance` (strict less-than; the
///   bound itself is *off*).
/// - otherwise: PASS when `|observed - paper_value| <= tolerance`.
pub fn compare_anchor(anchor: &Anchor, observed: Option<f64>) -> AnchorStatus {
    match observed {
        None => AnchorStatus::NoData,
        Some(v) => {
            let pass = if anchor.upper_bound {
                v < anchor.tolerance
            } else {
                (v - anchor.paper_value).abs() <= anchor.tolerance
            };
            if pass {
                AnchorStatus::Pass
            } else {
                AnchorStatus::Off
            }
        }
    }
}

/// One reproduction summary row (the CSV output unit).
///
/// Numeric columns are pre-formatted as fixed 6-decimal strings (`{:.6}`) so
/// [`socsim_results::write_csv`] emits stable, byte-deterministic output:
/// `paper_value` / `tolerance` are always `{:.6}`, and `observed_value` is
/// `{:.6}` when present or the **empty string** when the observation is missing.
///
/// The original `f64` / `Option<f64>` are still available via
/// [`ReproduceRow::paper_value`], [`ReproduceRow::observed_value`], and
/// [`ReproduceRow::tolerance`] for programmatic use; only the *serialized* form
/// is the fixed-precision string.
#[derive(Debug, Clone, Serialize)]
pub struct ReproduceRow {
    /// Study label.
    pub study: String,
    /// Table/figure label.
    pub table_or_fig: String,
    /// Condition.
    pub condition: String,
    /// Metric name.
    pub metric: String,
    /// Paper reference value, pre-formatted `{:.6}`.
    pub paper_value: String,
    /// Observed value, pre-formatted `{:.6}` (empty string when absent).
    pub observed_value: String,
    /// Tolerance / upper bound, pre-formatted `{:.6}`.
    pub tolerance: String,
    /// Classification tag (`"PASS"` / `"off"` / `"NO_DATA"`).
    pub status: String,
}

impl ReproduceRow {
    /// Parse the pre-formatted `paper_value` back to `f64`.
    pub fn paper_value(&self) -> f64 {
        self.paper_value.parse().unwrap_or(0.0)
    }

    /// The observed value as `Option<f64>` (`None` when the column is empty).
    pub fn observed_value(&self) -> Option<f64> {
        if self.observed_value.is_empty() {
            None
        } else {
            self.observed_value.parse().ok()
        }
    }

    /// Parse the pre-formatted `tolerance` back to `f64`.
    pub fn tolerance(&self) -> f64 {
        self.tolerance.parse().unwrap_or(0.0)
    }
}

/// Format an observed value: `{:.6}` or the empty string when `None`.
fn fmt_observed(observed: Option<f64>) -> String {
    observed.map(|v| format!("{v:.6}")).unwrap_or_default()
}

/// Join a paper's anchors against an observation lookup, classifying each.
///
/// The caller passes its own `anchors` slice plus an `observed` closure mapping
/// each anchor to its observed value (`None` = no data). The per-anchor
/// classification is [`compare_anchor`]. Numeric columns are stored
/// pre-formatted `{:.6}` for byte-deterministic output (see [`ReproduceRow`]).
pub fn build_rows<F>(anchors: &[Anchor], observed: F) -> Vec<ReproduceRow>
where
    F: Fn(&Anchor) -> Option<f64>,
{
    anchors
        .iter()
        .map(|a| {
            let obs = observed(a);
            ReproduceRow {
                study: a.study.to_string(),
                table_or_fig: a.table_or_fig.to_string(),
                condition: a.condition.to_string(),
                metric: a.metric.to_string(),
                paper_value: format!("{:.6}", a.paper_value),
                observed_value: fmt_observed(obs),
                tolerance: format!("{:.6}", a.tolerance),
                status: compare_anchor(a, obs).tag().to_string(),
            }
        })
        .collect()
}

/// A row of the canonical paper-anchors table (observation-independent).
///
/// `paper_value` / `tolerance` are pre-formatted `{:.6}` strings for
/// byte-deterministic output, and `comparison` is `"upper_bound"` or
/// `"tolerance"`.
#[derive(Debug, Clone, Serialize)]
struct AnchorRow {
    study: String,
    table_or_fig: String,
    condition: String,
    metric: String,
    paper_value: String,
    tolerance: String,
    comparison: String,
    note: String,
}

/// Write `reproduce_summary.csv` (the PASS/off comparison) to `path`.
///
/// Uses [`socsim_results::write_csv`]; columns are study, table_or_fig,
/// condition, metric, paper_value, observed_value, tolerance, status.
pub fn write_reproduce_summary(
    rows: &[ReproduceRow],
    path: impl AsRef<Path>,
) -> Result<(), WriteError> {
    write_csv(rows, path)
}

/// Write `paper_anchors.csv` (the canonical reference table) to `path`.
///
/// Observation-independent — always writable. Uses
/// [`socsim_results::write_csv`].
pub fn write_paper_anchors(anchors: &[Anchor], path: impl AsRef<Path>) -> Result<(), WriteError> {
    let rows: Vec<AnchorRow> = anchors
        .iter()
        .map(|a| AnchorRow {
            study: a.study.to_string(),
            table_or_fig: a.table_or_fig.to_string(),
            condition: a.condition.to_string(),
            metric: a.metric.to_string(),
            paper_value: format!("{:.6}", a.paper_value),
            tolerance: format!("{:.6}", a.tolerance),
            comparison: if a.upper_bound {
                "upper_bound"
            } else {
                "tolerance"
            }
            .to_string(),
            note: a.note.to_string(),
        })
        .collect();
    write_csv(&rows, path)
}

/// Scan `results_root` for the newest timestamped run directory satisfying
/// `predicate`.
///
/// Real subdirectories (the `latest` symlink is excluded) are visited in
/// **descending name order**
/// (newest timestamp first), and the first whose path satisfies `predicate` is
/// returned. The predicate receives the candidate directory path so a paper can
/// require e.g. a particular sidecar file or a `config.json` field. Returns
/// `Ok(None)` when `results_root` is not a directory or nothing matches.
pub fn find_latest<P>(results_root: &Path, predicate: P) -> std::io::Result<Option<PathBuf>>
where
    P: Fn(&Path) -> bool,
{
    if !results_root.is_dir() {
        return Ok(None);
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(results_root)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && p.file_name().map(|n| n != "latest").unwrap_or(false))
        .collect();
    // Descending by directory name (timestamp): newest first.
    dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    Ok(dirs.into_iter().find(|d| predicate(d)))
}
