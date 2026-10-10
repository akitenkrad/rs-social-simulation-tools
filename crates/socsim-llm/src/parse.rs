//! Reusable free-text → discrete-choice extraction.
//!
//! LLM answers to a closed-vocabulary question ("vote DEMOCRAT or REPUBLICAN",
//! "reply VOICE or SILENCE") arrive as prose with leading markdown, punctuation,
//! or a full explanatory sentence.  [`extract_first_choice`] maps such free text
//! back to one of a known set of labels by scanning for synonyms on **word
//! boundaries**, picking the label whose synonym appears **first** in the text.
//!
//! This is a pure, network-free helper (always compiled, no feature gate) and is
//! independent of any concrete model or backend.

use serde_json::{Map, Value};
use thiserror::Error;

/// A parsed JSON object together with whether it came from one Markdown fence.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonObjectPayload {
    /// The parsed JSON object.
    pub object: Map<String, Value>,
    /// `true` when a recognized outer Markdown fence was removed.
    pub fenced: bool,
}

/// Errors produced while parsing an LLM response as a JSON object.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum JsonPayloadError {
    /// The selected payload is not valid JSON.
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    /// The selected payload is valid JSON but is not an object.
    #[error("the response must be a JSON object")]
    NotAnObject,
}

/// Extract the payload from exactly one supported Markdown code fence.
///
/// Leading and trailing whitespace around the response is ignored. The
/// response must then contain at least three lines: an opening line equal to
/// ````` ``` ````` or case-insensitively equal to ````` ```json `````, a
/// closing line equal to ````` ``` `````, and at least one line between them.
/// Whitespace surrounding either fence line is ignored. The inner lines are
/// joined with `\n` and returned unchanged otherwise.
///
/// Returns `None` for prose around the fence, an unclosed fence, unsupported
/// language tags, or any inner line whose trimmed text starts with
/// ````` ``` ````` (which rejects nested or multiple fenced blocks).
pub fn single_fenced_payload(raw: &str) -> Option<String> {
    let lines: Vec<_> = raw.trim().lines().collect();
    if lines.len() < 3 {
        return None;
    }
    let opening = lines.first()?.trim();
    if opening != "```" && !opening.eq_ignore_ascii_case("```json") {
        return None;
    }
    if lines.last()?.trim() != "```" {
        return None;
    }
    let inner = &lines[1..lines.len() - 1];
    if inner.iter().any(|line| line.trim().starts_with("```")) {
        return None;
    }
    Some(inner.join("\n"))
}

/// Return whether `raw` consists of exactly one supported Markdown fence.
///
/// This applies the same acceptance and rejection rules as
/// [`single_fenced_payload`].
pub fn is_single_fenced(raw: &str) -> bool {
    single_fenced_payload(raw).is_some()
}

/// Select the JSON text from a raw model response.
///
/// A response accepted by [`single_fenced_payload`] yields its inner text and
/// `true`. Every other response yields `raw.trim()` and `false`; malformed or
/// unsupported fences are deliberately not stripped.
pub fn json_payload_text(raw: &str) -> (String, bool) {
    match single_fenced_payload(raw) {
        Some(payload) => (payload, true),
        None => (raw.trim().to_string(), false),
    }
}

/// Parse a raw model response as a JSON object.
///
/// The response is normalized by [`json_payload_text`], parsed as JSON, and
/// then required to be an object. Invalid JSON returns
/// [`JsonPayloadError::InvalidJson`]; valid arrays, scalars, and `null` return
/// [`JsonPayloadError::NotAnObject`].
pub fn parse_json_object(raw: &str) -> Result<JsonObjectPayload, JsonPayloadError> {
    let (payload, fenced) = json_payload_text(raw);
    let value: Value = serde_json::from_str(&payload)
        .map_err(|error| JsonPayloadError::InvalidJson(error.to_string()))?;
    let Value::Object(object) = value else {
        return Err(JsonPayloadError::NotAnObject);
    };
    Ok(JsonObjectPayload { object, fenced })
}

