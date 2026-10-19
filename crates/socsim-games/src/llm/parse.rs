use std::collections::BTreeMap;

use rand::Rng;
use serde::{Deserialize, Serialize};
use socsim_core::SimRng;
use socsim_llm::{
    extract_first_choice, is_single_fenced, json_field_was_string, parse_json_object,
};

use super::{Violation, ViolationKind};
use crate::game::continuous::check_bounds;
use crate::{ContinuousGame, FiniteGame, RuleError, StageGame};

/// Converts backend text into valid game actions and supplies fallback samples.
///
/// Parser authors implement this trait and pass an instance to [`LlmPolicy`](super::LlmPolicy).
/// The policy calls [`validate`](Self::validate) during initialization,
/// [`parse`](Self::parse) for each reply, and [`sample_uniform`](Self::sample_uniform)
/// only for a uniform-random fallback. Implementations must return violations,
/// rather than panic, for arbitrary backend text.
pub trait ActionParser<G: StageGame>: Send + Sync {
    /// Parses `raw` for one role, or reports a structured violation.
    ///
    /// `None` means one action must be valid for all roles represented by the
    /// decision; built-in parsers use role 0's common action space.
    fn parse(&self, raw: &str, game: &G, role: Option<usize>) -> ParseOutcome<G::Action>;

    /// Draws a valid action for the uniform-random fallback.
    ///
    /// The driver calls this only after successful [`validate`](Self::validate).
    fn sample_uniform(&self, game: &G, role: Option<usize>, rng: &mut SimRng) -> G::Action;

    /// Checks parser settings against the selected game before simulation.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError`] when labels, dimensions, or configured bounds are
    /// incompatible with the game. The default accepts every game.
    fn validate(&self, _game: &G) -> Result<(), RuleError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
/// Parser for finite-action labels in strict JSON or lenient text.
pub struct LabelParser {
    lenient: bool,
    require_reason: bool,
    synonyms: BTreeMap<String, Vec<String>>,
}

impl LabelParser {
    /// Creates a parser requiring an exact JSON string in the `action` field.
    pub fn strict() -> Self {
        Self::default()
    }

    /// Creates a parser that also finds unambiguous action labels in free text.
    pub fn lenient() -> Self {
        Self {
            lenient: true,
            ..Self::default()
        }
    }

    /// Configures whether a string-valued `reason` field is mandatory.
    pub fn require_reason(mut self, yes: bool) -> Self {
        self.require_reason = yes;
        self
    }

    /// Adds alternative spellings recognized for one canonical action label.
    ///
    /// Synonyms affect only lenient matching and replace any earlier synonyms
    /// registered for `label`.
    pub fn with_synonyms(mut self, label: &str, synonyms: &[&str]) -> Self {
        self.synonyms.insert(
            label.to_owned(),
            synonyms
                .iter()
                .map(|synonym| (*synonym).to_owned())
                .collect(),
        );
        self
    }

    fn violation(kind: ViolationKind, message: impl Into<String>) -> ParseOutcome<usize> {
        ParseOutcome::Violation(Violation {
            kind,
            message: message.into(),
        })
    }

    fn labels<G: FiniteGame>(game: &G, role: usize) -> Vec<String> {
        (0..game.n_actions(role))
            .map(|action| game.action_label(role, action).into_owned())
            .collect()
    }
}

impl<G: FiniteGame> ActionParser<G> for LabelParser {
    fn parse(&self, raw: &str, game: &G, role: Option<usize>) -> ParseOutcome<usize> {
        let role = role.unwrap_or(0);
        if role >= game.n_roles() {
            return Self::violation(ViolationKind::Range, format!("role {role} is out of range"));
        }
        let labels = Self::labels(game, role);
        let parsed = parse_json_object(raw);

        if let Ok(payload) = &parsed {
            if self.require_reason
                && payload
                    .object
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .is_none()
            {
                return Self::violation(
                    ViolationKind::Format,
                    "reason must be present and encoded as a string",
                );
            }
            let action_label = payload
                .object
                .get("action")
                .and_then(|value| value.as_str());
            if action_label.is_none() && !self.lenient {
                return Self::violation(
                    ViolationKind::Format,
                    "action must be present and encoded as a string",
                );
            }
            if let Some(action) = action_label
                .and_then(|action_label| labels.iter().position(|label| label == action_label))
            {
                return ParseOutcome::Ok {
                    action,
                    flags: ParseFlags {
                        fenced: payload.fenced,
                        value_was_string: json_field_was_string(raw, "action"),
                        ..ParseFlags::default()
                    },
                };
            }
        } else if self.require_reason || !self.lenient {
            return Self::violation(ViolationKind::Format, parsed.unwrap_err().to_string());
        }

        if !self.lenient {
            return Self::violation(
                ViolationKind::Label,
                "action does not exactly match an available label",
            );
        }

        let synonym_storage: Vec<Vec<&str>> = labels
            .iter()
            .map(|label| {
                std::iter::once(label.as_str())
                    .chain(
                        self.synonyms
                            .get(label)
                            .into_iter()
                            .flatten()
                            .map(String::as_str),
                    )
                    .collect()
            })
            .collect();
        let vocab: Vec<(&str, &[&str])> = labels
            .iter()
            .zip(&synonym_storage)
            .map(|(label, synonyms)| (label.as_str(), synonyms.as_slice()))
            .collect();
        let Some(label) = extract_first_choice(raw, &vocab) else {
            return Self::violation(
                ViolationKind::Label,
                "response does not contain an available action label",
            );
        };
        let action = labels
            .iter()
            .position(|candidate| candidate == label)
            .expect("extract_first_choice returns a label from the supplied vocabulary");
        ParseOutcome::Ok {
            action,
            flags: ParseFlags {
                fenced: is_single_fenced(raw),
                value_was_string: json_field_was_string(raw, "action"),
                lenient_match: true,
                ..ParseFlags::default()
            },
        }
    }

