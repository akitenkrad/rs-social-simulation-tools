//! Built-in CES 2022 Common Content [`SurveySchema`].
//!
//! The truth source is the **2022 Cooperative Election Study (CES) Common
//! Content** (Harvard Dataverse `doi:10.7910/DVN/PR4L8P`, version 4, ~60,000
//! respondents). This module ports the **native** CES demographic codings —
//! the real raw column names (`race` / `gender4` / `ideo5`) and their value
//! codes — verbatim from the CES 2022 codebook / pre-election questionnaire, so
//! a recode over the published microdata is faithful. Study-specific
//! categorizations (e.g. gong2026's White / Non-white binary collapse) are the
//! *consumer's* job, exactly as the ANES schemas expose native codings that a
//! study then maps — see [`crate::anes`].
//!
//! # Variables
//!
//! - [`race`] (`race`): the 8 native CES race categories. Note the codebook
//!   ordering is non-contiguous: `8` is *Middle Eastern* and `6`/`7` are *two
//!   or more races* / *other*.
//! - [`gender`] (`gender4`): the 4 native CES gender categories (`man`,
//!   `woman`, `non-binary`, `other`).
//! - [`ideology`] (`ideo5`): the 5-point liberal–conservative scale. CES code
//!   `6` ("Not sure") is intentionally **unmapped** (recodes to `None`) because
//!   it is not a point on the spectrum.
//!
//! # Outcome
//!
//! [`SurveySchema`] requires a single outcome map, but CES Common Content has no
//! single "vote" outcome with a fixed code map: the 2022 vote-choice variables
//! (`CC22_411`/`412`/`413`) are coded by *candidate slot*, with the candidate's
//! party carried in a separate per-respondent piped variable — so they cannot be
//! reduced to a fixed `(code -> party)` map here without fabricating one. We
//! therefore use a representative **fixed-coded policy item** as the canonical
//! outcome slot: `CC22_332a` ("Always allow a woman to obtain an abortion as a
//! matter of choice"), coded `1` = support, `2` = oppose. A study needing other
//! issue items (e.g. gong2026's 84 questions, each on its own 2/4/5-point scale)
//! declares its own per-question schema rather than reusing this slot.
//!
//! Provenance/acquisition metadata lives in [`CES_2022_META`]. The Common
//! Content release is **CC0 1.0 (public domain)** and directly downloadable from
//! the Harvard Dataverse access API, so the data file is a [`Source::Dataverse`]
//! (no account / terms acceptance required).

use socsim_survey::{DemoVar, OutcomeMap, SurveySchema, ValMap};

use crate::registry::{DataFile, DatasetMeta, Source};

/// The support-slot outcome label (CES `CC22_332a` code 1).
pub const OUTCOME_SUPPORT: &str = "support";
/// The oppose-slot outcome label (CES `CC22_332a` code 2).
pub const OUTCOME_OPPOSE: &str = "oppose";

// ---------------------------------------------------------------------------
// Native CES value maps (codes + labels verbatim from the CES 2022 codebook /
// pre-election questionnaire `demosfront`/`ideo5` items).
// ---------------------------------------------------------------------------

fn race_valmap() -> ValMap {
    ValMap::new(&[
        (1, "white"),
        (2, "black or African-American"),
        (3, "Hispanic or Latino"),
        (4, "Asian or Asian-American"),
        (5, "Native American"),
        (6, "two or more races"),
        (7, "other"),
        (8, "Middle Eastern"),
    ])
}

fn gender_valmap() -> ValMap {
    ValMap::new(&[(1, "man"), (2, "woman"), (3, "non-binary"), (4, "other")])
}

fn ideology_valmap() -> ValMap {
    // CES `ideo5` is a 5-point scale; code 6 ("Not sure") is left unmapped so it
    // recodes to None rather than being placed on the spectrum.
    ValMap::new(&[
        (1, "very liberal"),
        (2, "liberal"),
        (3, "moderate"),
        (4, "conservative"),
        (5, "very conservative"),
    ])
}

// ---------------------------------------------------------------------------
// Per-variable DemoVar builders (raw column names from the CES 2022 codebook).
// ---------------------------------------------------------------------------

/// `race` variable (CES raw column `race`).
pub fn race() -> DemoVar {
    DemoVar::valmap("race", "race", race_valmap())
}
/// `gender` variable (CES raw column `gender4`).
pub fn gender() -> DemoVar {
    DemoVar::valmap("gender", "gender4", gender_valmap())
}
/// `ideology` variable (CES raw column `ideo5`).
pub fn ideology() -> DemoVar {
    DemoVar::valmap("ideology", "ideo5", ideology_valmap())
}

/// The canonical outcome map: CES `CC22_332a` (abortion as a matter of choice),
/// `1` = support, `2` = oppose.
fn outcome() -> OutcomeMap {
    OutcomeMap::new("CC22_332a", &[(1, OUTCOME_SUPPORT), (2, OUTCOME_OPPOSE)])
}

// ---------------------------------------------------------------------------
// Schema.
// ---------------------------------------------------------------------------