/// Parse a JSON value as an unsigned integer and report whether it was a string.
///
/// Non-negative JSON integer values representable as `u64` are accepted as
/// `(value, false)`. Strings are trimmed and accepted as `(value, true)` only
/// when the result is non-empty, contains ASCII digits exclusively, and is
/// representable as `u64`. Leading zeroes and surrounding whitespace are
/// accepted.
///
/// Missing values, negative numbers, fractions, `null`, signed strings,
/// decimal strings such as `"3.0"`, non-ASCII digits, and values overflowing
/// `u64` return `None`. Narrower domain or game ranges are the caller's
/// responsibility.
pub fn parse_integer_value(value: Option<&Value>) -> Option<(u64, bool)> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return Some((number, false));
    }
    let text = value.as_str()?;
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    trimmed.parse::<u64>().ok().map(|number| (number, true))
}

/// Return whether a JSON object's named field was encoded as a string.
///
/// A supported outer fence is stripped using [`json_payload_text`]. Invalid
/// JSON, non-object JSON, and missing fields return `false`. The string's
/// contents need not be a valid integer or have any other particular form.
pub fn json_field_was_string(raw: &str, field: &str) -> bool {
    let (payload, _) = json_payload_text(raw);
    serde_json::from_str::<Value>(&payload)
        .ok()
        .and_then(|value| value.as_object()?.get(field).cloned())
        .is_some_and(|value| value.is_string())
}

/// Extract the first discrete choice mentioned in `text`.
///
/// `vocab` is a label → synonyms table: each entry is `(label, &[synonyms])`,
/// where the returned value (on a match) is the borrowed `label`.  A label
/// matches at the position where any of its synonyms occurs, and the label whose
/// synonym appears **earliest** in the (normalized) text wins.  Ties at the same
/// position are broken by **longest synonym** (the more specific match), which
/// keeps results stable regardless of label order in `vocab`.
///
/// Matching is:
/// - **case-insensitive** (text and synonyms are lowercased),
/// - **word-boundary anchored** — a synonym matches only when both its sides are
///   at a non-alphanumeric boundary, so `"democrat"` does **not** match inside
///   `"democratically"`, and
/// - tolerant of **leading markdown / punctuation** (`* # > -`, whitespace) and
///   of a full-sentence answer, because boundaries are detected anywhere in the
///   text rather than only at the start.
///
/// Returns `None` when no synonym of any label occurs on a word boundary.
/// Empty synonyms (or empty `text`) never match.
pub fn extract_first_choice<'a>(text: &str, vocab: &[(&'a str, &[&str])]) -> Option<&'a str> {
    let hay = text.to_lowercase();
    let bytes = hay.as_bytes();

    let mut best: Option<(usize, usize, &'a str)> = None; // (position, synonym_len, label)

    for &(label, synonyms) in vocab {
        for syn in synonyms {
            if syn.is_empty() {
                continue;
            }
            let needle = syn.to_lowercase();
            if let Some(pos) = find_on_word_boundary(&hay, bytes, &needle) {
                let len = needle.len();
                let better = match best {
                    None => true,
                    // Earlier position wins; on a tie the longer synonym wins.
                    Some((bp, bl, _)) => pos < bp || (pos == bp && len > bl),
                };
                if better {
                    best = Some((pos, len, label));
                }
            }
        }
    }

    best.map(|(_, _, label)| label)
}

/// First byte offset at which `needle` occurs in `hay` flanked by word
/// boundaries on both sides (a boundary is the string edge or a non-alphanumeric
/// byte), or `None`.
fn find_on_word_boundary(hay: &str, bytes: &[u8], needle: &str) -> Option<usize> {
    let nlen = needle.len();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(needle) {
        let start = from + rel;
        let end = start + nlen;
        let left_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let right_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if left_ok && right_ok {
            return Some(start);
        }
        from = start + 1;
    }
    None
}

