use std::cmp::Ordering;

use crate::mixed::{expected_payoff, regret, regrets};
use crate::{FiniteGame, MixedStrategy};

use super::{inspect_game, tolerance, Equilibrium, LpFailure, SolveError};

const LEXICOGRAPHIC_RELATIVE_TOLERANCE: f64 = 1e-10;

/// Configuration for [`lemke_howson`].
///
/// Labels `0..m` denote role 0's row actions and labels `m..m+n` denote role
/// 1's column actions, where the game has shape `m` by `n`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LemkeHowsonOptions {
    /// The initially dropped label.
    pub dropped_label: usize,
    /// The maximum number of complementary pivots.
    pub max_pivots: usize,
}

impl Default for LemkeHowsonOptions {
    fn default() -> Self {
        Self {
            dropped_label: 0,
            max_pivots: 100_000,
        }
    }
}

impl LemkeHowsonOptions {
    /// Sets the initially dropped label.
    pub fn with_dropped_label(mut self, dropped_label: usize) -> Self {
        self.dropped_label = dropped_label;
        self
    }

    /// Sets the maximum number of complementary pivots.
    pub fn with_max_pivots(mut self, max_pivots: usize) -> Self {
        self.max_pivots = max_pivots;
        self
    }
}

/// One verified equilibrium found along a Lemke--Howson path.
#[derive(Clone, Debug, PartialEq)]
pub struct LemkeHowsonResult {
    /// The equilibrium verified against the original, unshifted game.
    pub equilibrium: Equilibrium,
    /// The number of complementary pivots performed.
    pub pivots: usize,
}

/// Finds one equilibrium of a two-player finite game by complementary pivoting.
///
/// Before pivoting, each player's payoffs are independently shifted by a
/// constant so every entry is strictly positive. Labels `0..m` denote role
/// 0's row actions and labels `m..m+n` denote role 1's column actions. Degenerate
/// pivots use a lexicographic minimum-ratio test based on the current inverse
/// basis.
pub fn lemke_howson<G: FiniteGame>(
    game: &G,
    options: &LemkeHowsonOptions,
) -> Result<LemkeHowsonResult, SolveError> {
    let n_roles = game.n_roles();
    if n_roles != 2 {
        return Err(SolveError::UnsupportedRoles { n_roles });
    }

    let info = inspect_game(game)?;
    let comparison_tolerance = tolerance(game)?;
    debug_assert_eq!(info.tolerance, comparison_tolerance);
    let rows = info.counts[0];
    let columns = info.counts[1];
    let n_labels = rows + columns;
    if options.dropped_label >= n_labels {
        return Err(SolveError::InvalidLabel {
            label: options.dropped_label,
            n_labels,
        });
    }

    let [row_payoffs, column_payoffs] = positive_payoff_matrices(game, rows, columns)?;
    let mut row_tableau = Tableau::for_row_player(&column_payoffs, rows, columns);
    let mut column_tableau = Tableau::for_column_player(&row_payoffs, rows, columns);
    let mut in_row_tableau = options.dropped_label < rows;
    let mut entering = options.dropped_label;
    let mut pivots = 0;

    loop {
        if pivots == options.max_pivots {
            return Err(SolveError::MaxPivots {
                limit: options.max_pivots,
            });
        }
        let leaving = if in_row_tableau {
            row_tableau.pivot(entering, LEXICOGRAPHIC_RELATIVE_TOLERANCE)?
        } else {
            column_tableau.pivot(entering, LEXICOGRAPHIC_RELATIVE_TOLERANCE)?
        };
        pivots += 1;
        if leaving == options.dropped_label {
            break;
        }
        entering = leaving;
        in_row_tableau = !in_row_tableau;
    }

    let row_weights = row_tableau.values(0..rows);
    let column_weights = column_tableau.values(rows..n_labels);
    let strategies = vec![
        normalize_strategy(row_weights, comparison_tolerance)?,
        normalize_strategy(column_weights, comparison_tolerance)?,
    ];
    let payoffs = (0..2)
        .map(|role| expected_payoff(game, role, &strategies))
        .collect::<Result<Vec<_>, _>>()?;
    let per_role_regret = regrets(game, &strategies)?;
    let maximum_regret = regret(game, &strategies)?;
    if per_role_regret
        .iter()
        .any(|value| *value > comparison_tolerance)
        || maximum_regret > comparison_tolerance
    {
        return Err(SolveError::VerificationFailed {
            duality_gap: 0.0,
            regret: maximum_regret,
        });
    }

    Ok(LemkeHowsonResult {
        equilibrium: Equilibrium {
            strategies,
            payoffs,
            regret: maximum_regret,
        },
        pivots,
    })
}

