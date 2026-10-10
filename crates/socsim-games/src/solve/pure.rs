use crate::FiniteGame;

use super::{advance_profile, inspect_game, SolveError};

/// A verified pure-strategy Nash equilibrium.
#[derive(Clone, Debug, PartialEq)]
pub struct PureNash {
    pub profile: Vec<usize>,
    pub payoffs: Vec<f64>,
    pub regret: f64,
}

/// Enumerates all pure-strategy Nash equilibria in lexicographic profile order.
pub fn pure_nash<G: FiniteGame>(game: &G) -> Result<Vec<PureNash>, SolveError> {
    let info = inspect_game(game)?;
    let n_roles = game.n_roles();
    let mut result = Vec::new();
    let mut profile = vec![0; n_roles];

    loop {
        let payoffs = (0..n_roles)
            .map(|role| game.payoff(role, &profile))
            .collect::<Vec<_>>();
        let mut maximum_regret = 0.0_f64;
        for role in 0..n_roles {
            let original = profile[role];
            let mut best = payoffs[role];
            for action in 0..info.counts[role] {
                profile[role] = action;
                best = best.max(game.payoff(role, &profile));
            }
            profile[role] = original;
            maximum_regret = maximum_regret.max(best - payoffs[role]);
        }
        if maximum_regret <= info.tolerance {
            result.push(PureNash {
                profile: profile.clone(),
                payoffs,
                regret: maximum_regret,
            });
        }

        if !advance_profile(&mut profile, &info.counts) {
            break;
        }
    }
    Ok(result)
}
