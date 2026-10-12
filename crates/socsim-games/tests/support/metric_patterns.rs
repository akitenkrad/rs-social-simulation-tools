//! The grammar of the metric-name patterns `describe()` returns.
//!
//! The patterns are meant to be copied unchanged into the `[[metric_patterns]]`
//! of a runvault `runvault.toml`, so they follow runvault's rule, restated here
//! so the crate can check it without depending on runvault:
//!
//! - a pattern and a name are split on `.`, and the segment counts must agree;
//! - a segment written wholly as `{axis}` matches any one segment of the name;
//! - every other segment must equal the name's segment byte for byte.
//!
//! A brace in any other position is therefore a literal character. No column
//! name contains one, so a segment such as `a{k}` describes nothing, and
//! runvault refuses to record the columns it was meant to describe.

/// Whether `segment` is an axis: `{…}` with at least one character inside.
pub fn is_axis(segment: &str) -> bool {
    segment.len() > 2 && segment.starts_with('{') && segment.ends_with('}')
}

/// Whether `pattern` describes the column `name`.
pub fn matches(pattern: &str, name: &str) -> bool {
    let want: Vec<&str> = pattern.split('.').collect();
    let got: Vec<&str> = name.split('.').collect();
    want.len() == got.len() && want.iter().zip(&got).all(|(w, g)| is_axis(w) || w == g)
}

/// Whether every brace in `pattern` belongs to an axis segment.
pub fn is_well_formed(pattern: &str) -> bool {
    pattern.split('.').all(|segment| {
        if is_axis(segment) {
            !segment[1..segment.len() - 1].contains(['{', '}'])
        } else {
            !segment.is_empty() && !segment.contains(['{', '}'])
        }
    })
}

/// Asserts that every pattern is well formed, that every recorded name is
/// described by one of the patterns, and that every pattern describes at least
/// one recorded name.
pub fn assert_patterns_cover<'a>(
    patterns: &[(String, String)],
    names: impl IntoIterator<Item = &'a str>,
) {
    for (pattern, _) in patterns {
        assert!(is_well_formed(pattern), "malformed pattern {pattern:?}");
    }
    let names: Vec<&str> = names.into_iter().collect();
    assert!(!names.is_empty(), "no columns were recorded");
    for name in &names {
        assert!(
            patterns.iter().any(|(pattern, _)| matches(pattern, name)),
            "no pattern describes the column {name:?}"
        );
    }
    for (pattern, _) in patterns {
        assert!(
            names.iter().any(|name| matches(pattern, name)),
            "pattern {pattern:?} describes none of the recorded columns"
        );
    }
}