/// The CES 2022 Common Content schema (native race / gender / ideology codings
/// + a representative fixed-coded policy outcome).
///
/// Distribution estimation runs through [`socsim_survey::estimate_distributions`]
/// on records loaded from the CES 2022 Common Content CSV. The variable *set* is
/// the three demographics gong2026 conditions on (race, gender, ideology); a
/// study wanting more demographics (age from `birthyr`, `educ`, `pid7`, …) or a
/// different outcome item declares its own schema by adding [`DemoVar`]s.
pub fn ces_2022() -> SurveySchema {
    SurveySchema::builder("CES 2022 Common Content")
        .var(race())
        .var(gender())
        .var(ideology())
        .outcome(outcome())
        .build()
}

/// Built-in CES schema for a supported year (2022 only).
pub fn ces(year: u16) -> Option<SurveySchema> {
    match year {
        2022 => Some(ces_2022()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Registry metadata.
//
// The CES 2022 Common Content is released CC0 1.0 (public domain) and is
// directly downloadable from the Harvard Dataverse access API, so the data file
// is a `Source::Dataverse` (no account / terms acceptance). The CSV is pinned by
// its sha256 and known respondent count (60,000) for download verification.
// ---------------------------------------------------------------------------

const CES_2022_FILES: &[DataFile] = &[DataFile {
    // The CES 2022 Common Content release CSV (Dataverse file id 10140882,
    // published as `CCES22_Common_OUTPUT_vv_topost.csv`).
    logical_name: "ces_2022_common.csv",
    source: Source::Dataverse {
        base: "https://dataverse.harvard.edu",
        file_id: 10140882,
    },
    sha256: Some("dcdaeba631e0bdf50f3d7dcf4dcde7cc980e4281ff5dab87efcaa7353ca089bc"),
    expect_rows: Some(60000),
}];

/// Provenance/acquisition metadata for the CES 2022 Common Content.
pub const CES_2022_META: DatasetMeta = DatasetMeta {
    key: "ces-2022",
    name: "CES 2022 Common Content",
    doi: Some("10.7910/DVN/PR4L8P"),
    source_url: "https://doi.org/10.7910/DVN/PR4L8P",
    citation: "Schaffner, Brian; Ansolabehere, Stephen; Shih, Marissa. \
               Cooperative Election Study Common Content, 2022. Harvard Dataverse.",
    license: "CC0 1.0 (public domain): \
              http://creativecommons.org/publicdomain/zero/1.0 — no account or \
              terms acceptance required to download.",
    files: CES_2022_FILES,
};

/// Registry metadata for a supported CES year (2022 only).
pub fn meta(year: u16) -> Option<&'static DatasetMeta> {
    match year {
        2022 => Some(&CES_2022_META),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use socsim_survey::{estimate_distributions, Record};
    use std::collections::HashMap;

    /// A synthetic CES record over the real column names.
    fn rec(race: &str, gender4: &str, ideo5: &str, cc22_332a: &str) -> Record {
        let mut m: HashMap<String, String> = HashMap::new();
        m.insert("race".into(), race.into());
        m.insert("gender4".into(), gender4.into());
        m.insert("ideo5".into(), ideo5.into());
        m.insert("CC22_332a".into(), cc22_332a.into());
        m
    }

    #[test]
    fn schema_builds_with_expected_keys() {
        let schema = ces_2022();
        assert_eq!(schema.name, "CES 2022 Common Content");
        // Scan order is declaration order: race, gender, ideology.
        assert_eq!(schema.var_keys(), vec!["race", "gender", "ideology"]);
    }

    #[test]
    fn native_codes_recode_to_expected_labels() {
        let schema = ces_2022();
        let r = rec("8", "3", "5", "1");
        // race code 8 is Middle Eastern (non-contiguous codebook ordering).
        assert_eq!(
            schema.var("race").unwrap().recode_label(&r).as_deref(),
            Some("Middle Eastern")
        );
        assert_eq!(
            schema.var("gender").unwrap().recode_label(&r).as_deref(),
            Some("non-binary")
        );
        assert_eq!(
            schema.var("ideology").unwrap().recode_label(&r).as_deref(),
            Some("very conservative")
        );
    }

    #[test]
    fn ideology_not_sure_is_unmapped() {
        let schema = ces_2022();
        // ideo5 == 6 ("Not sure") recodes to None.
        let r = rec("1", "1", "6", "1");
        assert!(schema.var("ideology").unwrap().recode_label(&r).is_none());
    }

    #[test]
    fn estimate_distributions_runs_on_synthetic_records() {
        let schema = ces_2022();
        let records = vec![
            // white man very-liberal supporter
            rec("1", "1", "1", "1"),
            // black woman very-conservative opposer
            rec("2", "2", "5", "2"),
            // white woman moderate supporter
            rec("1", "2", "3", "1"),
        ];
        let dist = estimate_distributions(&records, &schema);

        // Race: 2 white / 1 black over 3 → labels sorted.
        let race = dist.demo("race").expect("race dist present");
        assert_eq!(race.labels, vec!["black or African-American", "white"]);
        assert!((race.probs.iter().sum::<f64>() - 1.0).abs() < 1e-12);

        // Outcome: 2 support / 1 oppose.
        assert_eq!(dist.outcome.total(), 3);
        assert!((dist.outcome.rate_of(OUTCOME_SUPPORT) - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn ces_lookup_and_meta() {
        assert!(ces(2022).is_some());
        assert!(ces(2020).is_none());
        assert_eq!(meta(2022).unwrap().key, "ces-2022");
        assert!(meta(2024).is_none());
    }
}
