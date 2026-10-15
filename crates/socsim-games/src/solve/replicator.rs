use microlp::{ComparisonOp, OptimizationDirection, Problem, SolutionStatus, SolveOutcome};

use crate::mixed::MixedError;
use crate::{FiniteGame, MixedStrategy};

use super::{inspect_game, solve_least_squares, SolveError};

const DEFAULT_DT: f64 = 0.01;
const DEFAULT_T_MAX: f64 = 10_000.0;
const DEFAULT_TOLERANCE: f64 = 1e-12;
const MAX_ACTIONS: usize = 20;
const MAX_CONDITION_ESTIMATE: f64 = 1e12;

/// Options for continuous-time replicator integration.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplicatorOptions {
    /// RK4 step size in model time; must be finite and positive. Default `0.01`.
    pub dt: f64,
    /// Integration horizon in model time; must be finite and non-negative.
    /// Default `10_000`.
    pub t_max: f64,
    /// Convergence threshold on the largest absolute component of the replicator
    /// velocity; must be finite and positive. Default `1e-12`. Coordinates that
    /// drop below `-tol` after a step fail the run with
    /// [`SolveError::NotConverged`].
    pub tol: f64,
}

impl ReplicatorOptions {
    /// Constructs options with `dt = 0.01`, `t_max = 10_000`, and `tol = 1e-12`.
    pub fn new() -> Self {
        Self {
            dt: DEFAULT_DT,
            t_max: DEFAULT_T_MAX,
            tol: DEFAULT_TOLERANCE,
        }
    }

    /// Sets [`dt`](Self::dt). Validated when the integration starts.
    pub fn with_dt(mut self, dt: f64) -> Self {
        self.dt = dt;
        self
    }

    /// Sets [`t_max`](Self::t_max). Validated when the integration starts.
    pub fn with_t_max(mut self, t_max: f64) -> Self {
        self.t_max = t_max;
        self
    }

    /// Sets [`tol`](Self::tol). Validated when the integration starts.
    pub fn with_tol(mut self, tol: f64) -> Self {
        self.tol = tol;
        self
    }
}

impl Default for ReplicatorOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// The final state of a replicator integration.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplicatorRun {
    /// Population shares at the end of the integration, renormalized to sum to 1.
    pub state: MixedStrategy,
    /// Model time reached; at most [`ReplicatorOptions::t_max`].
    pub t: f64,
    /// Number of RK4 steps taken.
    pub steps: u64,
    /// `true` when the velocity fell below [`ReplicatorOptions::tol`]; `false`
    /// when `t_max` was reached first.
    pub converged: bool,
}

/// An isolated replicator fixed point and its exact support.
#[derive(Clone, Debug, PartialEq)]
pub struct FixedPoint {
    /// Actions with positive share, in ascending order.
    pub support: Vec<usize>,
    /// The fixed point; zero outside `support`.
    pub point: MixedStrategy,
}

/// A positive-dimensional affine family of replicator fixed points.
#[derive(Clone, Debug, PartialEq)]
pub struct AffineComponent {
    /// Actions on which the family is defined, in ascending order.
    pub support: Vec<usize>,
    /// One point of the family, inside the simplex face spanned by `support`.
    pub base: MixedStrategy,
    /// Basis of the family's directions, one full-length vector per dimension.
    ///
    /// Each entry is zero outside `support` and sums to zero, so `base + Σ cᵢ·dᵢ`
    /// stays a fixed point while it remains in the simplex.
    pub directions: Vec<Vec<f64>>,
}

/// The fixed points found on all faces of the strategy simplex.
#[derive(Clone, Debug, PartialEq)]
pub struct FixedPointSet {
    /// Fixed points that are unique on their support and do not lie in any
    /// of [`components`](Self::components).
    pub isolated: Vec<FixedPoint>,
    /// Positive-dimensional families of fixed points. A family is omitted when it
    /// lies within a larger one.
    pub components: Vec<AffineComponent>,
    /// Supports, as sorted action lists, whose fixed points could not be decided
    /// numerically (for example an ill-conditioned indifference system).
    pub inconclusive: Vec<Vec<usize>>,
}

