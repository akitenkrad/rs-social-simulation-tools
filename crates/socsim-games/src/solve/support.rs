use crate::mixed::{action_payoffs, expected_payoff, regret, regrets};
use crate::{FiniteGame, MixedStrategy};

use super::{
    advance_profile, inspect_game, solve_least_squares, tolerance, Equilibrium,
    LeastSquaresSolution, SolveError,
};

const MAX_SUPPORT_PAIRS: u128 = 1 << 20;
const MAX_CONDITION_ESTIMATE: f64 = 1e12;

/// The verified equilibria and numerical findings from support enumeration.
#[derive(Clone, Debug, PartialEq)]
pub struct EquilibriumSet {
    /// Verified equilibrium points, deduplicated within comparison tolerance.
    pub equilibria: Vec<Equilibrium>,
    /// Degenerate or numerically inconclusive supports encountered by the search.
    pub findings: Vec<SupportFinding>,
}

impl EquilibriumSet {
    /// Returns true when no degeneracy or numerical findings were recorded.
    pub fn is_complete(&self) -> bool {
        self.findings.is_empty()
    }
}

/// A support for which numerical support enumeration was not uniquely decisive.
#[derive(Clone, Debug, PartialEq)]
pub enum SupportFinding {
    /// A rank-deficient support whose equilibrium set could not be verified.
    Degenerate {
        /// Ascending action indices in the support of each role.
        support: Vec<Vec<usize>>,
        /// Numerical reason a unique equilibrium could not be certified.
        reason: String,
    },
    /// A verified point on a positive-dimensional equilibrium component.
    ///
    /// Each direction is flattened as all role-0 probabilities followed by all
    /// role-1 probabilities. Payoff-variable coordinates are omitted.
    Component {
        /// Ascending action indices in the support of each role.
        support: Vec<Vec<usize>>,
        /// Verified point lying on the component.
        base: Equilibrium,
        /// Basis directions in flattened role-0 then role-1 probability order.
        directions: Vec<Vec<f64>>,
    },
    /// A support whose conditioning or final verification was inconclusive.
    Inconclusive {
        /// Ascending action indices in the support of each role.
        support: Vec<Vec<usize>>,
        /// Conditioning or verification failure that prevented certification.
        reason: String,
    },
}

/// Enumerates every non-empty support pair of a two-player finite game.
///
/// Supports contain ascending action indices. They are ordered by ordinary
/// lexicographic `Vec` order, with role 0 as the outer key and role 1 as the
/// inner key. Verified duplicate equilibria retain the first occurrence.
pub fn support_enumeration<G: FiniteGame>(game: &G) -> Result<EquilibriumSet, SolveError> {
    let n_roles = game.n_roles();
    if n_roles != 2 {
        return Err(SolveError::UnsupportedRoles { n_roles });
    }
    let counts = [game.n_actions(0), game.n_actions(1)];
    let support_pairs = support_pair_count(counts[0], counts[1]);
    if support_pairs > MAX_SUPPORT_PAIRS {
        return Err(SolveError::TooLarge {
            profiles: support_pairs,
        });
    }
    let info = inspect_game(game)?;
    let comparison_tolerance = tolerance(game)?;
    debug_assert_eq!(info.tolerance, comparison_tolerance);

    let payoffs = payoff_matrices(game, &info.counts);
    let row_supports = non_empty_supports(info.counts[0]);
    let column_supports = non_empty_supports(info.counts[1]);
    let mut result = EquilibriumSet {
        equilibria: Vec::new(),
        findings: Vec::new(),
    };

    for rows in &row_supports {
        for columns in &column_supports {
            examine_support(
                game,
                &payoffs,
                rows,
                columns,
                comparison_tolerance,
                &mut result,
            )?;
        }
    }
    Ok(result)
}

