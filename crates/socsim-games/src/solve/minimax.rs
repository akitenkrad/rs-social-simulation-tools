use microlp::{
    ComparisonOp, Error as MicrolpError, OptimizationDirection, Problem, Solution, SolutionStatus,
    SolveOutcome, Variable,
};

use crate::mixed::{action_payoffs, expected_payoff, regret, regrets};
use crate::{FiniteGame, MixedStrategy};

use super::{inspect_game, tolerance, LpFailure, SolveError};

/// A verified minimax solution of a two-player zero-sum game.
#[derive(Clone, Debug, PartialEq)]
pub struct MinimaxSolution {
    /// The game value from role 0's perspective.
    pub value: f64,
    /// Role 0's maximizing strategy.
    pub row: MixedStrategy,
    /// Role 1's minimizing strategy.
    pub col: MixedStrategy,
    /// The absolute difference between the primal and dual objective values.
    pub duality_gap: f64,
    /// The maximum regret of the returned strategy profile.
    pub regret: f64,
}

/// A verified maximin guarantee for one role of a two-player game.
#[derive(Clone, Debug, PartialEq)]
pub struct SecurityLevel {
    /// The payoff guaranteed against every pure opponent action.
    pub level: f64,
    /// The role's maximizing strategy.
    pub strategy: MixedStrategy,
    /// The absolute difference between the primal and dual objective values.
    pub duality_gap: f64,
}

struct MatrixSolution {
    primal_value: f64,
    dual_value: f64,
    row: MixedStrategy,
    col: MixedStrategy,
    duality_gap: f64,
}