fn positive_payoff_matrices<G: FiniteGame>(
    game: &G,
    rows: usize,
    columns: usize,
) -> Result<[Vec<Vec<f64>>; 2], SolveError> {
    let mut matrices = [
        vec![vec![0.0; columns]; rows],
        vec![vec![0.0; columns]; rows],
    ];
    let mut minima = [f64::INFINITY; 2];
    for (role, matrix) in matrices.iter_mut().enumerate() {
        for (row, row_payoffs) in matrix.iter_mut().enumerate() {
            for (column, stored_payoff) in row_payoffs.iter_mut().enumerate() {
                let payoff = game.payoff(role, &[row, column]);
                *stored_payoff = payoff;
                minima[role] = minima[role].min(payoff);
            }
        }
    }

    for role in 0..2 {
        let shift = if minima[role] > 0.0 {
            0.0
        } else {
            1.0 - minima[role]
        };
        for payoff in matrices[role].iter_mut().flatten() {
            *payoff += shift;
            if !payoff.is_finite() || *payoff <= 0.0 {
                return Err(SolveError::NonFinitePayoff);
            }
        }
    }
    Ok(matrices)
}

fn normalize_strategy(
    mut weights: Vec<f64>,
    comparison_tolerance: f64,
) -> Result<MixedStrategy, SolveError> {
    if weights
        .iter()
        .any(|value| !value.is_finite() || *value < -comparison_tolerance)
    {
        let residual = weights
            .iter()
            .filter(|value| value.is_finite() && **value < 0.0)
            .map(|value| -*value)
            .fold(0.0, f64::max);
        return Err(SolveError::VerificationFailed {
            duality_gap: 0.0,
            regret: residual,
        });
    }
    for weight in &mut weights {
        if *weight < 0.0 {
            *weight = 0.0;
        }
    }
    let sum = weights.iter().sum::<f64>();
    if !sum.is_finite() || sum <= 0.0 {
        return Err(SolveError::VerificationFailed {
            duality_gap: 0.0,
            regret: sum.abs(),
        });
    }
    for weight in &mut weights {
        *weight /= sum;
    }
    Ok(MixedStrategy::new(weights)?)
}

struct Tableau {
    basis: Vec<usize>,
    rhs: Vec<f64>,
    coefficients: Vec<Vec<f64>>,
    inverse_basis: Vec<Vec<f64>>,
}

impl Tableau {
    fn for_row_player(payoffs: &[Vec<f64>], rows: usize, columns: usize) -> Self {
        let n_labels = rows + columns;
        let mut coefficients = vec![vec![0.0; n_labels]; columns];
        for constraint in 0..columns {
            for action in 0..rows {
                coefficients[constraint][action] = payoffs[action][constraint];
            }
        }
        Self::initial((rows..n_labels).collect(), coefficients, columns)
    }

    fn for_column_player(payoffs: &[Vec<f64>], rows: usize, columns: usize) -> Self {
        let n_labels = rows + columns;
        let mut coefficients = vec![vec![0.0; n_labels]; rows];
        for constraint in 0..rows {
            for action in 0..columns {
                coefficients[constraint][rows + action] = payoffs[constraint][action];
            }
        }
        Self::initial((0..rows).collect(), coefficients, rows)
    }