fn examine_support<G: FiniteGame>(
    game: &G,
    payoffs: &[Vec<Vec<f64>>; 2],
    rows: &[usize],
    columns: &[usize],
    comparison_tolerance: f64,
    result: &mut EquilibriumSet,
) -> Result<(), SolveError> {
    let role_zero_system = role_zero_indifference(payoffs, rows, columns);
    let role_one_system = role_one_indifference(payoffs, rows, columns);
    let role_zero_solution = solve_least_squares(&role_zero_system.0, &role_zero_system.1);
    let role_one_solution = solve_least_squares(&role_one_system.0, &role_one_system.1);

    let role_zero_excludes = unique_solution_excludes(
        payoffs,
        0,
        rows,
        columns,
        &role_zero_solution,
        comparison_tolerance,
    );
    let role_one_excludes = unique_solution_excludes(
        payoffs,
        1,
        columns,
        rows,
        &role_one_solution,
        comparison_tolerance,
    );
    if role_zero_excludes || role_one_excludes {
        return Ok(());
    }

    let support = vec![rows.to_vec(), columns.to_vec()];
    if unreliable(&role_zero_solution) || unreliable(&role_one_solution) {
        result.findings.push(SupportFinding::Inconclusive {
            support,
            reason: "the indifference system is ill-conditioned or non-finite".into(),
        });
        return Ok(());
    }

    let role_zero_full = role_zero_solution.rank == role_zero_solution.solution.len();
    let role_one_full = role_one_solution.rank == role_one_solution.solution.len();
    if role_zero_full && role_one_full {
        match verified_equilibrium(
            game,
            rows,
            columns,
            &role_zero_solution,
            &role_one_solution,
            comparison_tolerance,
        )? {
            Ok(equilibrium) => {
                push_unique(&mut result.equilibria, equilibrium, comparison_tolerance)
            }
            Err(reason) => result
                .findings
                .push(SupportFinding::Inconclusive { support, reason }),
        }
        return Ok(());
    }

    if role_zero_solution.residual > comparison_tolerance
        || role_one_solution.residual > comparison_tolerance
    {
        result.findings.push(SupportFinding::Degenerate {
            support,
            reason: "a rank-deficient indifference system has excessive residual".into(),
        });
        return Ok(());
    }

    match verified_equilibrium(
        game,
        rows,
        columns,
        &role_zero_solution,
        &role_one_solution,
        comparison_tolerance,
    )? {
        Ok(base) => {
            let directions = component_directions(
                game.n_actions(0),
                game.n_actions(1),
                rows,
                columns,
                &role_zero_solution,
                &role_one_solution,
            );
            push_unique(&mut result.equilibria, base.clone(), comparison_tolerance);
            result.findings.push(SupportFinding::Component {
                support,
                base,
                directions,
            });
        }
        Err(reason) => result
            .findings
            .push(SupportFinding::Degenerate { support, reason }),
    }
    Ok(())
}

fn role_zero_indifference(
    payoffs: &[Vec<Vec<f64>>; 2],
    rows: &[usize],
    columns: &[usize],
) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut matrix = Vec::with_capacity(rows.len() + 1);
    let mut rhs = vec![0.0; rows.len() + 1];
    for &row in rows {
        let mut equation = columns
            .iter()
            .map(|&column| payoffs[0][row][column])
            .collect::<Vec<_>>();
        equation.push(-1.0);
        matrix.push(equation);
    }
    let mut normalization = vec![1.0; columns.len()];
    normalization.push(0.0);
    matrix.push(normalization);
    rhs[rows.len()] = 1.0;
    (matrix, rhs)
}

fn role_one_indifference(
    payoffs: &[Vec<Vec<f64>>; 2],
    rows: &[usize],
    columns: &[usize],
) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut matrix = Vec::with_capacity(columns.len() + 1);
    let mut rhs = vec![0.0; columns.len() + 1];
    for &column in columns {
        let mut equation = rows
            .iter()
            .map(|&row| payoffs[1][row][column])
            .collect::<Vec<_>>();
        equation.push(-1.0);
        matrix.push(equation);
    }
    let mut normalization = vec![1.0; rows.len()];
    normalization.push(0.0);
    matrix.push(normalization);
    rhs[columns.len()] = 1.0;
    (matrix, rhs)
}