/// Whether `b` is part of a "word" for boundary purposes (ASCII alphanumeric).
///
/// Non-ASCII (multi-byte UTF-8) bytes are treated as non-word, which is
/// acceptable for the ASCII-label vocabularies this helper targets.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vocab() -> Vec<(&'static str, &'static [&'static str])> {
        vec![
            ("DEMOCRAT", &["democrat", "democratic", "dem"][..]),
            ("REPUBLICAN", &["republican", "gop", "rep"][..]),
        ]
    }

    #[test]
    fn leading_punctuation_and_markdown() {
        let v = vocab();
        assert_eq!(extract_first_choice("* DEMOCRAT", &v), Some("DEMOCRAT"));
        assert_eq!(
            extract_first_choice("> - Republican", &v),
            Some("REPUBLICAN")
        );
        assert_eq!(extract_first_choice("  #democrat", &v), Some("DEMOCRAT"));
    }

    #[test]
    fn full_sentence_answer() {
        let v = vocab();
        assert_eq!(
            extract_first_choice("I would most likely vote Republican this year.", &v),
            Some("REPUBLICAN")
        );
    }

    #[test]
    fn two_labels_first_wins() {
        let v = vocab();
        // "democrat" appears before "republican" → DEMOCRAT.
        assert_eq!(
            extract_first_choice("Between democrat and republican, hard to say.", &v),
            Some("DEMOCRAT")
        );
        // Reversed order in the text.
        assert_eq!(
            extract_first_choice("republican, then democrat", &v),
            Some("REPUBLICAN")
        );
    }

    #[test]
    fn word_boundary_avoids_false_match() {
        let v = vocab();
        // "democratically" must NOT match the "democrat" synonym (boundary).
        assert_eq!(
            extract_first_choice("They behaved democratically last term.", &v),
            None
        );
        // But the standalone word does match.
        assert_eq!(
            extract_first_choice("They are a democrat.", &v),
            Some("DEMOCRAT")
        );
    }

    #[test]
    fn no_match_returns_none() {
        let v = vocab();
        assert_eq!(extract_first_choice("undecided / independent", &v), None);
        assert_eq!(extract_first_choice("", &v), None);
    }

    #[test]
    fn longest_synonym_tie_break_at_same_position() {
        // Two labels whose synonyms start at the same offset; the longer, more
        // specific synonym should win.
        let v: Vec<(&str, &[&str])> = vec![("SHORT", &["voice"][..]), ("LONG", &["voiceless"][..])];
        assert_eq!(extract_first_choice("voiceless agents", &v), Some("LONG"));
    }

    #[test]
    fn case_insensitive() {
        let v = vocab();
        assert_eq!(
            extract_first_choice("GOP all the way", &v),
            Some("REPUBLICAN")
        );
    }

    #[test]
    fn dollar_protocol_parser_accepts_valid_json_and_exposes_missing_fields() {
        let parsed = parse_json_object(r#"{"action":"bid","amount":12,"reason":"test"}"#).unwrap();
        assert!(!parsed.fenced);
        assert_eq!(
            parse_integer_value(parsed.object.get("amount")),
            Some((12, false))
        );
        assert_eq!(
            parsed.object.get("reason").and_then(|value| value.as_str()),
            Some("test")
        );

        assert!(matches!(
            parse_json_object("not json"),
            Err(JsonPayloadError::InvalidJson(_))
        ));
        let missing_amount = parse_json_object(r#"{"action":"bid","reason":"missing"}"#).unwrap();
        assert_eq!(
            parse_integer_value(missing_amount.object.get("amount")),
            None
        );
        let missing_reason = parse_json_object(r#"{"action":"drop"}"#).unwrap();
        assert!(!missing_reason.object.contains_key("reason"));
    }

    #[test]
    fn dollar_protocol_parser_accepts_trimmed_decimal_strings_and_leaves_range_to_caller() {
        for action in ["bid", "invest"] {
            let raw = format!(r#"{{"action":"{action}","amount":" 003 ","reason":"go"}}"#);
            let parsed = parse_json_object(&raw).unwrap();
            assert_eq!(
                parse_integer_value(parsed.object.get("amount")),
                Some((3, true))
            );
            assert!(json_field_was_string(&raw, "amount"));
        }

        let parsed = parse_json_object(r#"{"action":"bid","amount":"21","reason":"go"}"#).unwrap();
        let (amount, was_string) = parse_integer_value(parsed.object.get("amount")).unwrap();
        assert_eq!((amount, was_string), (21, true));
        assert!(!(11..=20).contains(&amount));
    }

    #[test]
    fn dollar_protocol_parser_rejects_non_decimal_strings_and_floats() {
        for value in [
            json!("3.0"),
            json!("$3"),
            json!("three"),
            json!(" 3 dollars"),
            json!(""),
            json!("-3"),
            json!("+3"),
        ] {
            assert_eq!(parse_integer_value(Some(&value)), None, "accepted {value}");
        }
        assert_eq!(parse_integer_value(Some(&json!(3.0))), None);

        let above_u32 = json!(u64::from(u32::MAX) + 1);
        let (amount, was_string) = parse_integer_value(Some(&above_u32)).unwrap();
        assert_eq!(amount, u64::from(u32::MAX) + 1);
        assert!(!was_string);
        assert!(u32::try_from(amount).is_err());

        let above_u32_string = json!((u64::from(u32::MAX) + 1).to_string());
        let (amount, was_string) = parse_integer_value(Some(&above_u32_string)).unwrap();
        assert_eq!(amount, u64::from(u32::MAX) + 1);
        assert!(was_string);
        assert!(u32::try_from(amount).is_err());
    }

    #[test]
    fn dollar_protocol_quiz_accepts_decimal_string_and_records_type() {
        let raw = r#"{"first_bid":" 052 ","reason":"calculated"}"#;
        let parsed = parse_json_object(raw).unwrap();
        assert_eq!(
            parse_integer_value(parsed.object.get("first_bid")),
            Some((52, true))
        );
        assert!(json_field_was_string(raw, "first_bid"));

        let invalid = parse_json_object(r#"{"first_bid":"3.0","reason":"calculated"}"#).unwrap();
        assert_eq!(parse_integer_value(invalid.object.get("first_bid")), None);
    }

    #[test]
    fn dollar_protocol_parser_accepts_exactly_one_markdown_fence() {
        for opening in ["```", "```json", "```JSON"] {
            let raw =
                format!("{opening}\n{{\"action\":\"drop\",\"amount\":0,\"reason\":\"done\"}}\n```");
            let parsed = parse_json_object(&raw).unwrap();
            assert!(parsed.fenced);
            assert!(is_single_fenced(&raw));
            assert_eq!(
                single_fenced_payload(&raw).as_deref(),
                Some(r#"{"action":"drop","amount":0,"reason":"done"}"#)
            );
        }
    }

    #[test]
    fn dollar_protocol_parser_accepts_crlf_and_surrounding_whitespace() {
        let raw = " \r\n```JSON\r\n{\"action\":\"drop\",\"reason\":\"done\"}\r\n```\r\n ";
        let parsed = parse_json_object(raw).unwrap();
        assert!(parsed.fenced);
        assert_eq!(
            parsed.object.get("action").and_then(|value| value.as_str()),
            Some("drop")
        );
    }

    #[test]
    fn dollar_protocol_framing_vocabulary_is_exposed_without_game_semantics() {
        for action in ["bid", "drop", "invest", "withdraw"] {
            let raw = format!(r#"{{"action":"{action}","amount":12,"reason":"go"}}"#);
            let parsed = parse_json_object(&raw).unwrap();
            assert_eq!(
                parsed.object.get("action").and_then(|value| value.as_str()),
                Some(action)
            );
        }
    }

    #[test]
    fn dollar_protocol_calculator_and_fenced_json_fields_are_preserved() {
        for action in ["drop", "withdraw"] {
            let raw = format!("```json\n{{\"action\":\"{action}\",\"reason\":\"done\"}}\n```");
            let parsed = parse_json_object(&raw).unwrap();
            assert!(parsed.fenced);
            assert_eq!(
                parsed.object.get("action").and_then(|value| value.as_str()),
                Some(action)
            );
        }

        let parsed = parse_json_object(r#"{"action":"calc","expr":"17 + 5"}"#).unwrap();
        assert_eq!(
            parsed.object.get("expr").and_then(|value| value.as_str()),
            Some("17 + 5")
        );
    }

    #[test]
    fn dollar_protocol_parser_rejects_prose_multiple_and_unclosed_fences() {
        let inputs = [
            "Here is the result:\n```json\n{\"action\":\"drop\",\"reason\":\"done\"}\n```",
            "```json\n{\"action\":\"drop\",\"reason\":\"done\"}\n```\n```json\n{\"action\":\"drop\",\"reason\":\"again\"}\n```",
            "```json\n{\"action\":\"drop\",\"reason\":\"done\"}",
        ];
        for raw in inputs {
            assert!(!is_single_fenced(raw));
            assert!(matches!(
                parse_json_object(raw),
                Err(JsonPayloadError::InvalidJson(_))
            ));
        }
    }

    #[test]
    fn dollar_protocol_contradictory_extra_fields_remain_visible() {
        let parsed = parse_json_object(r#"{"action":"drop","amount":12,"reason":"done"}"#).unwrap();
        assert_eq!(
            parsed.object.get("action").and_then(|value| value.as_str()),
            Some("drop")
        );
        assert!(parsed.object.contains_key("amount"));
    }

    #[test]
    fn dollar_protocol_retry_inputs_keep_format_and_range_distinct() {
        assert!(matches!(
            parse_json_object("bad"),
            Err(JsonPayloadError::InvalidJson(_))
        ));
        let parsed = parse_json_object(r#"{"action":"bid","amount":10}"#).unwrap();
        let (amount, _) = parse_integer_value(parsed.object.get("amount")).unwrap();
        assert!(!(11..=20).contains(&amount));
    }

    #[test]
    fn dollar_protocol_calculator_expression_text_is_parser_neutral() {
        for expression in ["(17 + 5) * 3", "-7 / 3", "-7 % 3", "2 ** 8"] {
            let raw = format!(r#"{{"action":"calc","expr":"{expression}"}}"#);
            let parsed = parse_json_object(&raw).unwrap();
            assert_eq!(
                parsed.object.get("expr").and_then(|value| value.as_str()),
                Some(expression)
            );
        }
        let long_expression = "1".repeat(201);
        let raw = format!(r#"{{"action":"calc","expr":"{long_expression}"}}"#);
        assert_eq!(
            parse_json_object(&raw)
                .unwrap()
                .object
                .get("expr")
                .and_then(|value| value.as_str()),
            Some(long_expression.as_str())
        );
    }

    #[test]
    fn dollar_protocol_calculator_request_count_is_not_a_json_concern() {
        for expression in ["1 / 0", "1 + 1", "2 + 2", "3 + 3"] {
            let raw = format!(r#"{{"action":"calc","expr":"{expression}"}}"#);
            assert!(parse_json_object(&raw).is_ok());
        }
    }

    #[test]
    fn dollar_quiz_plain_fenced_string_and_invalid_json_cases() {
        let plain = parse_json_object(r#"{"first_bid":52,"reason":"calculated"}"#).unwrap();
        assert!(!plain.fenced);
        assert_eq!(
            parse_integer_value(plain.object.get("first_bid")),
            Some((52, false))
        );

        let fenced =
            parse_json_object("```JSON\n{\"first_bid\":50,\"reason\":\"tie-aware\"}\n```").unwrap();
        assert!(fenced.fenced);
        assert_eq!(
            parse_integer_value(fenced.object.get("first_bid")),
            Some((50, false))
        );

        let string_bid = parse_json_object(r#"{"first_bid":"50","reason":"tie-aware"}"#).unwrap();
        assert_eq!(
            parse_integer_value(string_bid.object.get("first_bid")),
            Some((50, true))
        );
        assert!(matches!(
            parse_json_object("not json"),
            Err(JsonPayloadError::InvalidJson(_))
        ));
    }

    #[test]
    fn integer_value_boundaries_and_rejections() {
        let cases = [
            (json!(0), Some((0, false))),
            (json!(u32::MAX), Some((u64::from(u32::MAX), false))),
            (
                json!(u64::from(u32::MAX) + 1),
                Some((u64::from(u32::MAX) + 1, false)),
            ),
            (json!("18446744073709551616"), None),
            (json!("-1"), None),
            (json!(" 7 "), Some((7, true))),
            (json!("07"), Some((7, true))),
            (json!("3.0"), None),
            (json!(3.5), None),
            (json!(-1), None),
            (json!(null), None),
        ];
        for (value, expected) in cases {
            assert_eq!(
                parse_integer_value(Some(&value)),
                expected,
                "value: {value}"
            );
        }
        assert_eq!(parse_integer_value(None), None);
    }

    #[test]
    fn payload_text_and_object_errors_are_reported() {
        assert_eq!(
            json_payload_text("  {\"a\":1}  "),
            ("{\"a\":1}".into(), false)
        );
        assert!(matches!(
            parse_json_object("[]"),
            Err(JsonPayloadError::NotAnObject)
        ));
        assert!(!json_field_was_string(r#"{"field":1}"#, "field"));
        assert!(!json_field_was_string(r#"{"field":"x"}"#, "missing"));
        assert!(!json_field_was_string("not json", "field"));
    }
}