/// Integrates the continuous-time replicator equation using classical RK4.
pub fn replicator_integrate<G: FiniteGame>(
    game: &G,
    x0: &MixedStrategy,
    options: &ReplicatorOptions,
) -> Result<ReplicatorRun, SolveError> {
    validate_options(options)?;
    let (payoffs, _) = symmetric_payoff_matrix(game)?;
    validate_strategy_dimension(x0, payoffs.len())?;

    let mut state = x0.probs().to_vec();
    let mut time = 0.0;
    let mut steps = 0_u64;
    loop {
        let velocity = replicator_derivative(&payoffs, &state);
        let residual = infinity_norm(&velocity);
        if residual < options.tol {
            return Ok(ReplicatorRun {
                state: MixedStrategy::new(state)?,
                t: time,
                steps,
                converged: true,
            });
        }
        if time >= options.t_max {
            return Ok(ReplicatorRun {
                state: MixedStrategy::new(state)?,
                t: time,
                steps,
                converged: false,
            });
        }

        let step = options.dt.min(options.t_max - time);
        if step <= 0.0 || time + step <= time {
            return Err(SolveError::InvalidOptions(
                "dt is too small to advance integration time".into(),
            ));
        }
        state = rk4_step(&payoffs, &state, step);
        steps = steps.saturating_add(1);
        time += step;
        normalize_after_step(&mut state, options.tol, residual, steps)?;
    }
}

/// Enumerates isolated and positive-dimensional replicator fixed points.
pub fn replicator_fixed_points<G: FiniteGame>(game: &G) -> Result<FixedPointSet, SolveError> {
    let (payoffs, comparison_tolerance) = symmetric_payoff_matrix(game)?;
    let action_count = payoffs.len();
    if action_count > MAX_ACTIONS {
        return Err(SolveError::TooLarge {
            profiles: support_count(action_count),
        });
    }

    let mut result = FixedPointSet {
        isolated: Vec::new(),
        components: Vec::new(),
        inconclusive: Vec::new(),
    };
    for support in supports_by_size(action_count) {
        examine_support(&payoffs, &support, comparison_tolerance, &mut result);
    }
    remove_component_subsets(&payoffs, comparison_tolerance, &mut result);
    Ok(result)
}

pub(crate) fn symmetric_payoff_matrix<G: FiniteGame>(
    game: &G,
) -> Result<(Vec<Vec<f64>>, f64), SolveError> {
    let n_roles = game.n_roles();
    if n_roles != 2 {
        return Err(SolveError::UnsupportedRoles { n_roles });
    }
    let info = inspect_game(game)?;
    if !game.is_symmetric() {
        return Err(SolveError::InvalidOptions(
            "replicator and ESS solvers require a symmetric game".into(),
        ));
    }
    if info.counts[0] != info.counts[1] {
        return Err(SolveError::InvalidOptions(
            "a symmetric game must have the same actions in both roles".into(),
        ));
    }
    let payoffs = (0..info.counts[0])
        .map(|row| {
            (0..info.counts[1])
                .map(|column| game.payoff(0, &[row, column]))
                .collect()
        })
        .collect();
    Ok((payoffs, info.tolerance))
}

fn validate_options(options: &ReplicatorOptions) -> Result<(), SolveError> {
    if !options.dt.is_finite() || options.dt <= 0.0 {
        return Err(SolveError::InvalidOptions(
            "dt must be finite and positive".into(),
        ));
    }
    if !options.t_max.is_finite() || options.t_max < 0.0 {
        return Err(SolveError::InvalidOptions(
            "t_max must be finite and non-negative".into(),
        ));
    }
    if !options.tol.is_finite() || options.tol <= 0.0 {
        return Err(SolveError::InvalidOptions(
            "tol must be finite and positive".into(),
        ));
    }
    Ok(())
}

fn validate_strategy_dimension(
    strategy: &MixedStrategy,
    expected: usize,
) -> Result<(), SolveError> {
    if strategy.len() != expected {
        return Err(MixedError::Dimension {
            role: 0,
            expected,
            actual: strategy.len(),
        }
        .into());
    }
    Ok(())
}

pub(crate) fn replicator_derivative(payoffs: &[Vec<f64>], state: &[f64]) -> Vec<f64> {
    let fitness = payoffs
        .iter()
        .map(|row| row.iter().zip(state).map(|(a, x)| a * x).sum::<f64>())
        .collect::<Vec<_>>();
    let mean_fitness = state
        .iter()
        .zip(&fitness)
        .map(|(x, value)| x * value)
        .sum::<f64>();
    state
        .iter()
        .zip(fitness)
        .map(|(x, value)| x * (value - mean_fitness))
        .collect()
}

fn rk4_step(payoffs: &[Vec<f64>], state: &[f64], step: f64) -> Vec<f64> {
    let k1 = replicator_derivative(payoffs, state);
    let k2_state = offset(state, &k1, step / 2.0);
    let k2 = replicator_derivative(payoffs, &k2_state);
    let k3_state = offset(state, &k2, step / 2.0);
    let k3 = replicator_derivative(payoffs, &k3_state);
    let k4_state = offset(state, &k3, step);
    let k4 = replicator_derivative(payoffs, &k4_state);
    state
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value + step * (k1[index] + 2.0 * k2[index] + 2.0 * k3[index] + k4[index]) / 6.0
        })
        .collect()
}