fn unique_solution_excludes(
    payoffs: &[Vec<Vec<f64>>; 2],
    role: usize,
    own_support: &[usize],
    opponent_support: &[usize],
    solved: &LeastSquaresSolution,
    comparison_tolerance: f64,
) -> bool {
    if unreliable(solved) {
        return false;
    }
    if solved.residual > comparison_tolerance {
        return true;
    }
    if solved.rank != solved.solution.len() {
        return false;
    }
    if solved.solution[..opponent_support.len()]
        .iter()
        .any(|&probability| probability < comparison_tolerance)
    {
        return true;
    }

    let utility = solved.solution[opponent_support.len()];
    let action_count = if role == 0 {
        payoffs[0].len()
    } else {
        payoffs[1][0].len()
    };
    (0..action_count)
        .filter(|action| !own_support.contains(action))
        .any(|action| {
            let payoff = opponent_support
                .iter()
                .enumerate()
                .map(|(offset, &opponent)| {
                    let profile_payoff = if role == 0 {
                        payoffs[0][action][opponent]
                    } else {
                        payoffs[1][opponent][action]
                    };
                    solved.solution[offset] * profile_payoff
                })
                .sum::<f64>();
            payoff - utility > comparison_tolerance
        })
}

fn unreliable(solved: &LeastSquaresSolution) -> bool {
    !solved.condition_estimate.is_finite()
        || solved.condition_estimate > MAX_CONDITION_ESTIMATE
        || !solved.residual.is_finite()
        || solved.solution.iter().any(|value| !value.is_finite())
        || solved
            .nullspace
            .iter()
            .flatten()
            .any(|value| !value.is_finite())
}

fn verified_equilibrium<G: FiniteGame>(
    game: &G,
    rows: &[usize],
    columns: &[usize],
    role_zero_solution: &LeastSquaresSolution,
    role_one_solution: &LeastSquaresSolution,
    comparison_tolerance: f64,
) -> Result<Result<Equilibrium, String>, SolveError> {
    let Some(role_zero) = strategy_from_solution(
        game.n_actions(0),
        rows,
        &role_one_solution.solution[..rows.len()],
        comparison_tolerance,
    ) else {
        return Ok(Err(
            "the base point is not a non-negative distribution".into()
        ));
    };
    let Some(role_one) = strategy_from_solution(
        game.n_actions(1),
        columns,
        &role_zero_solution.solution[..columns.len()],
        comparison_tolerance,
    ) else {
        return Ok(Err(
            "the base point is not a non-negative distribution".into()
        ));
    };
    let strategies = vec![role_zero, role_one];
    let payoffs = (0..2)
        .map(|role| expected_payoff(game, role, &strategies))
        .collect::<Result<Vec<_>, _>>()?;
    let per_role_regret = regrets(game, &strategies)?;
    let maximum_regret = regret(game, &strategies)?;

    for role in 0..2 {
        let values = action_payoffs(game, role, &strategies)?;
        let best = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if values
            .iter()
            .zip(strategies[role].probs())
            .any(|(value, probability)| *probability > 0.0 && best - value > comparison_tolerance)
        {
            return Ok(Err(
                "the solved support fails the pure-action payoff check".into()
            ));
        }
    }
    if per_role_regret
        .iter()
        .any(|value| *value > comparison_tolerance)
        || maximum_regret > comparison_tolerance
    {
        return Ok(Err(format!(
            "final regret {maximum_regret} exceeds tolerance {comparison_tolerance}"
        )));
    }

    Ok(Ok(Equilibrium {
        strategies,
        payoffs,
        regret: maximum_regret,
    }))
}

fn strategy_from_solution(
    action_count: usize,
    support: &[usize],
    probabilities: &[f64],
    comparison_tolerance: f64,
) -> Option<MixedStrategy> {
    if probabilities
        .iter()
        .any(|probability| !probability.is_finite() || *probability < -comparison_tolerance)
    {
        return None;
    }
    let mut normalized = probabilities
        .iter()
        .map(|probability| probability.max(0.0))
        .collect::<Vec<_>>();
    let sum = normalized.iter().sum::<f64>();
    if !sum.is_finite() || sum <= 0.0 {
        return None;
    }
    for probability in &mut normalized {
        *probability /= sum;
    }
    let mut full = vec![0.0; action_count];
    for (&action, probability) in support.iter().zip(normalized) {
        full[action] = probability;
    }
    MixedStrategy::new(full).ok()
}

