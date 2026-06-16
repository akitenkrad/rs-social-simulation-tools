//! Byte-parity tests for the pipe-delimited raw -> CSV converter.
#![cfg(feature = "acquire")]

use std::fs;

use socsim_datasets::acquire::raw_to_csv;

/// Header + 3 data rows, pipe-delimited, with padding spaces, and one field
/// value containing a comma (to exercise CSV quoting).
const RAW: &str = "id | name | note\n1 | alice | hello\n2 | bob | a, b, c\n3 | carol | ok\n";

/// Expected UTF-8 CSV: padding trimmed, the comma-bearing field quoted, CRLF
/// line terminators, no trailing quotes on plain fields.
const EXPECTED: &str = "id,name,note\r\n1,alice,hello\r\n2,bob,\"a, b, c\"\r\n3,carol,ok\r\n";

#[test]
fn converts_with_byte_parity() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw.txt");
    let output = dir.path().join("out.csv");
    fs::write(&input, RAW).unwrap();

    let report = raw_to_csv(&input, &output, b'|', true, None).unwrap();
    assert_eq!(report.columns, 3);
    assert_eq!(report.data_rows, 3);

    let bytes = fs::read(&output).unwrap();
    assert_eq!(bytes, EXPECTED.as_bytes());
}

#[test]
fn expect_rows_match_ok() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw.txt");
    let output = dir.path().join("out.csv");
    fs::write(&input, RAW).unwrap();

    let report = raw_to_csv(&input, &output, b'|', true, Some(3)).unwrap();
    assert_eq!(report.data_rows, 3);
}

#[test]
fn errors_on_wrong_field_count() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw.txt");
    let output = dir.path().join("out.csv");
    // Row 3 (file line 3) has only 2 fields instead of 3.
    fs::write(&input, "id | name | note\n1 | alice | hi\n2 | bob\n").unwrap();

    let err = raw_to_csv(&input, &output, b'|', true, None).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("line 3"), "message was: {msg}");
    assert!(msg.contains("2 fields"), "message was: {msg}");
}

#[test]
fn errors_on_expect_rows_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw.txt");
    let output = dir.path().join("out.csv");
    fs::write(&input, RAW).unwrap();

    let err = raw_to_csv(&input, &output, b'|', true, Some(99)).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("expected 99"), "message was: {msg}");
}