fn offset(state: &[f64], velocity: &[f64], scale: f64) -> Vec<f64> {
    state
        .iter()
        .zip(velocity)
        .map(|(value, change)| value + scale * change)
        .collect()
}

fn normalize_after_step(
    state: &mut [f64],
    tolerance: f64,
    residual: f64,
    steps: u64,
) -> Result<(), SolveError> {
    if state.iter().any(|value| !value.is_finite()) {
        return Err(not_converged(residual, steps));
    }
    for value in state.iter_mut() {
        if *value < -tolerance {
            return Err(not_converged(residual.max(-*value), steps));
        }
        if *value < 0.0 {
            *value = 0.0;
        }
    }
    let sum = state.iter().sum::<f64>();
    if !sum.is_finite() || sum <= 0.0 {
        return Err(not_converged(residual, steps));
    }
    state.iter_mut().for_each(|value| *value /= sum);
    Ok(())
}

fn not_converged(residual: f64, steps: u64) -> SolveError {
    SolveError::NotConverged {
        residual,
        iterations: usize::try_from(steps).unwrap_or(usize::MAX),
    }
}

fn infinity_norm(values: &[f64]) -> f64 {
    values.iter().map(|value| value.abs()).fold(0.0, f64::max)
}

fn examine_support(
    payoffs: &[Vec<f64>],
    support: &[usize],
    tolerance: f64,
    result: &mut FixedPointSet,
) {
    let (matrix, rhs) = indifference_system(payoffs, support);
    let solved = solve_least_squares(&matrix, &rhs);
    if !solved.residual.is_finite()
        || solved.solution.iter().any(|value| !value.is_finite())
        || solved
            .nullspace
            .iter()
            .flatten()
            .any(|value| !value.is_finite())
        || (solved.rank > 0 && solved.condition_estimate > MAX_CONDITION_ESTIMATE)
    {
        result.inconclusive.push(support.to_vec());
        return;
    }
    if solved.residual > tolerance {
        if solved.rank < support.len() {
            result.inconclusive.push(support.to_vec());
        }
        return;
    }

    if solved.rank == support.len() {
        if solved
            .solution
            .iter()
            .all(|probability| *probability > tolerance)
        {
            if let Some(point) =
                strategy_on_support(payoffs.len(), support, &solved.solution, tolerance)
            {
                result.isolated.push(FixedPoint {
                    support: support.to_vec(),
                    point,
                });
            } else {
                result.inconclusive.push(support.to_vec());
            }
        }
        return;
    }

    let interior = match relative_interior_point(&matrix, &rhs, tolerance) {
        Feasibility::Point(point) => point,
        Feasibility::Empty => return,
        Feasibility::Inconclusive => {
            result.inconclusive.push(support.to_vec());
            return;
        }
    };
    let base_values = strategy_on_support(payoffs.len(), support, &solved.solution, tolerance)
        .or_else(|| strategy_on_support(payoffs.len(), support, &interior, tolerance));
    let Some(base) = base_values else {
        result.inconclusive.push(support.to_vec());
        return;
    };
    let directions = solved
        .nullspace
        .iter()
        .map(|direction| {
            let mut full = vec![0.0; payoffs.len()];
            for (&action, &value) in support.iter().zip(direction) {
                full[action] = value;
            }
            full
        })
        .filter(|direction| direction.iter().any(|value| *value != 0.0))
        .collect::<Vec<_>>();
    if directions.is_empty() {
        result.inconclusive.push(support.to_vec());
    } else {
        result.components.push(AffineComponent {
            support: support.to_vec(),
            base,
            directions,
        });
    }
}

