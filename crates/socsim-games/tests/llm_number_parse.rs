use serde_json::json;
use socsim_games::llm::{NumberParser, OutOfRange, ParseOutcome, ViolationKind};
use socsim_llm::parse_integer_value;

fn accepted(outcome: ParseOutcome<f64>) -> (f64, socsim_games::llm::ParseFlags) {
    match outcome {
        ParseOutcome::Ok { action, flags } => (action, flags),
        ParseOutcome::Violation(violation) => panic!("unexpected violation: {violation:?}"),
    }
}

fn violation_kind(outcome: ParseOutcome<f64>) -> ViolationKind {
    match outcome {
        ParseOutcome::Ok { action, .. } => panic!("unexpected action: {action}"),
        ParseOutcome::Violation(violation) => violation.kind,
    }
}

#[test]
fn number_parser_accepts_signed_decimal_exponent_and_trimmed_string_lexemes() {
    let parser = NumberParser::new("amount");
    for (raw, expected, was_string) in [
        (r#"{"amount":-1}"#, -1.0, false),
        (r#"{"amount":3.14}"#, "3.14".parse::<f64>().unwrap(), false),
        (r#"{"amount":1e-3}"#, 1e-3, false),
        (r#"{"amount":" 2 "}"#, 2.0, true),
    ] {
        let (number, flags) = accepted(parser.parse_number(raw));
        assert_eq!(number, expected, "raw={raw}");
        assert_eq!(flags.value_was_string, was_string, "raw={raw}");
    }
}

#[test]
fn number_parser_rejects_non_finite_missing_and_wrong_type_values() {
    let parser = NumberParser::new("amount");
    for raw in [
        r#"{"amount":"inf"}"#,
        r#"{"amount":"-inf"}"#,
        r#"{"amount":"NaN"}"#,
        r#"{"amount":""}"#,
        r#"{"other":2}"#,
        r#"{"amount":null}"#,
        r#"{"amount":[]}"#,
        "not json",
    ] {
        assert_eq!(
            violation_kind(parser.parse_number(raw)),
            ViolationKind::Format,
            "raw={raw}"
        );
    }
}

#[test]
fn bounds_are_inclusive_and_violation_is_the_default_out_of_range_policy() {
    let parser = NumberParser::new("amount").bounds(-1.0, 3.0);
    assert_eq!(accepted(parser.parse_number(r#"{"amount":-1}"#)).0, -1.0);
    assert_eq!(accepted(parser.parse_number(r#"{"amount":3}"#)).0, 3.0);
    assert_eq!(
        violation_kind(parser.parse_number(r#"{"amount":-1.0001}"#)),
        ViolationKind::Range
    );
    assert_eq!(
        violation_kind(parser.parse_number(r#"{"amount":3.0001}"#)),
        ViolationKind::Range
    );
}

#[test]
fn clamp_moves_both_sides_to_the_bound_and_sets_only_the_clamped_flag() {
    let parser = NumberParser::new("amount")
        .bounds(-1.0, 3.0)
        .out_of_range(OutOfRange::Clamp);

    let (low, low_flags) = accepted(parser.parse_number(r#"{"amount":-2}"#));
    assert_eq!(low, -1.0);
    assert!(low_flags.clamped);
    assert!(!low_flags.value_was_string);

    let (high, high_flags) = accepted(parser.parse_number(r#"{"amount":"4"}"#));
    assert_eq!(high, 3.0);
    assert!(high_flags.clamped);
    assert!(high_flags.value_was_string);
}

#[test]
fn number_parser_and_unsigned_integer_parser_have_separate_lexical_contracts() {
    let parser = NumberParser::new("amount");
    assert_eq!(accepted(parser.parse_number(r#"{"amount":"3.0"}"#)).0, 3.0);
    assert_eq!(parse_integer_value(Some(&json!("3.0"))), None);
    assert_eq!(parse_integer_value(Some(&json!(-1))), None);
    assert_eq!(parse_integer_value(Some(&json!(" 003 "))), Some((3, true)));
}