    fn sample_uniform(&self, game: &G, role: Option<usize>, rng: &mut SimRng) -> usize {
        let role = role.unwrap_or(0);
        rng.gen_range(0..game.n_actions(role))
    }

    fn validate(&self, game: &G) -> Result<(), RuleError> {
        if !self.lenient {
            return Ok(());
        }
        for role in 0..game.n_roles() {
            for action in 0..game.n_actions(role) {
                let label = game.action_label(role, action);
                if label.is_empty()
                    || !label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                {
                    return Err(RuleError::Unsupported(format!(
                        "lenient label matching requires [A-Za-z0-9_-]+ labels; got {label:?}"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
/// Parser for a finite JSON number used by one-dimensional continuous games.
pub struct NumberParser {
    field: String,
    bounds: Option<(f64, f64)>,
    out_of_range: OutOfRange,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
/// Handling of a finite numeric reply outside the permitted closed interval.
pub enum OutOfRange {
    /// Replace the value with the nearest interval endpoint and set a parse flag.
    Clamp,
    /// Reject the value as a range violation; this is the default.
    #[default]
    Violation,
}

impl NumberParser {
    /// Creates a parser for the named JSON field with game-defined bounds.
    pub fn new(field: &str) -> Self {
        Self {
            field: field.to_owned(),
            bounds: None,
            out_of_range: OutOfRange::Violation,
        }
    }

    /// Overrides the accepted closed interval.
    ///
    /// [`ActionParser::validate`] later requires this interval to be ordered
    /// and contained within every role's game bounds.
    pub fn bounds(mut self, lo: f64, hi: f64) -> Self {
        self.bounds = Some((lo, hi));
        self
    }

    /// Selects whether out-of-range numbers are clamped or rejected.
    pub fn out_of_range(mut self, policy: OutOfRange) -> Self {
        self.out_of_range = policy;
        self
    }

    /// Parses the configured field using this parser's optional bounds.
    pub fn parse_number(&self, raw: &str) -> ParseOutcome<f64> {
        self.parse_number_within(raw, self.bounds)
    }

    /// Parses the numeric field and checks it against `bounds` instead of the
    /// parser's own bounds.
    fn parse_number_within(&self, raw: &str, bounds: Option<(f64, f64)>) -> ParseOutcome<f64> {
        let payload = match parse_json_object(raw) {
            Ok(payload) => payload,
            Err(error) => return format_violation(error.to_string()),
        };
        let Some(value) = payload.object.get(&self.field) else {
            return format_violation(format!("missing numeric field {:?}", self.field));
        };
        let value_was_string = json_field_was_string(raw, &self.field);
        let lexical = if let Some(text) = value.as_str() {
            text
        } else if value.is_number() {
            // serde_json owns the numeric spelling, so keep it alive through parsing.
            return self.parse_lexical(
                &value.to_string(),
                payload.fenced,
                value_was_string,
                bounds,
            );
        } else {
            return format_violation(format!("field {:?} must be a number or string", self.field));
        };
        self.parse_lexical(lexical, payload.fenced, value_was_string, bounds)
    }

    fn parse_lexical(
        &self,
        lexical: &str,
        fenced: bool,
        value_was_string: bool,
        bounds: Option<(f64, f64)>,
    ) -> ParseOutcome<f64> {
        let trimmed = lexical.trim();
        let Ok(mut action) = trimmed.parse::<f64>() else {
            return format_violation(format!("field {:?} is not a number", self.field));
        };
        if !action.is_finite() {
            return format_violation(format!("field {:?} must be finite", self.field));
        }

        let mut clamped = false;
        if let Some((lo, hi)) = bounds {
            if action < lo || action > hi {
                match self.out_of_range {
                    OutOfRange::Clamp => {
                        action = action.clamp(lo, hi);
                        clamped = true;
                    }
                    OutOfRange::Violation => {
                        return ParseOutcome::Violation(Violation {
                            kind: ViolationKind::Range,
                            message: format!(
                                "field {:?} must be between {lo} and {hi}",
                                self.field
                            ),
                        });
                    }
                }
            }
        }

        ParseOutcome::Ok {
            action,
            flags: ParseFlags {
                fenced,
                value_was_string,
                clamped,
                lenient_match: false,
            },
        }
    }
}

impl NumberParser {
    /// The bounds a continuous game's `role` is parsed against: the parser's
    /// own bounds when set, otherwise the game's bounds of the only component.
    fn effective_bounds<G: ContinuousGame>(&self, game: &G, role: usize) -> Option<(f64, f64)> {
        self.bounds.or_else(|| game.bounds(role).first().copied())
    }
}

/// Parses one number into the one-dimensional action of a continuous game.
///
/// The game's bounds are authoritative: [`ActionParser::validate`] rejects
/// games with an action of more than one component, and parser bounds that
/// do not lie inside the game's bounds. A value is checked against the
/// parser's own bounds when set, otherwise against the game's bounds.
impl<G: ContinuousGame> ActionParser<G> for NumberParser {
    fn parse(&self, raw: &str, game: &G, role: Option<usize>) -> ParseOutcome<Vec<f64>> {
        let role = role.unwrap_or(0);
        if role >= game.n_roles() {
            return ParseOutcome::Violation(Violation {
                kind: ViolationKind::Range,
                message: format!("role {role} is out of range"),
            });
        }
        let dims = game.bounds(role).len();
        if dims != 1 {
            return ParseOutcome::Violation(Violation {
                kind: ViolationKind::InvalidAction,
                message: format!("a single number cannot encode an action with {dims} components"),
            });
        }
        match self.parse_number_within(raw, self.effective_bounds(game, role)) {
            ParseOutcome::Ok { action, flags } => ParseOutcome::Ok {
                action: vec![action],
                flags,
            },
            ParseOutcome::Violation(violation) => ParseOutcome::Violation(violation),
        }
    }

    fn sample_uniform(&self, game: &G, role: Option<usize>, rng: &mut SimRng) -> Vec<f64> {
        let role = role.unwrap_or(0);
        let (lo, hi) = self
            .effective_bounds(game, role)
            .expect("validate requires a one-dimensional action");
        vec![rng.gen_range(lo..=hi)]
    }

    fn validate(&self, game: &G) -> Result<(), RuleError> {
        check_bounds(game).map_err(|error| RuleError::Unsupported(error.to_string()))?;
        for role in 0..game.n_roles() {
            let bounds = game.bounds(role);
            if bounds.len() != 1 {
                return Err(RuleError::Unsupported(format!(
                    "NumberParser supports only one-dimensional actions; role {role} has {} components",
                    bounds.len()
                )));
            }
            if let Some((own_lo, own_hi)) = self.bounds {
                let (lo, hi) = bounds[0];
                if !(own_lo <= own_hi && own_lo >= lo && own_hi <= hi) {
                    return Err(RuleError::Unsupported(format!(
                        "NumberParser bounds [{own_lo}, {own_hi}] must lie inside the bounds [{lo}, {hi}] of role {role}"
                    )));
                }
            }
        }
        Ok(())
    }
}

fn format_violation<A>(message: impl Into<String>) -> ParseOutcome<A> {
    ParseOutcome::Violation(Violation {
        kind: ViolationKind::Format,
        message: message.into(),
    })
}

#[derive(Clone, Debug, PartialEq)]
/// Successful parsed action or a retryable response violation.
pub enum ParseOutcome<A> {
    /// Parsing produced an action together with diagnostic properties.
    Ok {
        /// Parsed action, already normalized by the parser.
        action: A,
        /// Properties of the accepted representation.
        flags: ParseFlags,
    },
    /// The response cannot be used as an action.
    Violation(Violation),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Non-fatal properties recorded for an accepted response.
pub struct ParseFlags {
    /// The response consisted of one Markdown code fence.
    pub fenced: bool,
    /// The selected JSON value was encoded as a string.
    pub value_was_string: bool,
    /// A numeric value was moved to the nearest configured bound.
    pub clamped: bool,
    /// A finite label was found through lenient text matching.
    pub lenient_match: bool,
}
