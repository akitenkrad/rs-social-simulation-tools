//! Ported sun2024 `rss/reproduce.rs` unit tests (the parity guarantee) +
//! CSV-writer round-trip tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use socsim_reproduce::{
    build_rows, compare_anchor, find_latest, write_paper_anchors, write_reproduce_summary, Anchor,
    AnchorStatus,
};

/// sun2024 Study A Table 1 anchors, declared *in the test* (the crate ships no
/// anchors). These are the exact values from sun2024's `PAPER_ANCHORS`.
static ANCHORS: &[Anchor] = &[
    Anchor {
        study: "A",
        table_or_fig: "Table 1",
        condition: "overall",
        metric: "anes_biden_rate",
        paper_value: 0.5888,
        tolerance: 0.02,
        upper_bound: false,
        note: "ANES 2020 actual Democratic (Biden) vote share",
    },
    Anchor {
        study: "A",
        table_or_fig: "Table 1",
        condition: "overall",
        metric: "anes_trump_rate",
        paper_value: 0.4118,
        tolerance: 0.02,
        upper_bound: false,
        note: "ANES 2020 actual Republican (Trump) vote share",
    },
    Anchor {
        study: "A",
        table_or_fig: "Table 1",
        condition: "overall",
        metric: "rss_biden_rate",
        paper_value: 0.5743,
        tolerance: 0.02,
        upper_bound: false,
        note: "RSS 10-iteration mean Biden rate",
    },
    Anchor {
        study: "A",
        table_or_fig: "Table 1",
        condition: "overall",
        metric: "mean_kl",
        paper_value: 0.0004,
        tolerance: 0.001,
        upper_bound: true,
        note: "RSS 10-iteration mean KL; gate upper bound 0.001",
    },
];

/// Observation lookup mirroring sun2024's `StudyAObserved::value_for`.
fn observe(mean_biden: f64) -> impl Fn(&Anchor) -> Option<f64> {
    move |a: &Anchor| match a.metric {
        "anes_biden_rate" => Some(0.5888),
        "anes_trump_rate" => Some(1.0 - 0.5888),
        "rss_biden_rate" => Some(mean_biden),
        "mean_kl" => Some(0.0004),
        _ => None,
    }
}

// --- ported: no_data_when_observed_missing --------------------------------

#[test]
fn no_data_when_observed_missing() {
    for a in ANCHORS {
        assert_eq!(compare_anchor(a, None), AnchorStatus::NoData);
    }
}

// --- ported: paper_values_pass_against_themselves -------------------------

#[test]
fn paper_values_pass_against_themselves() {
    let rows = build_rows(ANCHORS, observe(0.5743));
    assert_eq!(rows.len(), 4);
    for r in &rows {
        assert_eq!(r.status, "PASS", "metric {}", r.metric);
    }
}

// --- ported: biden_outside_tolerance_is_off -------------------------------

#[test]
fn biden_outside_tolerance_is_off() {
    // RSS Biden 0.50 vs 0.5743 -> -7.43pt, outside ±2pt.
    let rows = build_rows(ANCHORS, observe(0.50));
    let biden = rows.iter().find(|r| r.metric == "rss_biden_rate").unwrap();
    assert_eq!(biden.status, "off");
}

// --- ported: kl_upper_bound_pass_and_off ----------------------------------

#[test]
fn kl_upper_bound_pass_and_off() {
    let kl = ANCHORS.iter().find(|a| a.metric == "mean_kl").unwrap();
    assert!(kl.upper_bound);
    // 0.0004 < 0.001 -> PASS.
    assert_eq!(compare_anchor(kl, Some(0.0004)), AnchorStatus::Pass);
    // 0.01 >= 0.001 -> off.
    assert_eq!(compare_anchor(kl, Some(0.01)), AnchorStatus::Off);
    // exactly the bound (0.001) is off (strict less-than).
    assert_eq!(compare_anchor(kl, Some(0.001)), AnchorStatus::Off);
}

