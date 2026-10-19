use crate::FiniteGame;

use super::{inspect_game, SolveError};

/// One action removed during iterated strict-dominance elimination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elimination {
    /// One-based simultaneous-elimination round.
    pub round: usize,
    /// Zero-based role whose action was removed.
    pub role: usize,
    /// Zero-based action removed from that role.
    pub action: usize,
}

/// The fixed point of iterated elimination by pure-strategy strict dominance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DominanceResult {
    /// Remaining action indices for each role, in ascending order.
    pub surviving: Vec<Vec<usize>>,
    /// Removed actions in round, role, and action order.
    pub eliminated: Vec<Elimination>,
}

/// Simultaneously removes all actions strictly dominated by another surviving
/// pure action, repeating until no further action can be removed.
pub fn iterated_strict_dominance<G: FiniteGame>(game: &G) -> Result<DominanceResult, SolveError> {
    let info = inspect_game(game)?;
    let mut surviving = info
        .counts
        .iter()
        .map(|&count| (0..count).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut eliminated = Vec::new();
    let mut round = 1;

    loop {
        let mut remove = Vec::new();
        for role in 0..game.n_roles() {
            for &action in &surviving[role] {
                let dominated = surviving[role].iter().copied().any(|alternative| {
                    alternative != action
                        && strictly_dominates(
                            game,
                            role,
                            alternative,
                            action,
                            &surviving,
                            info.tolerance,
                        )
                });
                if dominated {
                    remove.push((role, action));
                }
            }
        }

        if remove.is_empty() {
            break;
        }
        for &(role, action) in &remove {
            surviving[role].retain(|candidate| *candidate != action);
            eliminated.push(Elimination {
                round,
                role,
                action,
            });
        }
        round += 1;
    }

    Ok(DominanceResult {
        surviving,
        eliminated,
    })
}

fn strictly_dominates<G: FiniteGame>(
    game: &G,
    role: usize,
    better: usize,
    worse: usize,
    surviving: &[Vec<usize>],
    tolerance: f64,
) -> bool {
    let opponents = (0..game.n_roles())
        .filter(|candidate| *candidate != role)
        .collect::<Vec<_>>();
    let mut opponent_indices = vec![0; opponents.len()];
    let mut profile = vec![0; game.n_roles()];

    loop {
        for (position, &opponent) in opponents.iter().enumerate() {
            profile[opponent] = surviving[opponent][opponent_indices[position]];
        }
        profile[role] = better;
        let better_payoff = game.payoff(role, &profile);
        profile[role] = worse;
        let worse_payoff = game.payoff(role, &profile);
        if better_payoff - worse_payoff <= tolerance {
            return false;
        }

        let mut advanced = false;
        for position in (0..opponents.len()).rev() {
            opponent_indices[position] += 1;
            if opponent_indices[position] < surviving[opponents[position]].len() {
                advanced = true;
                break;
            }
            opponent_indices[position] = 0;
        }
        if !advanced {
            return true;
        }
    }
}
