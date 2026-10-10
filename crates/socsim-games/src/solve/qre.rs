use crate::mixed::{action_payoffs, regrets, MixedError};
use crate::{FiniteGame, MixedStrategy};

use super::{inspect_game, SolveError};

/// Configuration for [`logit_qre`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct QreOptions {
    /// The non-negative logit precision parameter.
    pub lambda: f64,
    /// An optional initial mixed-strategy profile.
    pub init: Option<Vec<MixedStrategy>>,
    /// The damping factor in `(0, 1]`. Defaults to `0.1`.
    pub damping: f64,
    /// The positive fixed-point residual tolerance. Defaults to `1e-12`.
    pub tol: f64,
    /// The maximum number of damped updates. Defaults to `100_000`.
    pub max_iter: usize,
}

impl QreOptions {
    /// Constructs options for the specified logit precision.
    ///
    /// The initial profile is uniform, the damping factor is `0.1`, the
    /// fixed-point tolerance is `1e-12`, and the update limit is `100_000`.
    pub fn new(lambda: f64) -> Self {
        Self {
            lambda,
            init: None,
            damping: 0.1,
            tol: 1e-12,
            max_iter: 100_000,
        }
    }

    /// Sets the initial mixed-strategy profile.
    pub fn with_init(mut self, init: Vec<MixedStrategy>) -> Self {
        self.init = Some(init);
        self
    }

    /// Sets the damping factor.
    pub fn with_damping(mut self, damping: f64) -> Self {
        self.damping = damping;
        self
    }

    /// Sets the fixed-point residual tolerance.
    pub fn with_tol(mut self, tol: f64) -> Self {
        self.tol = tol;
        self
    }

    /// Sets the maximum number of damped updates.
    pub fn with_max_iter(mut self, max_iter: usize) -> Self {
        self.max_iter = max_iter;
        self
    }
}

/// A locally found fixed point of the logit response map.
///
/// `regrets` contains the ordinary, unregularized regret for every role. A
/// logit QRE is not generally a Nash equilibrium at finite `lambda`.
#[derive(Clone, Debug, PartialEq)]
pub struct QreSolution {
    /// The logit precision used to compute the solution.
    pub lambda: f64,
    /// One mixed strategy per role.
    pub strategies: Vec<MixedStrategy>,
    /// The infinity-norm residual of the logit fixed-point equation.
    pub residual: f64,
    /// The ordinary regret of each role at the returned profile.
    pub regrets: Vec<f64>,
    /// The number of damped updates performed.
    pub iterations: usize,
}

/// Finds a local fixed point of the logit response map by damped iteration.
///
/// The search starts from `options.init`, or from the uniform profile when no
/// initial profile is supplied. Games can have multiple logit QREs, so distinct
/// initial profiles can converge to distinct fixed points. Increasing `lambda`
/// and reusing the preceding solution can be useful, but this iteration does
/// not trace the principal branch and cannot in general pass folds or
/// bifurcations. A solution is returned only after the residual has been
/// recomputed for the returned profile and found no larger than `options.tol`.
pub fn logit_qre<G: FiniteGame>(game: &G, options: &QreOptions) -> Result<QreSolution, SolveError> {
    validate_options(options)?;
    let info = inspect_game(game)?;
    let mut strategies = initial_profile(options, &info.counts)?;

    let mut residual = fixed_point_residual(game, &strategies, options.lambda)?;
    if residual <= options.tol {
        if let Some(solution) = verified_solution(game, options, &strategies, 0)? {
            return Ok(solution);
        }
    }

    for iterations in 1..=options.max_iter {
        let responses = logit_responses(game, &strategies, options.lambda)?;
        strategies = damped_update(&strategies, &responses, options.damping)?;
        residual = fixed_point_residual(game, &strategies, options.lambda)?;
        if residual <= options.tol {
            if let Some(solution) = verified_solution(game, options, &strategies, iterations)? {
                return Ok(solution);
            }
        }
    }

    residual = fixed_point_residual(game, &strategies, options.lambda)?;
    Err(SolveError::NotConverged {
        residual,
        iterations: options.max_iter,
    })
}

fn validate_options(options: &QreOptions) -> Result<(), SolveError> {
    if !options.lambda.is_finite() || options.lambda < 0.0 {
        return Err(SolveError::InvalidOptions(
            "lambda must be finite and non-negative".into(),
        ));
    }
    if !(options.damping.is_finite() && 0.0 < options.damping && options.damping <= 1.0) {
        return Err(SolveError::InvalidOptions(
            "damping must be finite and in (0, 1]".into(),
        ));
    }
    if !options.tol.is_finite() || options.tol <= 0.0 {
        return Err(SolveError::InvalidOptions(
            "tol must be finite and positive".into(),
        ));
    }
    if options.max_iter == 0 {
        return Err(SolveError::InvalidOptions(
            "max_iter must be positive".into(),
        ));
    }
    Ok(())
}