/// Solves and verifies the minimax strategies of a two-player zero-sum game.
pub fn zero_sum_minimax<G: FiniteGame>(game: &G) -> Result<MinimaxSolution, SolveError> {
    let n_roles = game.n_roles();
    if n_roles != 2 {
        return Err(SolveError::UnsupportedRoles { n_roles });
    }
    let info = inspect_game(game)?;
    let comparison_tolerance = tolerance(game)?;
    debug_assert_eq!(info.tolerance, comparison_tolerance);

    let matrix = (0..info.counts[0])
        .map(|row| {
            (0..info.counts[1])
                .map(|column| {
                    let profile = [row, column];
                    let row_payoff = game.payoff(0, &profile);
                    let column_payoff = game.payoff(1, &profile);
                    if (row_payoff + column_payoff).abs() > comparison_tolerance {
                        Err(SolveError::NotZeroSum)
                    } else {
                        Ok(row_payoff)
                    }
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let solved = solve_matrix(&matrix, comparison_tolerance)?;
    let strategies = vec![solved.row.clone(), solved.col.clone()];
    let per_role_regret = regrets(game, &strategies)?;
    let maximum_regret = regret(game, &strategies)?;
    let payoff_residual = (expected_payoff(game, 0, &strategies)? - solved.primal_value).abs();
    let row_best = action_payoffs(game, 0, &strategies)?
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);
    let column_best = action_payoffs(game, 1, &strategies)?
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);
    let action_value_residual = (row_best - solved.primal_value)
        .abs()
        .max((column_best + solved.primal_value).abs());
    let verification_regret = maximum_regret
        .max(payoff_residual)
        .max(action_value_residual);

    if solved.duality_gap > comparison_tolerance
        || per_role_regret
            .iter()
            .any(|value| *value > comparison_tolerance)
        || maximum_regret > comparison_tolerance
        || payoff_residual > comparison_tolerance
        || action_value_residual > comparison_tolerance
    {
        return Err(SolveError::VerificationFailed {
            duality_gap: solved.duality_gap,
            regret: verification_regret,
        });
    }

    Ok(MinimaxSolution {
        value: solved.primal_value,
        row: solved.row,
        col: solved.col,
        duality_gap: solved.duality_gap,
        regret: maximum_regret,
    })
}

/// Computes and verifies one role's maximin guarantee in a two-player game.
pub fn maximin_security_level<G: FiniteGame>(
    game: &G,
    role: usize,
) -> Result<SecurityLevel, SolveError> {
    let n_roles = game.n_roles();
    if n_roles != 2 {
        return Err(SolveError::UnsupportedRoles { n_roles });
    }
    if role >= n_roles {
        return Err(SolveError::InvalidOptions(format!(
            "role {role} is out of range for {n_roles} roles"
        )));
    }
    let info = inspect_game(game)?;
    let comparison_tolerance = tolerance(game)?;
    debug_assert_eq!(info.tolerance, comparison_tolerance);

    let (own_actions, opponent_actions) = if role == 0 {
        (info.counts[0], info.counts[1])
    } else {
        (info.counts[1], info.counts[0])
    };
    let matrix = (0..own_actions)
        .map(|own_action| {
            (0..opponent_actions)
                .map(|opponent_action| {
                    let profile = if role == 0 {
                        [own_action, opponent_action]
                    } else {
                        [opponent_action, own_action]
                    };
                    game.payoff(role, &profile)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let solved = solve_matrix(&matrix, comparison_tolerance)?;
    let primal_floor = column_payoffs(&matrix, solved.row.probs())
        .into_iter()
        .fold(f64::INFINITY, f64::min);
    let dual_ceiling = row_payoffs(&matrix, solved.col.probs())
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);
    let verification_regret = (solved.primal_value - primal_floor)
        .max(dual_ceiling - solved.dual_value)
        .max(0.0);

    if solved.duality_gap > comparison_tolerance || verification_regret > comparison_tolerance {
        return Err(SolveError::VerificationFailed {
            duality_gap: solved.duality_gap,
            regret: verification_regret,
        });
    }

    Ok(SecurityLevel {
        level: solved.primal_value,
        strategy: solved.row,
        duality_gap: solved.duality_gap,
    })
}

fn solve_matrix(
    matrix: &[Vec<f64>],
    comparison_tolerance: f64,
) -> Result<MatrixSolution, SolveError> {
    let rows = matrix.len();
    let columns = matrix[0].len();

    let mut primal = Problem::new(OptimizationDirection::Maximize);
    let row_vars = (0..rows)
        .map(|_| primal.add_var(0.0, (0.0, 1.0)))
        .collect::<Vec<_>>();
    let primal_value_var = primal.add_var(1.0, (f64::NEG_INFINITY, f64::INFINITY));
    for (column, _) in matrix[0].iter().enumerate() {
        let mut expression = row_vars
            .iter()
            .enumerate()
            .map(|(row, &variable)| (variable, matrix[row][column]))
            .collect::<Vec<_>>();
        expression.push((primal_value_var, -1.0));
        primal.add_constraint(expression, ComparisonOp::Ge, 0.0);
    }
    primal.add_constraint(
        row_vars.iter().map(|&variable| (variable, 1.0)),
        ComparisonOp::Eq,
        1.0,
    );

    let mut dual = Problem::new(OptimizationDirection::Minimize);
    let column_vars = (0..columns)
        .map(|_| dual.add_var(0.0, (0.0, 1.0)))
        .collect::<Vec<_>>();
    let dual_value_var = dual.add_var(1.0, (f64::NEG_INFINITY, f64::INFINITY));
    for row in matrix {
        let mut expression = column_vars
            .iter()
            .zip(row)
            .map(|(&variable, &coefficient)| (variable, coefficient))
            .collect::<Vec<_>>();
        expression.push((dual_value_var, -1.0));
        dual.add_constraint(expression, ComparisonOp::Le, 0.0);
    }
    dual.add_constraint(
        column_vars.iter().map(|&variable| (variable, 1.0)),
        ComparisonOp::Eq,
        1.0,
    );

    let primal_solution = solve_optimal(&primal)?;
    let dual_solution = solve_optimal(&dual)?;
    let primal_value = primal_solution.var_value(primal_value_var);
    let dual_value = dual_solution.var_value(dual_value_var);
    let duality_gap = if primal_value.is_finite() && dual_value.is_finite() {
        (primal_value - dual_value).abs()
    } else {
        f64::INFINITY
    };
    let row = normalized_strategy(
        variable_values(&primal_solution, &row_vars),
        comparison_tolerance,
        duality_gap,
    )?;
    let col = normalized_strategy(
        variable_values(&dual_solution, &column_vars),
        comparison_tolerance,
        duality_gap,
    )?;

    Ok(MatrixSolution {
        primal_value,
        dual_value,
        row,
        col,
        duality_gap,
    })
}

fn solve_optimal(problem: &Problem) -> Result<Solution, SolveError> {
    match problem.solve().map_err(map_lp_error)? {
        SolveOutcome::Solution(solution) => {
            ensure_optimal_status(solution.status())?;
            Ok(solution)
        }
        SolveOutcome::Interrupted(_) => Err(LpFailure::Interrupted.into()),
    }
}

fn map_lp_error(error: MicrolpError) -> SolveError {
    match error {
        MicrolpError::Infeasible => LpFailure::Infeasible.into(),
        MicrolpError::Unbounded => LpFailure::Unbounded.into(),
        other => LpFailure::Internal(other.to_string()).into(),
    }
}

fn ensure_optimal_status(status: SolutionStatus) -> Result<(), SolveError> {
    match status {
        SolutionStatus::Optimal => Ok(()),
        SolutionStatus::Feasible => Err(LpFailure::NotOptimal.into()),
    }
}

fn variable_values(solution: &Solution, variables: &[Variable]) -> Vec<f64> {
    variables
        .iter()
        .map(|&variable| solution.var_value(variable))
        .collect()
}

fn normalized_strategy(
    mut probabilities: Vec<f64>,
    comparison_tolerance: f64,
    duality_gap: f64,
) -> Result<MixedStrategy, SolveError> {
    let mut violation: f64 = 0.0;
    for probability in &mut probabilities {
        if !probability.is_finite() {
            violation = f64::INFINITY;
            break;
        }
        if *probability < 0.0 {
            violation = violation.max(-*probability);
            *probability = 0.0;
        } else if *probability > 1.0 {
            violation = violation.max(*probability - 1.0);
        }
    }
    let sum = probabilities.iter().sum::<f64>();
    if !sum.is_finite() || sum <= 0.0 {
        violation = f64::INFINITY;
    } else {
        violation = violation.max((sum - 1.0).abs());
    }
    if violation > comparison_tolerance {
        return Err(SolveError::VerificationFailed {
            duality_gap,
            regret: violation,
        });
    }
    probabilities.iter_mut().for_each(|value| *value /= sum);
    Ok(MixedStrategy::new(probabilities)?)
}

fn column_payoffs(matrix: &[Vec<f64>], row_strategy: &[f64]) -> Vec<f64> {
    (0..matrix[0].len())
        .map(|column| {
            matrix
                .iter()
                .zip(row_strategy)
                .map(|(row, probability)| row[column] * probability)
                .sum()
        })
        .collect()
}

fn row_payoffs(matrix: &[Vec<f64>], column_strategy: &[f64]) -> Vec<f64> {
    matrix
        .iter()
        .map(|row| {
            row.iter()
                .zip(column_strategy)
                .map(|(payoff, probability)| payoff * probability)
                .sum()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_backend_errors_without_exposing_backend_types() {
        assert_eq!(
            map_lp_error(MicrolpError::Infeasible),
            SolveError::Lp(LpFailure::Infeasible)
        );
        assert_eq!(
            map_lp_error(MicrolpError::Unbounded),
            SolveError::Lp(LpFailure::Unbounded)
        );
        assert_eq!(
            map_lp_error(MicrolpError::InternalError("numeric".into())),
            SolveError::Lp(LpFailure::Internal("numeric".into()))
        );
        assert_eq!(
            map_lp_error(MicrolpError::InvalidOptions("limit".into())),
            SolveError::Lp(LpFailure::Internal("limit".into()))
        );
        assert_eq!(
            map_lp_error(MicrolpError::InvalidOperation("resume".into())),
            SolveError::Lp(LpFailure::Internal("resume".into()))
        );
    }

    #[test]
    fn accepts_only_optimal_backend_status() {
        assert_eq!(ensure_optimal_status(SolutionStatus::Optimal), Ok(()));
        assert_eq!(
            ensure_optimal_status(SolutionStatus::Feasible),
            Err(SolveError::Lp(LpFailure::NotOptimal))
        );
    }
}