    fn initial(basis: Vec<usize>, coefficients: Vec<Vec<f64>>, constraints: usize) -> Self {
        let mut inverse_basis = vec![vec![0.0; constraints]; constraints];
        for (index, row) in inverse_basis.iter_mut().enumerate() {
            row[index] = 1.0;
        }
        Self {
            basis,
            rhs: vec![1.0; constraints],
            coefficients,
            inverse_basis,
        }
    }

    fn pivot(&mut self, entering: usize, relative_tolerance: f64) -> Result<usize, SolveError> {
        let pivot_row = self.lexicographic_minimum_ratio(entering, relative_tolerance)?;
        let leaving = self.basis[pivot_row];
        let pivot = self.coefficients[pivot_row][entering];

        self.rhs[pivot_row] /= pivot;
        for coefficient in &mut self.coefficients[pivot_row] {
            *coefficient /= pivot;
        }
        for value in &mut self.inverse_basis[pivot_row] {
            *value /= pivot;
        }
        self.coefficients[pivot_row][entering] = 0.0;
        self.coefficients[pivot_row][leaving] = 1.0 / pivot;

        let normalized_rhs = self.rhs[pivot_row];
        let normalized_coefficients = self.coefficients[pivot_row].clone();
        let normalized_inverse = self.inverse_basis[pivot_row].clone();
        for row in 0..self.basis.len() {
            if row == pivot_row {
                continue;
            }
            let factor = self.coefficients[row][entering];
            self.rhs[row] -= factor * normalized_rhs;
            for (coefficient, pivot_coefficient) in self.coefficients[row]
                .iter_mut()
                .zip(&normalized_coefficients)
            {
                *coefficient -= factor * pivot_coefficient;
            }
            for (value, pivot_value) in self.inverse_basis[row].iter_mut().zip(&normalized_inverse)
            {
                *value -= factor * pivot_value;
            }
            self.coefficients[row][entering] = 0.0;
        }
        self.basis[pivot_row] = entering;
        Ok(leaving)
    }

    fn lexicographic_minimum_ratio(
        &self,
        entering: usize,
        relative_tolerance: f64,
    ) -> Result<usize, SolveError> {
        let mut selected = None;
        for row in 0..self.basis.len() {
            if self.coefficients[row][entering] <= 0.0 {
                continue;
            }
            selected = match selected {
                None => Some(row),
                Some(current) => {
                    match self.lexicographic_ratio_cmp(row, current, entering, relative_tolerance) {
                        Ordering::Less => Some(row),
                        Ordering::Greater => Some(current),
                        Ordering::Equal => {
                            return Err(SolveError::Lp(LpFailure::Internal(
                                "lexicographic minimum-ratio test was not unique".into(),
                            )))
                        }
                    }
                }
            };
        }
        selected.ok_or(SolveError::Lp(LpFailure::Unbounded))
    }

    fn lexicographic_ratio_cmp(
        &self,
        left: usize,
        right: usize,
        entering: usize,
        relative_tolerance: f64,
    ) -> Ordering {
        let left_denominator = self.coefficients[left][entering];
        let right_denominator = self.coefficients[right][entering];
        let left_values = std::iter::once(self.rhs[left] / left_denominator).chain(
            self.inverse_basis[left]
                .iter()
                .map(|value| value / left_denominator),
        );
        let right_values = std::iter::once(self.rhs[right] / right_denominator).chain(
            self.inverse_basis[right]
                .iter()
                .map(|value| value / right_denominator),
        );
        let pairs = left_values.zip(right_values).collect::<Vec<_>>();

        for &(left_value, right_value) in &pairs {
            let threshold = relative_tolerance * left_value.abs().max(right_value.abs()).max(1.0);
            if (left_value - right_value).abs() > threshold {
                return left_value.total_cmp(&right_value);
            }
        }
        for (left_value, right_value) in pairs {
            let ordering = left_value.total_cmp(&right_value);
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    }

    fn values(&self, labels: std::ops::Range<usize>) -> Vec<f64> {
        labels
            .map(|label| {
                self.basis
                    .iter()
                    .position(|basic| *basic == label)
                    .map_or(0.0, |row| self.rhs[row])
            })
            .collect()
    }
}
