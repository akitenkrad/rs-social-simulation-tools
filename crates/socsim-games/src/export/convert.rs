use crate::solve::{
    BrIteration, BrStatus, Equilibrium, EquilibriumSet, EssVerdict, FixedPointSet,
    LemkeHowsonResult, MinimaxSolution, PureNash, QreSolution, SupportFinding,
};

use super::{EssLabel, ExportError, SolutionSet};

impl From<EssVerdict> for EssLabel {
    fn from(verdict: EssVerdict) -> Self {
        #[allow(unreachable_patterns)]
        match verdict {
            EssVerdict::True => Self::True,
            EssVerdict::False => Self::False,
            EssVerdict::Inconclusive => Self::Inconclusive,
            _ => Self::Inconclusive,
        }
    }
}

impl SolutionSet {
    /// Adds every pure Nash equilibrium in solver order using `prefix-k` IDs.
    pub fn push_pure_nash_all(
        &mut self,
        prefix: &str,
        equilibria: &[PureNash],
    ) -> Result<(), ExportError> {
        let counts = self.finite_action_counts(prefix, "nash")?;
        for (index, equilibrium) in equilibria.iter().enumerate() {
            let id = format!("{prefix}-{index}");
            if equilibrium.profile.len() != counts.len() {
                return Err(ExportError::Mismatch(format!(
                    "solution `{id}` profile has {} roles, expected {}",
                    equilibrium.profile.len(),
                    counts.len()
                )));
            }
            let mut strategies = Vec::with_capacity(counts.len());
            for (role, (&action, &count)) in equilibrium.profile.iter().zip(&counts).enumerate() {
                let mut strategy = vec![0.0; count];
                let Some(probability) = strategy.get_mut(action) else {
                    return Err(ExportError::Mismatch(format!(
                        "solution `{id}` profile for role {role} contains action {action}, but the role has {count} actions"
                    )));
                };
                *probability = 1.0;
                strategies.push(strategy);
            }
            self.push_nash(
                &id,
                "pure_nash",
                strategies,
                equilibrium.payoffs.clone(),
                equilibrium.regret,
            )?;
        }
        Ok(())
    }

    /// Adds one verified equilibrium with the supplied identifier and solver.
    pub fn push_equilibrium(
        &mut self,
        id: &str,
        solver: &str,
        equilibrium: &Equilibrium,
    ) -> Result<(), ExportError> {
        self.push_nash(
            id,
            solver,
            equilibrium
                .strategies
                .iter()
                .map(|strategy| strategy.probs().to_vec())
                .collect(),
            equilibrium.payoffs.clone(),
            equilibrium.regret,
        )
    }

