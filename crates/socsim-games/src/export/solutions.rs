use std::collections::HashSet;
use std::io::Write;

use serde::Serialize;

use super::spec::{GameCopy, GameCopyData};
use super::{ExportError, GameSpec};

const PROBABILITY_TOLERANCE: f64 = 1e-9;

/// The evolutionary-stability label attached to a population solution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum EssLabel {
    /// The population state is evolutionarily stable.
    True,
    /// The population state is not evolutionarily stable.
    False,
    /// The available checks did not establish a conclusive result.
    Inconclusive,
}

/// A validated collection of solutions for one [`GameSpec`].
///
/// The set stores a copy of the identifying game shape for `solutions.json`.
/// Each insertion checks the solution kind, dimensions, finite values,
/// probability vectors, and identifier uniqueness before changing the set.
#[derive(Debug)]
pub struct SolutionSet {
    game: GameCopy,
    ids: HashSet<String>,
    solutions: Vec<Solution>,
}

#[derive(Debug)]
enum Solution {
    Nash {
        id: String,
        solver: String,
        strategies: Vec<Vec<f64>>,
        payoffs: Vec<f64>,
        regret: f64,
        pure: bool,
    },
    Qre {
        id: String,
        lambda: f64,
        strategies: Vec<Vec<f64>>,
        residual: f64,
    },
    Population {
        id: String,
        solver: String,
        shares: Vec<f64>,
        ess: Option<EssLabel>,
    },
    PopulationComponent {
        id: String,
        solver: String,
        base: Vec<f64>,
        directions: Vec<Vec<f64>>,
    },
    Continuous {
        id: String,
        solver: String,
        actions: Vec<Vec<f64>>,
        residual: f64,
        converged: bool,
    },
    Inconclusive {
        id: String,
        solver: String,
        support: Vec<Vec<usize>>,
        reason: String,
    },
}

impl SolutionSet {
    /// Creates an empty solution set tied to a copy of `spec`'s game identity.
    pub fn new(spec: &GameSpec) -> Self {
        Self {
            game: spec.game_copy(),
            ids: HashSet::new(),
            solutions: Vec::new(),
        }
    }

    /// Adds a finite-game Nash equilibrium.
    pub fn push_nash(
        &mut self,
        id: &str,
        solver: &str,
        strategies: Vec<Vec<f64>>,
        payoffs: Vec<f64>,
        regret: f64,
    ) -> Result<(), ExportError> {
        let action_counts = self.finite_action_counts(id, "nash")?;
        validate_strategies(id, &strategies, &action_counts)?;
        validate_role_values(id, "payoffs", &payoffs, self.game.n_roles)?;
        validate_finite(&format!("solutions[{id}].regret"), regret)?;
        self.ensure_unique(id)?;
        let pure = strategies
            .iter()
            .flatten()
            .all(|&probability| probability == 0.0 || probability == 1.0);
        self.insert(
            id,
            Solution::Nash {
                id: id.to_owned(),
                solver: solver.to_owned(),
                strategies,
                payoffs,
                regret,
                pure,
            },
        );
        Ok(())
    }

    /// Adds a finite-game logit quantal-response equilibrium.
    pub fn push_qre(
        &mut self,
        id: &str,
        lambda: f64,
        strategies: Vec<Vec<f64>>,
        residual: f64,
    ) -> Result<(), ExportError> {
        let action_counts = self.finite_action_counts(id, "qre")?;
        validate_strategies(id, &strategies, &action_counts)?;
        validate_finite(&format!("solutions[{id}].lambda"), lambda)?;
        validate_finite(&format!("solutions[{id}].residual"), residual)?;
        self.ensure_unique(id)?;
        self.insert(
            id,
            Solution::Qre {
                id: id.to_owned(),
                lambda,
                strategies,
                residual,
            },
        );
        Ok(())
    }

    /// Adds a population state for a finite game with one common action count.
    pub fn push_population(
        &mut self,
        id: &str,
        solver: &str,
        shares: Vec<f64>,
        ess: Option<EssLabel>,
    ) -> Result<(), ExportError> {
        let count = self.common_action_count(id, "population")?;
        validate_probability(id, 0, &shares, count)?;
        self.ensure_unique(id)?;
        self.insert(
            id,
            Solution::Population {
                id: id.to_owned(),
                solver: solver.to_owned(),
                shares,
                ess,
            },
        );
        Ok(())
    }

