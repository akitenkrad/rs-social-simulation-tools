use std::io::Write;

use serde::Serialize;
use serde_json::Value;

use crate::{metric_action_ids, ContinuousGame, FiniteGame, MetricAction, MetricScheme};

use super::ExportError;

const MAX_EXPORT_ELEMENTS: u128 = 1_000_000;

/// A validated game definition for `game_spec.json`.
///
/// Finite specifications contain the complete row-major payoff tensors,
/// action labels, symmetry flag, and stable metric action identifiers.
/// Continuous specifications contain bounds and dimension labels. Both forms
/// contain a name, role count, and role labels. Fields are private so every
/// value written to JSON has passed the constructor checks.
#[derive(Debug)]
pub struct GameSpec {
    name: String,
    n_roles: usize,
    role_labels: Vec<String>,
    data: SpecData,
    params: Option<Value>,
}

#[derive(Debug)]
enum SpecData {
    Finite(FiniteData),
    Continuous(ContinuousData),
}

#[derive(Debug)]
struct FiniteData {
    action_labels: Vec<Vec<String>>,
    shape: Vec<usize>,
    values: Vec<Vec<f64>>,
    symmetric: bool,
    metric_scheme: &'static str,
    metric_actions: Vec<MetricAction>,
}

#[derive(Debug)]
struct ContinuousData {
    bounds: Vec<Vec<(f64, f64)>>,
    dimension_labels: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GameCopy {
    pub(crate) name: String,
    pub(crate) n_roles: usize,
    pub(crate) data: GameCopyData,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GameCopyData {
    Finite { action_labels: Vec<Vec<String>> },
    Continuous { dimension_labels: Vec<Vec<String>> },
}

impl GameSpec {
    /// Builds a finite specification by enumerating profiles with role 0 as
    /// the outermost axis and the last role as the fastest-changing axis.
    ///
    /// The constructor rejects an empty name, invalid role or action labels,
    /// non-finite payoffs, and payoff tensors with more than one million total
    /// role-payoff elements.
    pub fn from_finite<G: FiniteGame>(
        game: &G,
        name: &str,
        role_labels: Option<Vec<String>>,
    ) -> Result<Self, ExportError> {
        validate_name(name)?;
        let n_roles = game.n_roles();
        let shape = (0..n_roles)
            .map(|role| game.n_actions(role))
            .collect::<Vec<_>>();
        let profiles = shape.iter().fold(1_u128, |count, &actions| {
            count.saturating_mul(actions as u128)
        });
        let elements = profiles.saturating_mul(n_roles as u128);
        if elements > MAX_EXPORT_ELEMENTS {
            return Err(ExportError::TooLarge {
                elements,
                limit: MAX_EXPORT_ELEMENTS,
            });
        }

        let role_labels = validate_role_labels(role_labels, n_roles)?;
        let mut action_labels = Vec::with_capacity(n_roles);
        for (role, &count) in shape.iter().enumerate() {
            let mut labels = Vec::with_capacity(count);
            for action in 0..count {
                let label = game.action_label(role, action).into_owned();
                if label.is_empty() {
                    return Err(ExportError::EmptyLabel {
                        field: format!("action_labels[{role}][{action}]"),
                    });
                }
                labels.push(label);
            }
            action_labels.push(labels);
        }

        let profile_count = usize::try_from(profiles).map_err(|_| ExportError::TooLarge {
            elements,
            limit: MAX_EXPORT_ELEMENTS,
        })?;
        let mut values = (0..n_roles)
            .map(|_| Vec::with_capacity(profile_count))
            .collect::<Vec<_>>();
        let mut profile = vec![0; n_roles];
        for index in 0..profile_count {
            for (role, role_values) in values.iter_mut().enumerate() {
                let payoff = game.payoff(role, &profile);
                if !payoff.is_finite() {
                    return Err(ExportError::NonFinite {
                        field: format!("payoffs[{role}][{index}]"),
                    });
                }
                role_values.push(payoff);
            }
            advance_profile(&mut profile, &shape);
        }

        let metric_ids = metric_action_ids(game);
        let metric_scheme = match metric_ids.scheme() {
            MetricScheme::Pooled => "pooled",
            MetricScheme::PerRole => "per_role",
        };
        Ok(Self {
            name: name.to_owned(),
            n_roles,
            role_labels,
            data: SpecData::Finite(FiniteData {
                action_labels,
                shape,
                values,
                symmetric: game.is_symmetric(),
                metric_scheme,
                metric_actions: metric_ids.actions().to_vec(),
            }),
            params: None,
        })
    }

    /// Builds a continuous specification from explicit role bounds.
    ///
    /// There must be at least one role and one dimension per role. Every bound
    /// must have finite endpoints with `lo < hi`. Dimension labels must match
    /// the bound shape; omitted labels default to `x0`, `x1`, and so on for
    /// each role.
    pub fn continuous(
        name: &str,
        bounds: Vec<Vec<(f64, f64)>>,
        dimension_labels: Option<Vec<Vec<String>>>,
    ) -> Result<Self, ExportError> {
        validate_name(name)?;
        if bounds.is_empty() {
            return Err(ExportError::Shape(
                "bounds must contain at least one role".to_owned(),
            ));
        }
        for (role, role_bounds) in bounds.iter().enumerate() {
            if role_bounds.is_empty() {
                return Err(ExportError::Shape(format!(
                    "bounds[{role}] must contain at least one dimension"
                )));
            }
            for (dim, &(lo, hi)) in role_bounds.iter().enumerate() {
                if !lo.is_finite() || !hi.is_finite() {
                    return Err(ExportError::NonFinite {
                        field: format!("bounds[{role}][{dim}]"),
                    });
                }
                if lo >= hi {
                    return Err(ExportError::Shape(format!(
                        "bounds[{role}][{dim}] must satisfy lo < hi"
                    )));
                }
            }
        }

        let n_roles = bounds.len();
        let dimension_labels = validate_dimension_labels(dimension_labels, &bounds)?;
        Ok(Self {
            name: name.to_owned(),
            n_roles,
            role_labels: default_role_labels(n_roles),
            data: SpecData::Continuous(ContinuousData {
                bounds,
                dimension_labels,
            }),
            params: None,
        })
    }

    /// Builds a continuous specification by copying each role's bounds from a
    /// [`ContinuousGame`].
    pub fn from_continuous<G: ContinuousGame>(
        game: &G,
        name: &str,
        role_labels: Option<Vec<String>>,
        dimension_labels: Option<Vec<Vec<String>>>,
    ) -> Result<Self, ExportError> {
        let n_roles = game.n_roles();
        let bounds = (0..n_roles)
            .map(|role| game.bounds(role).to_vec())
            .collect();
        let spec = Self::continuous(name, bounds, dimension_labels)?;
        match role_labels {
            Some(labels) => spec.with_role_labels(labels),
            None => Ok(spec),
        }
    }

    /// Replaces role labels after applying the same length and empty-label
    /// checks used by the constructors.
    pub fn with_role_labels(mut self, role_labels: Vec<String>) -> Result<Self, ExportError> {
        self.role_labels = validate_role_labels(Some(role_labels), self.n_roles)?;
        Ok(self)
    }

    /// Attaches arbitrary JSON parameters for provenance.
    ///
    /// Parameters are recorded verbatim and are not used for validation or
    /// visualization; the expanded game definition remains authoritative.
    pub fn with_params(mut self, params: Value) -> Self {
        self.params = Some(params);
        self
    }

    pub(crate) fn game_copy(&self) -> GameCopy {
        let data = match &self.data {
            SpecData::Finite(finite) => GameCopyData::Finite {
                action_labels: finite.action_labels.clone(),
            },
            SpecData::Continuous(continuous) => GameCopyData::Continuous {
                dimension_labels: continuous.dimension_labels.clone(),
            },
        };
        GameCopy {
            name: self.name.clone(),
            n_roles: self.n_roles,
            data,
        }
    }

    pub(crate) fn write_pretty(&self, writer: impl Write) -> serde_json::Result<()> {
        serde_json::to_writer_pretty(writer, &SpecJson::from(self))
    }
}

fn validate_name(name: &str) -> Result<(), ExportError> {
    if name.is_empty() {
        return Err(ExportError::EmptyLabel {
            field: "name".to_owned(),
        });
    }
    Ok(())
}

fn default_role_labels(n_roles: usize) -> Vec<String> {
    (0..n_roles).map(|role| format!("role{role}")).collect()
}

fn validate_role_labels(
    role_labels: Option<Vec<String>>,
    n_roles: usize,
) -> Result<Vec<String>, ExportError> {
    let labels = role_labels.unwrap_or_else(|| default_role_labels(n_roles));
    if labels.len() != n_roles {
        return Err(ExportError::Shape(format!(
            "role_labels has length {}, expected {n_roles}",
            labels.len()
        )));
    }
    if let Some(role) = labels.iter().position(String::is_empty) {
        return Err(ExportError::EmptyLabel {
            field: format!("role_labels[{role}]"),
        });
    }
    Ok(labels)
}

fn validate_dimension_labels(
    dimension_labels: Option<Vec<Vec<String>>>,
    bounds: &[Vec<(f64, f64)>],
) -> Result<Vec<Vec<String>>, ExportError> {
    let labels = dimension_labels.unwrap_or_else(|| {
        bounds
            .iter()
            .map(|role_bounds| {
                (0..role_bounds.len())
                    .map(|dim| format!("x{dim}"))
                    .collect()
            })
            .collect()
    });
    if labels.len() != bounds.len() {
        return Err(ExportError::Shape(format!(
            "dimension_labels has {} roles, expected {}",
            labels.len(),
            bounds.len()
        )));
    }
    for (role, (role_labels, role_bounds)) in labels.iter().zip(bounds).enumerate() {
        if role_labels.len() != role_bounds.len() {
            return Err(ExportError::Shape(format!(
                "dimension_labels[{role}] has length {}, expected {}",
                role_labels.len(),
                role_bounds.len()
            )));
        }
        if let Some(dim) = role_labels.iter().position(String::is_empty) {
            return Err(ExportError::EmptyLabel {
                field: format!("dimension_labels[{role}][{dim}]"),
            });
        }
    }
    Ok(labels)
}

fn advance_profile(profile: &mut [usize], shape: &[usize]) {
    for role in (0..profile.len()).rev() {
        profile[role] += 1;
        if profile[role] < shape[role] {
            return;
        }
        profile[role] = 0;
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum SpecJson<'a> {
    Finite(FiniteSpecJson<'a>),
    Continuous(ContinuousSpecJson<'a>),
}

impl<'a> From<&'a GameSpec> for SpecJson<'a> {
    fn from(spec: &'a GameSpec) -> Self {
        match &spec.data {
            SpecData::Finite(finite) => Self::Finite(FiniteSpecJson {
                schema: "socsim-games.game-spec",
                version: 1,
                kind: "finite",
                name: &spec.name,
                n_roles: spec.n_roles,
                role_labels: &spec.role_labels,
                action_labels: &finite.action_labels,
                payoffs: PayoffsJson {
                    layout: "row_major_role0_outer",
                    shape: &finite.shape,
                    values: &finite.values,
                },
                symmetric: finite.symmetric,
                metric_scheme: finite.metric_scheme,
                metric_actions: finite
                    .metric_actions
                    .iter()
                    .map(MetricActionJson::from)
                    .collect(),
                params: spec.params.as_ref(),
            }),
            SpecData::Continuous(continuous) => Self::Continuous(ContinuousSpecJson {
                schema: "socsim-games.game-spec",
                version: 1,
                kind: "continuous",
                name: &spec.name,
                n_roles: spec.n_roles,
                role_labels: &spec.role_labels,
                bounds: &continuous.bounds,
                dimension_labels: &continuous.dimension_labels,
                params: spec.params.as_ref(),
            }),
        }
    }
}

#[derive(Serialize)]
struct FiniteSpecJson<'a> {
    schema: &'static str,
    version: u32,
    kind: &'static str,
    name: &'a str,
    n_roles: usize,
    role_labels: &'a [String],
    action_labels: &'a [Vec<String>],
    payoffs: PayoffsJson<'a>,
    symmetric: bool,
    metric_scheme: &'static str,
    metric_actions: Vec<MetricActionJson<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a Value>,
}

#[derive(Serialize)]
struct PayoffsJson<'a> {
    layout: &'static str,
    shape: &'a [usize],
    values: &'a [Vec<f64>],
}

#[derive(Serialize)]
struct MetricActionJson<'a> {
    id: &'a str,
    role: Option<usize>,
    action: usize,
}

impl<'a> From<&'a MetricAction> for MetricActionJson<'a> {
    fn from(action: &'a MetricAction) -> Self {
        Self {
            id: &action.id,
            role: action.role,
            action: action.action,
        }
    }
}

#[derive(Serialize)]
struct ContinuousSpecJson<'a> {
    schema: &'static str,
    version: u32,
    kind: &'static str,
    name: &'a str,
    n_roles: usize,
    role_labels: &'a [String],
    bounds: &'a [Vec<(f64, f64)>],
    dimension_labels: &'a [Vec<String>],
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a Value>,
}
