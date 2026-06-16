//! Ported sun2024 `common/anes.rs` recode tests + distribution tests.
//!
//! These ports are the parity guarantee: identical sample records must produce
//! identical recoded labels / outcomes / category frequencies as sun2024.

use std::collections::HashMap;

use socsim_survey::anes::{anes_2012, anes_2016, anes_2020, OUTCOME_DEM, OUTCOME_REP};
use socsim_survey::{
    actual_outcome, demo_label, estimate_distributions, recode_row, AgeBins, CategoryDist, Record,
    SurveySchema,
};

fn rec(pairs: &[(&str, &str)]) -> Record {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// --- ported: age_bins ------------------------------------------------------

#[test]
fn age_bins() {
    let b = AgeBins::anes_decade();
    assert_eq!(b.bin(25), Some("18-29"));
    assert_eq!(b.bin(33), Some("30-39"));
    assert_eq!(b.bin(80), Some("70+"));
    assert_eq!(b.bin(10), None);
}

// --- ported: vote_binarization_2020 ---------------------------------------

#[test]
fn vote_binarization_2020() {
    let s = anes_2020();
    let mut r = rec(&[("V202110x", "1")]);
    assert_eq!(actual_outcome(&r, &s), Some(OUTCOME_DEM));
    r.insert("V202110x".into(), "2".into());
    assert_eq!(actual_outcome(&r, &s), Some(OUTCOME_REP));
    r.insert("V202110x".into(), "-9".into());
    assert_eq!(actual_outcome(&r, &s), None);
}

// --- ported: vote_binarization_2012_2016 ----------------------------------

#[test]
fn vote_binarization_2012_2016() {
    let s12 = anes_2012();
    let mut r12 = rec(&[("presvote2012_x", "1")]);
    assert_eq!(actual_outcome(&r12, &s12), Some(OUTCOME_DEM)); // Obama / Dem slot
    r12.insert("presvote2012_x".into(), "2".into());
    assert_eq!(actual_outcome(&r12, &s12), Some(OUTCOME_REP)); // Romney / Rep slot
    r12.insert("presvote2012_x".into(), "-2".into());
    assert_eq!(actual_outcome(&r12, &s12), None);

    let s16 = anes_2016();
    let mut r16 = rec(&[("V162062x", "1")]);
    assert_eq!(actual_outcome(&r16, &s16), Some(OUTCOME_DEM)); // Clinton / Dem slot
    r16.insert("V162062x".into(), "2".into());
    assert_eq!(actual_outcome(&r16, &s16), Some(OUTCOME_REP));
    // third-party code (3) is not binarized -> None.
    r16.insert("V162062x".into(), "3".into());
    assert_eq!(actual_outcome(&r16, &s16), None);
}

// --- ported: recode_2020_row ----------------------------------------------

#[test]
fn recode_2020_row() {
    let s = anes_2020();
    let r = rec(&[
        ("V201549x", "1"),  // race=white
        ("V201600", "2"),   // gender=woman
        ("V201507x", "45"), // age=40-49
        ("V201200", "4"),   // ideology=moderate
        ("V201231x", "1"),  // party=strong democrat
        ("V202406", "1"),   // interest=very
        ("V201452", "1"),   // church=attend
        ("V202022", "1"),   // discuss=yes
    ]);
    let row = recode_row(&r, &s);
    assert!(row.is_complete(&s));
    assert_eq!(row.attrs["race"], "white");
    assert_eq!(row.attrs["age"], "40-49");
    assert_eq!(row.attrs["party_id"], "a strong democrat");
}

#[test]
fn demo_label_missing_and_mapped() {
    let s = anes_2020();
    let r = rec(&[("V201549x", "2"), ("V201600", "-9")]);
    assert_eq!(demo_label(&r, &s, "race"), Some("black".to_string()));
    // negative code (missing) -> None
    assert_eq!(demo_label(&r, &s, "gender"), None);
    // variable absent from schema -> None
    assert_eq!(demo_label(&r, &s, "no_such_var"), None);
}

// --- CategoryDist::from_counts (ported distribution test) ------------------

#[test]
fn category_dist_normalizes() {
    let mut c = HashMap::new();
    c.insert("white".to_string(), 3u64);
    c.insert("black".to_string(), 1u64);
    let d = CategoryDist::from_counts(&c);
    // sorted: black, white
    assert_eq!(d.labels, vec!["black", "white"]);
    assert!((d.probs[0] - 0.25).abs() < 1e-12);
    assert!((d.probs[1] - 0.75).abs() < 1e-12);
    assert!((d.probs.iter().sum::<f64>() - 1.0).abs() < 1e-12);
}

// --- estimate_distributions on synthetic records with known frequencies ----

#[test]
fn estimate_distributions_known_frequencies() {
    let s: SurveySchema = anes_2020();
    // 4 records: race white,white,black + one with missing race.
    // gender woman x3 (+1 missing). vote: 2 Biden, 1 Trump, 1 missing.
    let records = vec![
        rec(&[("V201549x", "1"), ("V201600", "2"), ("V202110x", "1")]),
        rec(&[("V201549x", "1"), ("V201600", "2"), ("V202110x", "1")]),
        rec(&[("V201549x", "2"), ("V201600", "2"), ("V202110x", "2")]),
        rec(&[("V201549x", "-9"), ("V201600", "-9"), ("V202110x", "-9")]),
    ];
    let d = estimate_distributions(&records, &s);

    // race normalized over its 3 non-missing rows: black 1/3, white 2/3.
    let race = d.demo("race").unwrap();
    assert_eq!(race.labels, vec!["black", "white"]);
    assert!((race.prob_for("black") - 1.0 / 3.0).abs() < 1e-12);
    assert!((race.prob_for("white") - 2.0 / 3.0).abs() < 1e-12);

    // gender over its 3 non-missing rows: woman 1.0.
    let gender = d.demo("gender").unwrap();
    assert!((gender.prob_for("woman") - 1.0).abs() < 1e-12);

    // outcome: Biden 2, Trump 1, missing excluded.
    assert_eq!(d.outcome.total(), 3);
    assert_eq!(d.outcome.count_of(OUTCOME_DEM), 2);
    assert_eq!(d.outcome.count_of(OUTCOME_REP), 1);
    assert!((d.outcome.rate_of(OUTCOME_DEM) - 2.0 / 3.0).abs() < 1e-12);
}

/// Small helper used by the test above to look up a probability by label.
trait ProbFor {
    fn prob_for(&self, label: &str) -> f64;
}
impl ProbFor for CategoryDist {
    fn prob_for(&self, label: &str) -> f64 {
        self.labels
            .iter()
            .position(|l| l == label)
            .map(|i| self.probs[i])
            .unwrap_or(0.0)
    }
}