    /// Adds an affine component of population fixed points.
    pub fn push_population_component(
        &mut self,
        id: &str,
        solver: &str,
        base: Vec<f64>,
        directions: Vec<Vec<f64>>,
    ) -> Result<(), ExportError> {
        let count = self.common_action_count(id, "population_component")?;
        validate_probability(id, 0, &base, count)?;
        for (direction, values) in directions.iter().enumerate() {
            if values.len() != count {
                return Err(ExportError::Mismatch(format!(
                    "solution `{id}` direction {direction} has length {}, expected {count}",
                    values.len()
                )));
            }
            for (action, &value) in values.iter().enumerate() {
                validate_finite(
                    &format!("solutions[{id}].directions[{direction}][{action}]"),
                    value,
                )?;
            }
        }
        self.ensure_unique(id)?;
        self.insert(
            id,
            Solution::PopulationComponent {
                id: id.to_owned(),
                solver: solver.to_owned(),
                base,
                directions,
            },
        );
        Ok(())
    }

    /// Adds a continuous-game action profile.
    pub fn push_continuous(
        &mut self,
        id: &str,
        solver: &str,
        actions: Vec<Vec<f64>>,
        residual: f64,
        converged: bool,
    ) -> Result<(), ExportError> {
        let dimensions = self.continuous_dimensions(id, "continuous")?;
        if actions.len() != dimensions.len() {
            return Err(ExportError::Mismatch(format!(
                "solution `{id}` has {} roles, expected {}",
                actions.len(),
                dimensions.len()
            )));
        }
        for (role, (role_actions, &expected)) in actions.iter().zip(&dimensions).enumerate() {
            if role_actions.len() != expected {
                return Err(ExportError::Mismatch(format!(
                    "solution `{id}` role {role} has {} dimensions, expected {expected}",
                    role_actions.len()
                )));
            }
            for (dim, &value) in role_actions.iter().enumerate() {
                validate_finite(&format!("solutions[{id}].actions[{role}][{dim}]"), value)?;
            }
        }
        validate_finite(&format!("solutions[{id}].residual"), residual)?;
        self.ensure_unique(id)?;
        self.insert(
            id,
            Solution::Continuous {
                id: id.to_owned(),
                solver: solver.to_owned(),
                actions,
                residual,
                converged,
            },
        );
        Ok(())
    }

    /// Adds a finite-game support that a solver could not resolve.
    pub fn push_inconclusive(
        &mut self,
        id: &str,
        solver: &str,
        support: Vec<Vec<usize>>,
        reason: &str,
    ) -> Result<(), ExportError> {
        let action_counts = self.finite_action_counts(id, "inconclusive")?;
        if support.len() != action_counts.len() {
            return Err(ExportError::Mismatch(format!(
                "solution `{id}` support has {} roles, expected {}",
                support.len(),
                action_counts.len()
            )));
        }
        for (role, (actions, &count)) in support.iter().zip(&action_counts).enumerate() {
            if let Some(&action) = actions.iter().find(|&&action| action >= count) {
                return Err(ExportError::Mismatch(format!(
                    "solution `{id}` support for role {role} contains action {action}, but the role has {count} actions"
                )));
            }
        }
        self.ensure_unique(id)?;
        self.insert(
            id,
            Solution::Inconclusive {
                id: id.to_owned(),
                solver: solver.to_owned(),
                support,
                reason: reason.to_owned(),
            },
        );
        Ok(())
    }

    pub(super) fn finite_action_counts(
        &self,
        id: &str,
        kind: &str,
    ) -> Result<Vec<usize>, ExportError> {
        match &self.game.data {
            GameCopyData::Finite { action_labels } => {
                Ok(action_labels.iter().map(Vec::len).collect())
            }
            GameCopyData::Continuous { .. } => Err(ExportError::Mismatch(format!(
                "solution `{id}` of kind `{kind}` requires a finite game"
            ))),
        }
    }

    fn continuous_dimensions(&self, id: &str, kind: &str) -> Result<Vec<usize>, ExportError> {
        match &self.game.data {
            GameCopyData::Continuous { dimension_labels } => {
                Ok(dimension_labels.iter().map(Vec::len).collect())
            }
            GameCopyData::Finite { .. } => Err(ExportError::Mismatch(format!(
                "solution `{id}` of kind `{kind}` requires a continuous game"
            ))),
        }
    }

