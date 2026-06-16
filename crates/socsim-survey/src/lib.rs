//! Config-driven survey recode for socsim (engine-free).
//!
//! A **data-driven schema** for recoding survey microdata. A [`SurveySchema`]
//! describes, for one survey-year:
//!
//! - the raw CSV column name for each demographic variable, plus a
//!   per-variable value-code -> canonical-label map ([`ValMap`]);
//! - an age-binning rule ([`AgeBins`]) for the continuous age column;
//! - the outcome column plus a code -> outcome-label map.
//!
//! Given a schema, the generic [`recode_row`] / [`demo_label`] /
//! [`actual_outcome`] / [`estimate_distributions`] functions do the recoding.
//! The design generalizes the kind of hard-coded, per-year ANES-style recode a
//! survey replication would otherwise spell out: each year's match arms become
//! data declared in a schema.
//!
//! # Demographics are extensible
//!
//! A [`SurveySchema`] declares its own *set* of variables: a schema lists
//! exactly the [`DemoVar`]s it covers (keyed by a stable snake_case string), so
//! a newer survey can add or drop variables without touching this crate.
//!
//! # Built-in ANES presets (optional `anes` feature)
//!
//! Built-in ANES 2012 / 2016 / 2020 schema builders (`anes::anes_2012`,
//! `anes::anes_2016`, `anes::anes_2020`) ship behind the optional **`anes`
//! feature** (disabled by default). They declare the exact V-variable column
//! names and value maps for those years, plus the eight ANES demographic
//! [`DemoVar`] constants. The default build is the generic engine only.
//!
//! # Extension point
//!
//! The [`SurveySchema`] struct *is* the extension API: a new survey schema
//! (e.g. CES 2022) is declared exactly the way the built-in ANES builders are —
//! see the [`SurveySchema::builder`] doc example. Provide the survey's real
//! column names and value codes and build a schema; no change to this crate is
//! needed.
//!
//! Engine-free: depends only on `std`, `serde`, and `csv`. It pulls in no
//! `socsim-core` / engine crate.

#![forbid(unsafe_code)]

mod distribution;
mod schema;

#[cfg(feature = "anes")]
pub mod anes;

pub use distribution::{estimate_distributions, CategoryDist, Distributions, OutcomeDistribution};
pub use schema::{
    actual_outcome, demo_label, recode_row, AgeBins, DemoVar, OutcomeMap, Recode, RecodedRow,
    SurveySchema, SurveySchemaBuilder, ValMap,
};

use std::collections::HashMap;

/// A single raw CSV record: column name -> raw cell value.
///
/// Records are read as name->value maps rather than fixed structs because
/// survey column sets differ per year (and ANES `.tab` files are comma-separated
/// despite the extension).
pub type Record = HashMap<String, String>;

/// Read a header-bearing CSV into a vector of [`Record`]s (column -> value).
///
/// The delimiter is `,` even for `.tab` files. Returns a [`csv::Error`] on
/// open/parse failure.
pub fn load_named_records<P: AsRef<std::path::Path>>(path: P) -> Result<Vec<Record>, csv::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b',')
        .has_headers(true)
        .from_path(path)?;
    let headers = reader.headers()?.clone();
    let mut out = Vec::new();
    for rec in reader.records() {
        let rec = rec?;
        let mut map = HashMap::with_capacity(headers.len());
        for (h, v) in headers.iter().zip(rec.iter()) {
            map.insert(h.to_string(), v.to_string());
        }
        out.push(map);
    }
    Ok(out)
}

/// Extract a non-negative integer code from a raw cell (column `key`).
///
/// Tolerates pandas-float strings such as `"29.0"` (parses as f64 then rounds),
/// and treats empty cells and negative codes (ANES-style missing/non-applicable)
/// as `None`.
pub fn raw_code(rec: &Record, key: &str) -> Option<i64> {
    let v = rec.get(key)?.trim();
    if v.is_empty() {
        return None;
    }
    let n = v.parse::<f64>().ok()?.round() as i64;
    if n < 0 {
        None
    } else {
        Some(n)
    }
}