fn initial_profile(
    options: &QreOptions,
    counts: &[usize],
) -> Result<Vec<MixedStrategy>, SolveError> {
    let strategies = match &options.init {
        Some(init) => {
            validate_dimensions(init, counts)?;
            init.clone()
        }
        None => counts
            .iter()
            .map(|&count| MixedStrategy::uniform(count))
            .collect::<Result<Vec<_>, _>>()?,
    };

    if options.lambda == 0.0 {
        counts
            .iter()
            .map(|&count| MixedStrategy::uniform(count))
            .collect::<Result<Vec<_>, _>>()
            .map_err(SolveError::from)
    } else {
        Ok(strategies)
    }
}

fn validate_dimensions(strategies: &[MixedStrategy], counts: &[usize]) -> Result<(), MixedError> {
    if strategies.len() != counts.len() {
        return Err(MixedError::RoleCount {
            expected: counts.len(),
            actual: strategies.len(),
        });
    }
    for (role, (strategy, &expected)) in strategies.iter().zip(counts).enumerate() {
        if strategy.len() != expected {
            return Err(MixedError::Dimension {
                role,
                expected,
                actual: strategy.len(),
            });
        }
    }
    Ok(())
}

fn logit_responses<G: FiniteGame>(
    game: &G,
    strategies: &[MixedStrategy],
    lambda: f64,
) -> Result<Vec<Vec<f64>>, SolveError> {
    if lambda == 0.0 {
        return (0..game.n_roles())
            .map(|role| {
                Ok(vec![
                    1.0 / game.n_actions(role) as f64;
                    game.n_actions(role)
                ])
            })
            .collect();
    }
    (0..game.n_roles())
        .map(|role| {
            let payoffs = action_payoffs(game, role, strategies)?;
            softmax(&payoffs, lambda)
        })
        .collect()
}

fn softmax(payoffs: &[f64], lambda: f64) -> Result<Vec<f64>, SolveError> {
    let maximum = payoffs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !maximum.is_finite() {
        return Err(SolveError::NonFinitePayoff);
    }

    let mut weights = payoffs
        .iter()
        .map(|payoff| (lambda * (payoff - maximum)).exp())
        .collect::<Vec<_>>();
    let total = weights.iter().sum::<f64>();
    if !total.is_finite() || total <= 0.0 {
        return Err(SolveError::NonFinitePayoff);
    }
    for weight in &mut weights {
        *weight /= total;
    }
    Ok(weights)
}

fn damped_update(
    strategies: &[MixedStrategy],
    responses: &[Vec<f64>],
    damping: f64,
) -> Result<Vec<MixedStrategy>, SolveError> {
    strategies
        .iter()
        .zip(responses)
        .map(|(strategy, response)| {
            let probabilities = strategy
                .probs()
                .iter()
                .zip(response)
                .map(|(&probability, &target)| (1.0 - damping) * probability + damping * target)
                .collect::<Vec<_>>();
            normalize_strategy(probabilities).map_err(SolveError::from)
        })
        .collect()
}

fn normalize_strategy(mut probabilities: Vec<f64>) -> Result<MixedStrategy, MixedError> {
    let total = probabilities.iter().sum::<f64>();
    if !total.is_finite() || total <= 0.0 {
        return Err(MixedError::NotADistribution);
    }
    for probability in &mut probabilities {
        *probability /= total;
    }
    MixedStrategy::new(probabilities)
}

fn fixed_point_residual<G: FiniteGame>(
    game: &G,
    strategies: &[MixedStrategy],
    lambda: f64,
) -> Result<f64, SolveError> {
    let responses = logit_responses(game, strategies, lambda)?;
    Ok(strategies
        .iter()
        .zip(&responses)
        .flat_map(|(strategy, response)| strategy.probs().iter().zip(response))
        .map(|(&probability, &target)| (probability - target).abs())
        .fold(0.0, f64::max))
}

fn verified_solution<G: FiniteGame>(
    game: &G,
    options: &QreOptions,
    strategies: &[MixedStrategy],
    iterations: usize,
) -> Result<Option<QreSolution>, SolveError> {
    let strategies = strategies
        .iter()
        .map(|strategy| normalize_strategy(strategy.probs().to_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    let residual = fixed_point_residual(game, &strategies, options.lambda)?;
    if residual > options.tol {
        return Ok(None);
    }
    let regrets = regrets(game, &strategies)?;
    Ok(Some(QreSolution {
        lambda: options.lambda,
        strategies,
        residual,
        regrets,
        iterations,
    }))
}
