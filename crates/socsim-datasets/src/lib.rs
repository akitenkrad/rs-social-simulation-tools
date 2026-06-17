//! Survey dataset schemas, metadata/registry, and optional acquisition for
//! socsim replications (engine-free).
//!
//! This crate is the single source of truth for the *dataset-specific* side of
//! survey replications:
//!
//! - **Schemas** ([`anes`], [`ces`]): per-survey-year [`socsim_survey::SurveySchema`]
//!   builders that declare the real raw-column names and value-code -> label
//!   maps. These are the canonical ANES 2012 / 2016 / 2020 schemas (moved here
//!   verbatim from `socsim-survey`), plus the CES 2022 Common Content schema.
//! - **Registry** ([`registry`]): machine-readable [`DatasetMeta`] / [`DataFile`]
//!   / [`Source`] records — DOI, source URL, citation, license, and the list of
//!   files (with their `sha256` / `expect_rows` for verification) for each
//!   dataset.
//! - **Acquisition** ([`acquire`], behind the optional `acquire` feature):
//!   download from source URLs, atomic-write into a local cache, verify
//!   `sha256` + row counts, and a Rust port of the pipe-delimited raw -> CSV
//!   converter.
//!
//! # Data is never vendored
//!
//! No raw survey data ships in this repository. The registry records *where*
//! the data comes from and *how* to verify it; the `acquire` feature fetches it
//! on demand into a consuming repo's (gitignored) `data/` directory. Files that
//! are license-gated (e.g. raw ANES Time Series microdata, which requires a free
//! electionstudies.org account and a data-use agreement) are declared as
//! [`Source::Manual`] with the instructions URL rather than auto-downloaded.
//!
//! # Engine-free
//!
//! Depends only on `socsim-survey` (for the [`socsim_survey::SurveySchema`] /
//! [`socsim_survey::DemoVar`] / [`socsim_survey::ValMap`] / [`socsim_survey::AgeBins`]
//! / [`socsim_survey::OutcomeMap`] types) plus, under the `acquire` feature,
//! `ureq` + `sha2` + `csv` + `tempfile` + `anyhow`. It pulls in no
//! `socsim-core` / engine crate.

#![forbid(unsafe_code)]

pub mod anes;
pub mod ces;
pub mod registry;

#[cfg(feature = "acquire")]
pub mod acquire;

pub use registry::{DataFile, DatasetMeta, Source};

/// All datasets known to the registry, in a stable order.
///
/// The order is fixed (ANES 2012 → 2016 → 2020 → CES 2022) so callers — e.g.
/// `socsim datasets list` — can rely on a deterministic listing. Each entry is
/// a `&'static` borrow of the per-dataset [`DatasetMeta`] `const`.
pub fn all() -> Vec<&'static DatasetMeta> {
    vec![
        &anes::ANES_2012,
        &anes::ANES_2016,
        &anes::ANES_2020,
        &ces::CES_2022_META,
    ]
}

/// Look up a dataset by its stable [`DatasetMeta::key`] (e.g. `"anes-2020"`).
///
/// Returns `None` if no registered dataset has that key. Lookup is over
/// [`all`], so it sees exactly the datasets compiled into the registry.
pub fn by_key(key: &str) -> Option<&'static DatasetMeta> {
    all().into_iter().find(|m| m.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_four_datasets() {
        assert_eq!(all().len(), 4);
    }

    #[test]
    fn by_key_finds_anes_2020() {
        let m = by_key("anes-2020").expect("anes-2020 present");
        assert_eq!(m.key, anes::ANES_2020.key);
        assert_eq!(m.name, anes::ANES_2020.name);
    }

    #[test]
    fn by_key_unknown_is_none() {
        assert!(by_key("nope").is_none());
    }
}
