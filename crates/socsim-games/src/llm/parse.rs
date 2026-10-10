use std::collections::BTreeMap;

use rand::Rng;
use serde::{Deserialize, Serialize};
use socsim_core::SimRng;
use socsim_llm::{
    extract_first_choice, is_single_fenced, json_field_was_string, parse_json_object,
};

use super::{Violation, ViolationKind};
use crate::{FiniteGame, RuleError, StageGame};

pub trait ActionParser<G: StageGame>: Send + Sync {
    fn parse(&self, raw: &str, game: &G, role: Option<usize>) -> ParseOutcome<G::Action>;

    fn sample_uniform(&self, game: &G, role: Option<usize>, rng: &mut SimRng) -> G::Action;

    fn validate(&self, _game: &G) -> Result<(), RuleError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct LabelParser {
    lenient: bool,
    require_reason: bool,
    synonyms: BTreeMap<String, Vec<String>>,
}

impl LabelParser {
    pub fn strict() -> Self {
        Self::default()
    }

    pub fn lenient() -> Self {
        Self {
            lenient: true,
            ..Self::default()
        }
    }

    pub fn require_reason(mut self, yes: bool) -> Self {
        self.require_reason = yes;
        self
    }

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
pub struct NumberParser {
    field: String,
    bounds: Option<(f64, f64)>,
    out_of_range: OutOfRange,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutOfRange {
    Clamp,
    #[default]
    Violation,
}

impl NumberParser {
    pub fn new(field: &str) -> Self {
        Self {
            field: field.to_owned(),
            bounds: None,
            out_of_range: OutOfRange::Violation,
        }
    }

    pub fn bounds(mut self, lo: f64, hi: f64) -> Self {
        self.bounds = Some((lo, hi));
        self
    }

    pub fn out_of_range(mut self, policy: OutOfRange) -> Self {
        self.out_of_range = policy;
        self
    }

    pub fn parse_number(&self, raw: &str) -> ParseOutcome<f64> {
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
            return self.parse_lexical(&value.to_string(), payload.fenced, value_was_string);
        } else {
            return format_violation(format!("field {:?} must be a number or string", self.field));
        };
        self.parse_lexical(lexical, payload.fenced, value_was_string)
    }

    fn parse_lexical(
        &self,
        lexical: &str,
        fenced: bool,
        value_was_string: bool,
    ) -> ParseOutcome<f64> {
        let trimmed = lexical.trim();
        let Ok(mut action) = trimmed.parse::<f64>() else {
            return format_violation(format!("field {:?} is not a number", self.field));
        };
        if !action.is_finite() {
            return format_violation(format!("field {:?} must be finite", self.field));
        }

        let mut clamped = false;
        if let Some((lo, hi)) = self.bounds {
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

fn format_violation<A>(message: impl Into<String>) -> ParseOutcome<A> {
    ParseOutcome::Violation(Violation {
        kind: ViolationKind::Format,
        message: message.into(),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParseOutcome<A> {
    Ok { action: A, flags: ParseFlags },
    Violation(Violation),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseFlags {
    pub fenced: bool,
    pub value_was_string: bool,
    pub clamped: bool,
    pub lenient_match: bool,
}
