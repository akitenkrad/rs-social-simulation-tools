//! Config-driven survey recode for socsim (engine-free).
//!
//! This crate generalizes the hard-coded, per-year ANES recode used by the
//! `sun2024` "Random Silicon Sampling" replication into a **data-driven
//! schema**. A [`SurveySchema`] describes, for one survey-year:
//!
//! - the raw CSV column name for each demographic variable, plus a
//!   per-variable value-code -> canonical-label map ([`ValMap`]);
//! - an age-binning rule ([`AgeBins`]) for the continuous age column;
//! - the outcome (vote) column plus a code -> outcome-label map.
//!
//! Given a schema, the generic [`recode_row`] / [`demo_label`] /
//! [`actual_outcome`] / [`estimate_distributions`] functions do the work that
//! sun2024 spelled out per-year in `common/anes.rs`. The match arms become
//! data: ANES 2012 / 2016 / 2020 ship as built-in schema builders
//! ([`anes_2012`], [`anes_2016`], [`anes_2020`]) that port the **exact**
//! V-variable column names and value maps from sun2024 — parity matters, so
//! none of those mappings are changed here.
//!
//! # Demographics are extensible
//!
//! sun2024 uses 8 demographic variables (race, gender, age, ideology, party id,
//! political interest, church attendance, discuss politics). A [`SurveySchema`]
//! declares its own *set* of variables: a schema lists exactly the
//! [`DemoVar`]s it covers (keyed by a stable snake_case string), so a newer
//! survey can add or drop variables without touching this crate. The eight ANES
//! variables are provided as [`DemoVar`] constants for convenience.
//!
//! # CES 2022 extension point
//!
//! This crate does **not** ship a CES 2022 schema, because the CES V-variable
//! column names and value codes are not available here and must not be
//! fabricated. The [`SurveySchema`] struct *is* the extension API: a CES schema
//! is declared exactly the way the ANES builders are. See the
//! `// TODO(gong2026): CES 2022 schema` skeleton in [`anes_2020`]'s module
//! documentation below and the [`SurveySchema::builder`] doc example. CES is
//! considered complete once `gong2026` wires its real column names and maps.
//!
//! Engine-free: depends only on `std`, `serde`, and `csv`. It pulls in no
//! `socsim-core` / engine crate.

#![forbid(unsafe_code)]

mod distribution;
mod schema;

pub mod anes;

pub use distribution::{estimate_distributions, CategoryDist, Distributions, OutcomeDistribution};
pub use schema::{
    actual_outcome, demo_label, recode_row, AgeBins, DemoVar, OutcomeMap, Recode, RecodedRow,
    SurveySchema, SurveySchemaBuilder, ValMap,
};

use std::collections::HashMap;

/// A single raw CSV record: column name -> raw cell value.
///
/// This matches sun2024's `load_named_records` representation (ANES `.tab`
/// files are comma-separated despite the extension, and column sets differ per
/// year, so records are read as name->value maps rather than fixed structs).
pub type Record = HashMap<String, String>;

/// Read a header-bearing CSV into a vector of [`Record`]s (column -> value).
///
/// The delimiter is `,` even for `.tab` files, matching sun2024's
/// `load_named_records`. Returns a [`csv::Error`] on open/parse failure.
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
/// Ported verbatim from sun2024's `raw_code`: tolerates pandas-float strings
/// such as `"29.0"` (parses as f64 then rounds), and treats empty cells and
/// negative codes (ANES missing/non-applicable) as `None`.
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