// --- ported: build_rows_marks_no_data_without_observed --------------------

#[test]
fn build_rows_marks_no_data_without_observed() {
    let rows = build_rows(ANCHORS, |_| None);
    assert_eq!(rows.len(), 4);
    for r in &rows {
        assert_eq!(r.status, "NO_DATA");
        // empty serialized column; accessor maps it back to None.
        assert_eq!(r.observed_value, "");
        assert!(r.observed_value().is_none());
    }
}

// --- centred tolerance boundary -------------------------------------------

#[test]
fn centred_tolerance_boundary() {
    // rss_biden_rate anchor: paper 0.5743, tolerance 0.02.
    // Just inside the band passes; just outside is off. (Exactly on the bound is
    // float-unstable, mirroring sun2024's gate boundary tests, so we probe the
    // safe inside/outside.)
    let a = &ANCHORS[2];
    assert_eq!(compare_anchor(a, Some(0.5743 + 0.0199)), AnchorStatus::Pass);
    assert_eq!(compare_anchor(a, Some(0.5743 + 0.0201)), AnchorStatus::Off);
}

// --- CSV writers round-trip -----------------------------------------------

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("socsim-reproduce-{tag}-{}-{n}", std::process::id()))
}

/// Deserialized with the numeric columns kept as **raw strings**, so the tests
/// can lock the exact `{:.6}` byte form sun2024 emits (not just the value).
#[derive(Debug, Deserialize)]
struct SummaryRow {
    study: String,
    table_or_fig: String,
    condition: String,
    metric: String,
    paper_value: String,
    observed_value: String,
    tolerance: String,
    status: String,
}

#[test]
fn reproduce_summary_csv_round_trips() {
    let dir = unique_dir("summary");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("reproduce_summary.csv");

    let rows = build_rows(ANCHORS, observe(0.5743));
    write_reproduce_summary(&rows, &path).unwrap();

    let mut rdr = csv::Reader::from_path(&path).unwrap();
    let headers: Vec<String> = rdr.headers().unwrap().iter().map(String::from).collect();
    assert_eq!(
        headers,
        vec![
            "study",
            "table_or_fig",
            "condition",
            "metric",
            "paper_value",
            "observed_value",
            "tolerance",
            "status",
        ]
    );
    let parsed: Vec<SummaryRow> = rdr.deserialize().map(|r| r.unwrap()).collect();
    assert_eq!(parsed.len(), 4);
    // every row PASS, observed values present.
    for r in &parsed {
        assert_eq!(r.status, "PASS");
        assert_eq!(r.study, "A");
        assert_eq!(r.table_or_fig, "Table 1");
        assert_eq!(r.condition, "overall");
        assert!(!r.observed_value.is_empty());
    }

    // Byte-parity: numeric columns are fixed 6-decimal strings exactly as
    // sun2024 emits them (e.g. mean_kl observed 0.0004 -> "0.000400", paper
    // value 0.0004 -> "0.000400", tolerance 0.001 -> "0.001000").
    let kl = parsed.iter().find(|r| r.metric == "mean_kl").unwrap();
    assert_eq!(kl.paper_value, "0.000400");
    assert_eq!(kl.observed_value, "0.000400");
    assert_eq!(kl.tolerance, "0.001000");
    let biden = parsed
        .iter()
        .find(|r| r.metric == "rss_biden_rate")
        .unwrap();
    assert_eq!(biden.paper_value, "0.574300");
    assert_eq!(biden.observed_value, "0.574300");
    assert_eq!(biden.tolerance, "0.020000");

    // The exact CSV bytes of the mean_kl data row (column order + {:.6}).
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("A,Table 1,overall,mean_kl,0.000400,0.000400,0.001000,PASS"),
        "unexpected mean_kl row bytes in:\n{text}"
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reproduce_summary_csv_no_data_leaves_observed_empty() {
    let dir = unique_dir("nodata");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("reproduce_summary.csv");

    let rows = build_rows(ANCHORS, |_| None);
    write_reproduce_summary(&rows, &path).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("NO_DATA"));
    // sun2024 renders a missing observation as the empty string between
    // paper_value and tolerance: e.g. ...,mean_kl,0.000400,,0.001000,NO_DATA.
    assert!(
        text.contains("A,Table 1,overall,mean_kl,0.000400,,0.001000,NO_DATA"),
        "missing observation must be an empty column, got:\n{text}"
    );
    let mut rdr = csv::Reader::from_path(&path).unwrap();
    let parsed: Vec<SummaryRow> = rdr.deserialize().map(|r| r.unwrap()).collect();
    for r in &parsed {
        assert_eq!(r.observed_value, "");
        assert_eq!(r.status, "NO_DATA");
    }

    std::fs::remove_dir_all(&dir).unwrap();
}

