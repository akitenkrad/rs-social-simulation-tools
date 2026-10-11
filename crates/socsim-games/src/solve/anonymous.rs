use crate::game::MAX_PROFILES;
use crate::AnonymousGame;

use super::SolveError;

/// A population profile represented by the number of players using each action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountProfile {
    /// Counts indexed by action. Their sum is the number of roles in the game.
    pub counts: Vec<usize>,
}

/// Enumerates the pure Nash equilibria of an anonymous finite game.
///
/// The result is ordered lexicographically by [`CountProfile::counts`].
pub fn pure_nash_counts<G: AnonymousGame>(game: &G) -> Result<Vec<CountProfile>, SolveError> {
    let n_players = game.n_roles();
    if n_players == 0 {
        return Err(SolveError::UnsupportedRoles { n_roles: 0 });
    }

    let n_actions = game.n_actions(0);
    if n_actions == 0 {
        return Err(SolveError::EmptyActions { role: 0 });
    }
    for role in 1..n_players {
        let role_actions = game.n_actions(role);
        if role_actions == 0 {
            return Err(SolveError::EmptyActions { role });
        }
        if role_actions != n_actions {
            return Err(SolveError::InvalidOptions(
                "anonymous games require the same action count for every role".into(),
            ));
        }
    }

    let profiles = composition_count(n_players, n_actions);
    if profiles > MAX_PROFILES {
        return Err(SolveError::TooLarge { profiles });
    }

    let tolerance = anonymous_tolerance(game, n_players, n_actions)?;
    let mut equilibria = Vec::new();
    let mut counts = first_composition(n_players, n_actions);
    loop {
        if is_nash(game, &counts, tolerance)? {
            equilibria.push(CountProfile {
                counts: counts.clone(),
            });
        }
        if !advance_composition(&mut counts) {
            break;
        }
    }
    Ok(equilibria)
}

fn anonymous_tolerance<G: AnonymousGame>(
    game: &G,
    n_players: usize,
    n_actions: usize,
) -> Result<f64, SolveError> {
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    let mut counts_others = first_composition(n_players - 1, n_actions);
    loop {
        for action in 0..n_actions {
            let payoff = game.payoff_by_counts(action, &counts_others);
            if !payoff.is_finite() {
                return Err(SolveError::NonFinitePayoff);
            }
            minimum = minimum.min(payoff);
            maximum = maximum.max(payoff);
        }
        if !advance_composition(&mut counts_others) {
            break;
        }
    }

    let range = maximum - minimum;
    Ok(if range.is_finite() {
        1e-10 * range.max(1.0)
    } else {
        (maximum * 1e-10 - minimum * 1e-10).max(1e-10)
    })
}

fn is_nash<G: AnonymousGame>(
    game: &G,
    counts: &[usize],
    tolerance: f64,
) -> Result<bool, SolveError> {
    let mut counts_others = counts.to_vec();
    for action in 0..counts.len() {
        if counts[action] == 0 {
            continue;
        }
        counts_others[action] -= 1;
        let incumbent = game.payoff_by_counts(action, &counts_others);
        if !incumbent.is_finite() {
            return Err(SolveError::NonFinitePayoff);
        }
        for deviation in 0..counts.len() {
            let deviating = game.payoff_by_counts(deviation, &counts_others);
            if !deviating.is_finite() {
                return Err(SolveError::NonFinitePayoff);
            }
            if incumbent < deviating - tolerance {
                counts_others[action] += 1;
                return Ok(false);
            }
        }
        counts_others[action] += 1;
    }
    Ok(true)
}

fn composition_count(total: usize, parts: usize) -> u128 {
    let choose = (parts - 1).min(total);
    let top = total as u128 + parts as u128 - 1;
    let mut result = 1_u128;
    for i in 1..=choose as u128 {
        let numerator = top - choose as u128 + i;
        let Some(product) = result.checked_mul(numerator) else {
            return u128::MAX;
        };
        result = product / i;
    }
    result
}

fn first_composition(total: usize, parts: usize) -> Vec<usize> {
    let mut counts = vec![0; parts];
    counts[parts - 1] = total;
    counts
}

fn advance_composition(counts: &mut [usize]) -> bool {
    let Some(index) = (1..counts.len()).rev().find(|&index| counts[index] > 0) else {
        return false;
    };
    let amount = counts[index];
    counts[index] = 0;
    counts[index - 1] += 1;
    let last = counts.len() - 1;
    counts[last] += amount - 1;
    true
}