    fn common_action_count(&self, id: &str, kind: &str) -> Result<usize, ExportError> {
        let counts = self.finite_action_counts(id, kind)?;
        let Some(&first) = counts.first() else {
            return Err(ExportError::Mismatch(format!(
                "solution `{id}` requires at least one role"
            )));
        };
        if let Some((role, count)) = counts
            .iter()
            .copied()
            .enumerate()
            .find(|&(_, count)| count != first)
        {
            return Err(ExportError::Mismatch(format!(
                "solution `{id}` requires one common action count, but role 0 has {first} actions and role {role} has {count}"
            )));
        }
        Ok(first)
    }

    fn ensure_unique(&self, id: &str) -> Result<(), ExportError> {
        if self.ids.contains(id) {
            return Err(ExportError::DuplicateId(id.to_owned()));
        }
        Ok(())
    }

    fn insert(&mut self, id: &str, solution: Solution) {
        let inserted = self.ids.insert(id.to_owned());
        debug_assert!(inserted);
        self.solutions.push(solution);
    }

    pub(crate) fn matches(&self, spec: &GameSpec) -> bool {
        self.game == spec.game_copy()
    }

    pub(crate) fn write_pretty(&self, writer: impl Write) -> serde_json::Result<()> {
        serde_json::to_writer_pretty(writer, &SolutionsJson::from(self))
    }
}

fn validate_strategies(
    id: &str,
    strategies: &[Vec<f64>],
    action_counts: &[usize],
) -> Result<(), ExportError> {
    if strategies.len() != action_counts.len() {
        return Err(ExportError::Mismatch(format!(
            "solution `{id}` has {} roles, expected {}",
            strategies.len(),
            action_counts.len()
        )));
    }
    for (role, (strategy, &count)) in strategies.iter().zip(action_counts).enumerate() {
        validate_probability(id, role, strategy, count)?;
    }
    Ok(())
}

fn validate_probability(
    id: &str,
    role: usize,
    values: &[f64],
    expected: usize,
) -> Result<(), ExportError> {
    if values.len() != expected {
        return Err(ExportError::Probability {
            id: id.to_owned(),
            role,
            detail: format!("length is {}, expected {expected}", values.len()),
        });
    }
    for (action, &value) in values.iter().enumerate() {
        if !value.is_finite() {
            return Err(ExportError::NonFinite {
                field: format!("solutions[{id}].probabilities[{role}][{action}]"),
            });
        }
        if value < 0.0 {
            return Err(ExportError::Probability {
                id: id.to_owned(),
                role,
                detail: format!("action {action} has negative probability {value}"),
            });
        }
    }
    let sum = values.iter().sum::<f64>();
    if (sum - 1.0).abs() > PROBABILITY_TOLERANCE {
        return Err(ExportError::Probability {
            id: id.to_owned(),
            role,
            detail: format!("sum is {sum}, expected 1 within {PROBABILITY_TOLERANCE}"),
        });
    }
    Ok(())
}

fn validate_role_values(
    id: &str,
    field: &str,
    values: &[f64],
    expected: usize,
) -> Result<(), ExportError> {
    if values.len() != expected {
        return Err(ExportError::Mismatch(format!(
            "solution `{id}` field `{field}` has length {}, expected {expected}",
            values.len()
        )));
    }
    for (role, &value) in values.iter().enumerate() {
        validate_finite(&format!("solutions[{id}].{field}[{role}]"), value)?;
    }
    Ok(())
}

fn validate_finite(field: &str, value: f64) -> Result<(), ExportError> {
    if !value.is_finite() {
        return Err(ExportError::NonFinite {
            field: field.to_owned(),
        });
    }
    Ok(())
}

#[derive(Serialize)]
struct SolutionsJson<'a> {
    schema: &'static str,
    version: u32,
    game: GameJson<'a>,
    solutions: Vec<SolutionJson<'a>>,
}

