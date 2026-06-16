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
}
