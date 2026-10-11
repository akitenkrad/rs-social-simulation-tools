use crate::mixed::MixedError;
use crate::{FiniteGame, MixedStrategy};

use super::linalg::symmetric_eigenvalues;
use super::replicator::symmetric_payoff_matrix;
use super::SolveError;

/// A tolerance-aware evolutionary-stability decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EssVerdict {
    /// The strategy is an ESS: no best-response mutant can invade.
    True,
    /// The strategy is not an ESS: it is not a best response to itself, or some
    /// alternative best response is not driven out (it does at least as well
    /// against itself as the resident does against it).
    False,
    /// The test cannot decide within the payoff-scaled tolerance, for example
    /// when a second-order condition is zero or the support has more than three
    /// alternative best responses.
    Inconclusive,
}

/// Tests a symmetric mixed strategy for evolutionary stability.
pub fn ess_verdict<G: FiniteGame>(
    game: &G,
    resident: &MixedStrategy,
) -> Result<EssVerdict, SolveError> {
    let (payoffs, tolerance) = symmetric_payoff_matrix(game)?;
    if resident.len() != payoffs.len() {
        return Err(MixedError::Dimension {
            role: 0,
            expected: payoffs.len(),
            actual: resident.len(),
        }
        .into());
    }

    let action_values = matrix_vector(&payoffs, resident.probs());
    let best = action_values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let resident_payoff = resident
        .probs()
        .iter()
        .zip(&action_values)
        .map(|(probability, payoff)| probability * payoff)
        .sum::<f64>();
    if best - resident_payoff > tolerance {
        return Ok(EssVerdict::False);
    }

    let best_responses = action_values
        .iter()
        .enumerate()
        .filter_map(|(action, payoff)| (best - payoff <= tolerance).then_some(action))
        .collect::<Vec<_>>();
    let support = resident
        .probs()
        .iter()
        .enumerate()
        .filter_map(|(action, probability)| (*probability > 0.0).then_some(action))
        .collect::<Vec<_>>();
    if !support.iter().all(|action| best_responses.contains(action)) {
        return Ok(EssVerdict::Inconclusive);
    }

    if best_responses == support {
        return Ok(quadratic_space_verdict(
            &payoffs,
            &best_responses,
            tolerance,
        ));
    }
    if best_responses.len() > 3 {
        return Ok(EssVerdict::Inconclusive);
    }

    Ok(cone_verdict(&payoffs, &best_responses, &support, tolerance))
}

fn cone_verdict(
    payoffs: &[Vec<f64>],
    best_responses: &[usize],
    support: &[usize],
    tolerance: f64,
) -> EssVerdict {
    match (best_responses.len(), support.len()) {
        (2, 1) => {
            let resident = support[0];
            let mutant = best_responses
                .iter()
                .copied()
                .find(|action| *action != resident)
                .expect("a strict superset has an outside action");
            let direction = unit_difference(payoffs.len(), mutant, resident);
            scalar_sign_verdict(quadratic_form(payoffs, &direction), tolerance)
        }
        (3, 1) => {
            let resident = support[0];
            let outsiders = best_responses
                .iter()
                .copied()
                .filter(|action| *action != resident)
                .collect::<Vec<_>>();
            let first = unit_difference(payoffs.len(), outsiders[0], resident);
            let second = unit_difference(payoffs.len(), outsiders[1], resident);
            let q11 = quadratic_form(payoffs, &first);
            let q22 = quadratic_form(payoffs, &second);
            if q11 > tolerance || q22 > tolerance {
                return EssVerdict::False;
            }
            if q11 >= -tolerance || q22 >= -tolerance {
                return EssVerdict::Inconclusive;
            }
            let q12 = symmetric_bilinear_form(payoffs, &first, &second);
            let boundary = (q11 * q22).sqrt();
            if q12 < boundary {
                EssVerdict::True
            } else {
                EssVerdict::False
            }
        }
        (3, 2) => quadratic_space_verdict(payoffs, best_responses, tolerance),
        _ => EssVerdict::Inconclusive,
    }
}

fn quadratic_space_verdict(payoffs: &[Vec<f64>], actions: &[usize], tolerance: f64) -> EssVerdict {
    if actions.len() == 1 {
        return EssVerdict::True;
    }
    let basis = tangent_basis(payoffs.len(), actions);
    let projected = project_symmetric_part(payoffs, &basis);
    if projected
        .iter()
        .flatten()
        .all(|entry| entry.abs() <= tolerance)
    {
        return EssVerdict::False;
    }
    let jacobi_tolerance = (tolerance * 1e-3).max(f64::EPSILON);
    let maximum = symmetric_eigenvalues(&projected, jacobi_tolerance)
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);
    if maximum < -tolerance {
        EssVerdict::True
    } else if maximum > tolerance {
        EssVerdict::False
    } else {
        EssVerdict::Inconclusive
    }
}

fn scalar_sign_verdict(value: f64, tolerance: f64) -> EssVerdict {
    if value < -tolerance {
        EssVerdict::True
    } else if value > tolerance {
        EssVerdict::False
    } else {
        EssVerdict::Inconclusive
    }
}

fn tangent_basis(dimension: usize, actions: &[usize]) -> Vec<Vec<f64>> {
    let anchor = *actions.last().expect("an action set is non-empty");
    let mut basis: Vec<Vec<f64>> = Vec::with_capacity(actions.len() - 1);
    for &action in &actions[..actions.len() - 1] {
        let mut vector = unit_difference(dimension, action, anchor);
        for previous in &basis {
            let projection = dot(&vector, previous);
            for (value, basis_value) in vector.iter_mut().zip(previous) {
                *value -= projection * basis_value;
            }
        }
        let norm = dot(&vector, &vector).sqrt();
        vector.iter_mut().for_each(|value| *value /= norm);
        basis.push(vector);
    }
    basis
}

fn project_symmetric_part(payoffs: &[Vec<f64>], basis: &[Vec<f64>]) -> Vec<Vec<f64>> {
    basis
        .iter()
        .map(|left| {
            basis
                .iter()
                .map(|right| symmetric_bilinear_form(payoffs, left, right))
                .collect()
        })
        .collect()
}

fn matrix_vector(matrix: &[Vec<f64>], vector: &[f64]) -> Vec<f64> {
    matrix
        .iter()
        .map(|row| row.iter().zip(vector).map(|(a, x)| a * x).sum())
        .collect()
}

fn quadratic_form(matrix: &[Vec<f64>], vector: &[f64]) -> f64 {
    dot(vector, &matrix_vector(matrix, vector))
}

fn symmetric_bilinear_form(matrix: &[Vec<f64>], left: &[f64], right: &[f64]) -> f64 {
    let left_matrix_right = dot(left, &matrix_vector(matrix, right));
    let right_matrix_left = dot(right, &matrix_vector(matrix, left));
    (left_matrix_right + right_matrix_left) / 2.0
}

fn unit_difference(dimension: usize, positive: usize, negative: usize) -> Vec<f64> {
    let mut direction = vec![0.0; dimension];
    direction[positive] = 1.0;
    direction[negative] = -1.0;
    direction
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}