impl<'a> From<&'a SolutionSet> for SolutionsJson<'a> {
    fn from(set: &'a SolutionSet) -> Self {
        Self {
            schema: "socsim-games.solutions",
            version: 1,
            game: GameJson::from(&set.game),
            solutions: set.solutions.iter().map(SolutionJson::from).collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum GameJson<'a> {
    Finite(FiniteGameJson<'a>),
    Continuous(ContinuousGameJson<'a>),
}

impl<'a> From<&'a GameCopy> for GameJson<'a> {
    fn from(game: &'a GameCopy) -> Self {
        match &game.data {
            GameCopyData::Finite { action_labels } => Self::Finite(FiniteGameJson {
                name: &game.name,
                n_roles: game.n_roles,
                action_labels,
            }),
            GameCopyData::Continuous { dimension_labels } => Self::Continuous(ContinuousGameJson {
                name: &game.name,
                n_roles: game.n_roles,
                dimension_labels,
            }),
        }
    }
}

#[derive(Serialize)]
struct FiniteGameJson<'a> {
    name: &'a str,
    n_roles: usize,
    action_labels: &'a [Vec<String>],
}

#[derive(Serialize)]
struct ContinuousGameJson<'a> {
    name: &'a str,
    n_roles: usize,
    dimension_labels: &'a [Vec<String>],
}

#[derive(Serialize)]
#[serde(untagged)]
enum SolutionJson<'a> {
    Nash(NashJson<'a>),
    Qre(QreJson<'a>),
    Population(PopulationJson<'a>),
    PopulationComponent(PopulationComponentJson<'a>),
    Continuous(ContinuousJson<'a>),
    Inconclusive(InconclusiveJson<'a>),
}

impl<'a> From<&'a Solution> for SolutionJson<'a> {
    fn from(solution: &'a Solution) -> Self {
        match solution {
            Solution::Nash {
                id,
                solver,
                strategies,
                payoffs,
                regret,
                pure,
            } => Self::Nash(NashJson {
                id,
                solver,
                kind: "nash",
                strategies,
                payoffs,
                regret: *regret,
                pure: *pure,
            }),
            Solution::Qre {
                id,
                lambda,
                strategies,
                residual,
            } => Self::Qre(QreJson {
                id,
                solver: "logit_qre",
                kind: "qre",
                lambda: *lambda,
                strategies,
                residual: *residual,
            }),
            Solution::Population {
                id,
                solver,
                shares,
                ess,
            } => Self::Population(PopulationJson {
                id,
                solver,
                kind: "population",
                shares,
                ess: *ess,
            }),
            Solution::PopulationComponent {
                id,
                solver,
                base,
                directions,
            } => Self::PopulationComponent(PopulationComponentJson {
                id,
                solver,
                kind: "population_component",
                base,
                directions,
            }),
            Solution::Continuous {
                id,
                solver,
                actions,
                residual,
                converged,
            } => Self::Continuous(ContinuousJson {
                id,
                solver,
                kind: "continuous",
                actions,
                residual: *residual,
                converged: *converged,
            }),
            Solution::Inconclusive {
                id,
                solver,
                support,
                reason,
            } => Self::Inconclusive(InconclusiveJson {
                id,
                solver,
                kind: "inconclusive",
                support,
                reason,
            }),
        }
    }
}

#[derive(Serialize)]
struct NashJson<'a> {
    id: &'a str,
    solver: &'a str,
    kind: &'static str,
    strategies: &'a [Vec<f64>],
    payoffs: &'a [f64],
    regret: f64,
    pure: bool,
}

#[derive(Serialize)]
struct QreJson<'a> {
    id: &'a str,
    solver: &'static str,
    kind: &'static str,
    lambda: f64,
    strategies: &'a [Vec<f64>],
    residual: f64,
}

#[derive(Serialize)]
struct PopulationJson<'a> {
    id: &'a str,
    solver: &'a str,
    kind: &'static str,
    shares: &'a [f64],
    ess: Option<EssLabel>,
}

#[derive(Serialize)]
struct PopulationComponentJson<'a> {
    id: &'a str,
    solver: &'a str,
    kind: &'static str,
    base: &'a [f64],
    directions: &'a [Vec<f64>],
}

#[derive(Serialize)]
struct ContinuousJson<'a> {
    id: &'a str,
    solver: &'a str,
    kind: &'static str,
    actions: &'a [Vec<f64>],
    residual: f64,
    converged: bool,
}

#[derive(Serialize)]
struct InconclusiveJson<'a> {
    id: &'a str,
    solver: &'a str,
    kind: &'static str,
    support: &'a [Vec<usize>],
    reason: &'a str,
}