fn indifference_system(payoffs: &[Vec<f64>], support: &[usize]) -> (Vec<Vec<f64>>, Vec<f64>) {
    let reference = support[0];
    let mut matrix = support
        .iter()
        .skip(1)
        .map(|&action| {
            support
                .iter()
                .map(|&opponent| payoffs[action][opponent] - payoffs[reference][opponent])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    matrix.push(vec![1.0; support.len()]);
    let mut rhs = vec![0.0; support.len()];
    rhs[support.len() - 1] = 1.0;
    (matrix, rhs)
}

fn strategy_on_support(
    action_count: usize,
    support: &[usize],
    values: &[f64],
    tolerance: f64,
) -> Option<MixedStrategy> {
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < -tolerance)
    {
        return None;
    }
    let mut normalized = values
        .iter()
        .map(|value| value.max(0.0))
        .collect::<Vec<_>>();
    let sum = normalized.iter().sum::<f64>();
    if !sum.is_finite() || sum <= 0.0 {
        return None;
    }
    normalized.iter_mut().for_each(|value| *value /= sum);
    let mut full = vec![0.0; action_count];
    for (&action, probability) in support.iter().zip(normalized) {
        full[action] = probability;
    }
    MixedStrategy::new(full).ok()
}

enum Feasibility {
    Point(Vec<f64>),
    Empty,
    Inconclusive,
}

fn relative_interior_point(matrix: &[Vec<f64>], rhs: &[f64], tolerance: f64) -> Feasibility {
    let columns = matrix.first().map_or(0, Vec::len);
    let mut problem = Problem::new(OptimizationDirection::Maximize);
    let probabilities = (0..columns)
        .map(|_| problem.add_var(0.0, (0.0, 1.0)))
        .collect::<Vec<_>>();
    let margin = problem.add_var(1.0, (0.0, 1.0));
    for (row, &target) in matrix.iter().zip(rhs) {
        problem.add_constraint(
            probabilities
                .iter()
                .zip(row)
                .map(|(&variable, &coefficient)| (variable, coefficient)),
            ComparisonOp::Eq,
            target,
        );
    }
    for &probability in &probabilities {
        problem.add_constraint([(probability, 1.0), (margin, -1.0)], ComparisonOp::Ge, 0.0);
    }

    match problem.solve() {
        Err(microlp::Error::Infeasible) => Feasibility::Empty,
        Err(_) | Ok(SolveOutcome::Interrupted(_)) => Feasibility::Inconclusive,
        Ok(SolveOutcome::Solution(solution)) => {
            if solution.status() != SolutionStatus::Optimal {
                return Feasibility::Inconclusive;
            }
            if solution.var_value(margin) <= tolerance {
                return Feasibility::Empty;
            }
            Feasibility::Point(
                probabilities
                    .iter()
                    .map(|&variable| solution.var_value(variable))
                    .collect(),
            )
        }
    }
}

fn supports_by_size(action_count: usize) -> Vec<Vec<usize>> {
    fn combinations(
        next: usize,
        remaining: usize,
        action_count: usize,
        prefix: &mut Vec<usize>,
        result: &mut Vec<Vec<usize>>,
    ) {
        if remaining == 0 {
            result.push(prefix.clone());
            return;
        }
        for action in next..=action_count - remaining {
            prefix.push(action);
            combinations(action + 1, remaining - 1, action_count, prefix, result);
            prefix.pop();
        }
    }

    let mut result = Vec::new();
    for size in 1..=action_count {
        combinations(0, size, action_count, &mut Vec::new(), &mut result);
    }
    result
}

fn support_count(actions: usize) -> u128 {
    u32::try_from(actions)
        .ok()
        .and_then(|shift| 1_u128.checked_shl(shift))
        .and_then(|value| value.checked_sub(1))
        .unwrap_or(u128::MAX)
}

fn remove_component_subsets(payoffs: &[Vec<f64>], tolerance: f64, result: &mut FixedPointSet) {
    let components = result.components.clone();
    result.components = components
        .iter()
        .enumerate()
        .filter(|(candidate_index, candidate)| {
            !components
                .iter()
                .enumerate()
                .any(|(container_index, container)| {
                    candidate_index != &container_index
                        && component_contains_component(payoffs, container, candidate, tolerance)
                })
        })
        .map(|(_, component)| component.clone())
        .collect();
    result.isolated.retain(|fixed| {
        !result.components.iter().any(|component| {
            component_contains_vector(payoffs, component, fixed.point.probs(), false, tolerance)
        })
    });
}

fn component_contains_component(
    payoffs: &[Vec<f64>],
    container: &AffineComponent,
    candidate: &AffineComponent,
    tolerance: f64,
) -> bool {
    candidate.support.len() < container.support.len()
        && candidate
            .support
            .iter()
            .all(|action| container.support.contains(action))
        && component_contains_vector(payoffs, container, candidate.base.probs(), false, tolerance)
        && candidate.directions.iter().all(|direction| {
            component_contains_vector(payoffs, container, direction, true, tolerance)
        })
}

fn component_contains_vector(
    payoffs: &[Vec<f64>],
    component: &AffineComponent,
    vector: &[f64],
    is_direction: bool,
    tolerance: f64,
) -> bool {
    if vector
        .iter()
        .enumerate()
        .any(|(action, value)| !component.support.contains(&action) && value.abs() > tolerance)
    {
        return false;
    }
    let expected_sum = if is_direction { 0.0 } else { 1.0 };
    if (vector.iter().sum::<f64>() - expected_sum).abs() > tolerance {
        return false;
    }
    let reference = component.support[0];
    component.support.iter().skip(1).all(|&action| {
        payoffs[action]
            .iter()
            .zip(&payoffs[reference])
            .zip(vector)
            .map(|((left, right), value)| (left - right) * value)
            .sum::<f64>()
            .abs()
            <= tolerance
    })
}