    /// Adds support-enumeration equilibria and findings in solver order.
    ///
    /// Equilibria use `prefix-k`, findings use `prefix-fk`, and a verified base
    /// of a component uses `prefix-ck`, where `k` is the index in its source
    /// collection.
    pub fn push_equilibrium_set(
        &mut self,
        prefix: &str,
        set: &EquilibriumSet,
    ) -> Result<(), ExportError> {
        for (index, equilibrium) in set.equilibria.iter().enumerate() {
            self.push_equilibrium(
                &format!("{prefix}-{index}"),
                "support_enumeration",
                equilibrium,
            )?;
        }
        for (index, finding) in set.findings.iter().enumerate() {
            match finding {
                SupportFinding::Degenerate { support, reason }
                | SupportFinding::Inconclusive { support, reason } => self.push_inconclusive(
                    &format!("{prefix}-f{index}"),
                    "support_enumeration",
                    support.clone(),
                    reason,
                )?,
                SupportFinding::Component { support, base, .. } => {
                    self.push_equilibrium(
                        &format!("{prefix}-c{index}"),
                        "support_enumeration",
                        base,
                    )?;
                    self.push_inconclusive(
                        &format!("{prefix}-f{index}"),
                        "support_enumeration",
                        support.clone(),
                        "component",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Adds a Lemke--Howson equilibrium.
    pub fn push_lemke_howson(
        &mut self,
        id: &str,
        result: &LemkeHowsonResult,
    ) -> Result<(), ExportError> {
        self.push_equilibrium(id, "lemke_howson", &result.equilibrium)
    }

    /// Adds a two-player zero-sum minimax solution.
    pub fn push_minimax(
        &mut self,
        id: &str,
        solution: &MinimaxSolution,
    ) -> Result<(), ExportError> {
        self.push_nash(
            id,
            "zero_sum_minimax",
            vec![solution.row.probs().to_vec(), solution.col.probs().to_vec()],
            vec![solution.value, -solution.value],
            solution.regret,
        )
    }

    /// Adds a logit quantal-response equilibrium.
    pub fn push_qre_solution(
        &mut self,
        id: &str,
        solution: &QreSolution,
    ) -> Result<(), ExportError> {
        self.push_qre(
            id,
            solution.lambda,
            solution
                .strategies
                .iter()
                .map(|strategy| strategy.probs().to_vec())
                .collect(),
            solution.residual,
        )
    }

    /// Adds replicator fixed points, components, and inconclusive supports.
    ///
    /// Isolated points use `prefix-k`, components use `prefix-ck`, and
    /// inconclusive supports use `prefix-fk`. `ess` must have one entry for
    /// every isolated point.
    pub fn push_fixed_point_set(
        &mut self,
        prefix: &str,
        set: &FixedPointSet,
        ess: &[Option<EssVerdict>],
    ) -> Result<(), ExportError> {
        if ess.len() != set.isolated.len() {
            return Err(ExportError::Mismatch(format!(
                "fixed-point ESS labels have length {}, expected {}",
                ess.len(),
                set.isolated.len()
            )));
        }
        let n_roles = self.finite_action_counts(prefix, "population")?.len();
        for (index, (point, verdict)) in set.isolated.iter().zip(ess).enumerate() {
            self.push_population(
                &format!("{prefix}-{index}"),
                "replicator_fixed_points",
                point.point.probs().to_vec(),
                verdict.map(EssLabel::from),
            )?;
        }
        for (index, component) in set.components.iter().enumerate() {
            self.push_population_component(
                &format!("{prefix}-c{index}"),
                "replicator_fixed_points",
                component.base.probs().to_vec(),
                component.directions.clone(),
            )?;
        }
        for (index, support) in set.inconclusive.iter().enumerate() {
            self.push_inconclusive(
                &format!("{prefix}-f{index}"),
                "replicator_fixed_points",
                vec![support.clone(); n_roles],
                "replicator_fixed_points",
            )?;
        }
        Ok(())
    }

    /// Adds the result of continuous best-response iteration.
    pub fn push_br_iteration(
        &mut self,
        id: &str,
        iteration: &BrIteration,
    ) -> Result<(), ExportError> {
        self.push_continuous(
            id,
            "best_response_iteration",
            iteration.profile.clone(),
            iteration.residual,
            matches!(iteration.status, BrStatus::Converged),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};

    use serde::Serialize;

    use crate::solve::replicator::replicator_derivative;

    const REGEN_ENV: &str = "SOCSIM_GAMES_REGEN_FIXTURES";
    const N_GRID: usize = 4;

    type TestResult = Result<(), Box<dyn Error>>;

    #[derive(Serialize)]
    struct VectorFixture {
        schema: &'static str,
        version: u32,
        games: Vec<GameVectors>,
    }

    #[derive(Serialize)]
    struct GameVectors {
        game: &'static str,
        n_grid: usize,
        points: Vec<Vec<f64>>,
        derivatives: Vec<Vec<f64>>,
    }

    fn fixture_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/viz/replicator_vectors.json")
    }

    fn vectors(game: &'static str, payoffs: &[Vec<f64>], points: Vec<Vec<f64>>) -> GameVectors {
        let derivatives = points
            .iter()
            .map(|point| replicator_derivative(payoffs, point))
            .collect();
        GameVectors {
            game,
            n_grid: N_GRID,
            points,
            derivatives,
        }
    }

    fn simplex_points() -> Vec<Vec<f64>> {
        let n = N_GRID as f64;
        (0..=N_GRID)
            .flat_map(|i| {
                (0..=N_GRID - i)
                    .map(move |j| vec![i as f64 / n, j as f64 / n, (N_GRID - i - j) as f64 / n])
            })
            .collect()
    }

    fn line_points() -> Vec<Vec<f64>> {
        let n = N_GRID as f64;
        (0..=N_GRID)
            .map(|i| vec![i as f64 / n, (N_GRID - i) as f64 / n])
            .collect()
    }

    #[test]
    fn replicator_vectors_match_fixture_byte_for_byte() -> TestResult {
        let fixture = VectorFixture {
            schema: "socsim-games.replicator-vectors",
            version: 1,
            games: vec![
                vectors(
                    "rock_paper_scissors",
                    &[
                        vec![0.0, -1.0, 1.0],
                        vec![1.0, 0.0, -1.0],
                        vec![-1.0, 1.0, 0.0],
                    ],
                    simplex_points(),
                ),
                vectors(
                    "hawk_dove",
                    &[vec![-1.0, 2.0], vec![0.0, 1.0]],
                    line_points(),
                ),
            ],
        };
        let mut actual = serde_json::to_vec_pretty(&fixture)?;
        actual.push(b'\n');
        let path = fixture_path();
        if std::env::var(REGEN_ENV).as_deref() == Ok("1") {
            fs::write(path, &actual)?;
        } else {
            assert_eq!(actual, fs::read(path)?);
        }
        Ok(())
    }

    #[test]
    fn replicator_derivative_matches_known_values() {
        let rps = [
            vec![0.0, -1.0, 1.0],
            vec![1.0, 0.0, -1.0],
            vec![-1.0, 1.0, 0.0],
        ];
        assert_eq!(
            replicator_derivative(&rps, &[0.5, 0.5, 0.0]),
            [-0.25, 0.25, 0.0]
        );
        assert_eq!(
            replicator_derivative(&rps, &[1.0 / 3.0; 3]),
            [0.0, 0.0, 0.0]
        );
        let hawk_dove = [vec![-1.0, 2.0], vec![0.0, 1.0]];
        assert_eq!(replicator_derivative(&hawk_dove, &[0.5, 0.5]), [0.0, 0.0]);
    }
}