#[derive(Debug, Deserialize)]
struct AnchorCsvRow {
    study: String,
    metric: String,
    paper_value: String,
    tolerance: String,
    comparison: String,
    note: String,
}

#[test]
fn paper_anchors_csv_round_trips() {
    let dir = unique_dir("anchors");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("paper_anchors.csv");

    write_paper_anchors(ANCHORS, &path).unwrap();

    let mut rdr = csv::Reader::from_path(&path).unwrap();
    let headers: Vec<String> = rdr.headers().unwrap().iter().map(String::from).collect();
    assert_eq!(
        headers,
        vec![
            "study",
            "table_or_fig",
            "condition",
            "metric",
            "paper_value",
            "tolerance",
            "comparison",
            "note",
        ]
    );
    let parsed: Vec<AnchorCsvRow> = rdr.deserialize().map(|r| r.unwrap()).collect();
    assert_eq!(parsed.len(), 4);
    // mean_kl is the upper_bound anchor; others are tolerance.
    let kl = parsed.iter().find(|r| r.metric == "mean_kl").unwrap();
    assert_eq!(kl.comparison, "upper_bound");
    let biden = parsed
        .iter()
        .find(|r| r.metric == "rss_biden_rate")
        .unwrap();
    assert_eq!(biden.comparison, "tolerance");
    // Byte-parity: paper_value / tolerance are fixed 6-decimal strings.
    assert_eq!(biden.paper_value, "0.574300");
    assert_eq!(biden.tolerance, "0.020000");
    assert_eq!(kl.paper_value, "0.000400");
    assert_eq!(kl.tolerance, "0.001000");
    for r in &parsed {
        assert_eq!(r.study, "A");
        assert!(!r.note.is_empty());
    }

    std::fs::remove_dir_all(&dir).unwrap();
}

// --- find_latest ----------------------------------------------------------

#[test]
fn find_latest_picks_newest_matching_dir() {
    let root = unique_dir("findlatest");
    std::fs::create_dir_all(&root).unwrap();
    // Timestamp-shaped dirs; the predicate requires a gate_summary.csv sidecar.
    for ts in ["20240101_000000", "20240102_000000", "20240103_000000"] {
        let d = root.join(ts);
        std::fs::create_dir_all(&d).unwrap();
    }
    // Only the two older dirs have the sidecar; the newest (0103) does not, so
    // find_latest must skip it and return 0102.
    std::fs::write(root.join("20240101_000000/gate_summary.csv"), "x").unwrap();
    std::fs::write(root.join("20240102_000000/gate_summary.csv"), "x").unwrap();
    // A `latest` symlink-like dir must be ignored even if it matches.
    let latest = root.join("latest");
    std::fs::create_dir_all(&latest).unwrap();
    std::fs::write(latest.join("gate_summary.csv"), "x").unwrap();

    let found = find_latest(&root, |d| d.join("gate_summary.csv").exists()).unwrap();
    assert_eq!(found.unwrap().file_name().unwrap(), "20240102_000000");

    // Non-existent root -> Ok(None).
    let missing = find_latest(&root.join("nope"), |_| true).unwrap();
    assert!(missing.is_none());

    std::fs::remove_dir_all(&root).unwrap();
}