fn component_directions(
    row_count: usize,
    column_count: usize,
    rows: &[usize],
    columns: &[usize],
    role_zero_solution: &LeastSquaresSolution,
    role_one_solution: &LeastSquaresSolution,
) -> Vec<Vec<f64>> {
    let mut result = Vec::new();
    for null_vector in &role_one_solution.nullspace {
        let mut direction = vec![0.0; row_count + column_count];
        for (&action, &value) in rows.iter().zip(null_vector) {
            direction[action] = value;
        }
        if direction.iter().any(|value| *value != 0.0) {
            result.push(direction);
        }
    }
    for null_vector in &role_zero_solution.nullspace {
        let mut direction = vec![0.0; row_count + column_count];
        for (&action, &value) in columns.iter().zip(null_vector) {
            direction[row_count + action] = value;
        }
        if direction.iter().any(|value| *value != 0.0) {
            result.push(direction);
        }
    }
    result
}

fn push_unique(
    equilibria: &mut Vec<Equilibrium>,
    candidate: Equilibrium,
    comparison_tolerance: f64,
) {
    let duplicate = equilibria.iter().any(|existing| {
        existing.strategies.len() == candidate.strategies.len()
            && existing
                .strategies
                .iter()
                .zip(&candidate.strategies)
                .all(|(left, right)| {
                    left.len() == right.len()
                        && left
                            .probs()
                            .iter()
                            .zip(right.probs())
                            .all(|(a, b)| (a - b).abs() <= comparison_tolerance)
                })
    });
    if !duplicate {
        equilibria.push(candidate);
    }
}

fn payoff_matrices<G: FiniteGame>(game: &G, counts: &[usize]) -> [Vec<Vec<f64>>; 2] {
    let mut result = [
        vec![vec![0.0; counts[1]]; counts[0]],
        vec![vec![0.0; counts[1]]; counts[0]],
    ];
    let mut profile = vec![0; 2];
    loop {
        for (role, matrix) in result.iter_mut().enumerate() {
            matrix[profile[0]][profile[1]] = game.payoff(role, &profile);
        }
        if !advance_profile(&mut profile, counts) {
            break;
        }
    }
    result
}

fn non_empty_supports(action_count: usize) -> Vec<Vec<usize>> {
    fn extend(
        next: usize,
        action_count: usize,
        prefix: &mut Vec<usize>,
        result: &mut Vec<Vec<usize>>,
    ) {
        for action in next..action_count {
            prefix.push(action);
            result.push(prefix.clone());
            extend(action + 1, action_count, prefix, result);
            prefix.pop();
        }
    }

    let mut result = Vec::new();
    extend(0, action_count, &mut Vec::new(), &mut result);
    result
}

fn support_pair_count(rows: usize, columns: usize) -> u128 {
    fn support_count(actions: usize) -> u128 {
        u32::try_from(actions)
            .ok()
            .and_then(|shift| 1_u128.checked_shl(shift))
            .and_then(|value| value.checked_sub(1))
            .unwrap_or(u128::MAX)
    }

    support_count(rows).saturating_mul(support_count(columns))
}

#[cfg(test)]
mod tests {
    use super::{non_empty_supports, support_pair_count, MAX_SUPPORT_PAIRS};

    #[test]
    fn supports_are_in_vector_lexicographic_order() {
        assert_eq!(
            non_empty_supports(3),
            vec![
                vec![0],
                vec![0, 1],
                vec![0, 1, 2],
                vec![0, 2],
                vec![1],
                vec![1, 2],
                vec![2]
            ]
        );
    }

    #[test]
    fn support_pair_limit_accepts_ten_by_ten_and_rejects_eleven_by_ten() {
        assert_eq!(support_pair_count(10, 10), 1_046_529);
        assert!(support_pair_count(10, 10) <= MAX_SUPPORT_PAIRS);
        assert_eq!(support_pair_count(11, 10), 2_094_081);
        assert!(support_pair_count(11, 10) > MAX_SUPPORT_PAIRS);
        assert_eq!(support_pair_count(usize::MAX, 10), u128::MAX);
    }
}
