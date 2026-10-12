//! Best-response iteration for games with continuous actions.
//!
//! The solver iterates simultaneous best responses: every role responds to the
//! same previous profile, and the update is damped,
//! `x <- (1 - alpha) x + alpha BR(x)`, then projected onto the bounds.
//! Undamped simultaneous best responses need not converge. In linear Cournot
//! competition the interior equilibrium is neutrally stable for three firms and
//! unstable for four or more (Theocharis 1960), so the iteration is damped by
//! default and the damping is halved, restarting from the initial profile,
//! whenever an attempt does not converge.
//!
//! The best response of one role is taken, in this order, from the game's
//! analytic [`ContinuousGame::best_response`](crate::ContinuousGame::best_response), from an injected
//! [`BestResponseOracle`], or from a golden-section search when the game
//! declares [`NumericBr::Unimodal1D`](crate::NumericBr::Unimodal1D) and the role's action is one-dimensional.
//! Multi-dimensional games are never searched coordinate by coordinate: they
//! need an analytic best response or an oracle. A best response whose status is
//! not [`BrStatus::Converged`] is never used.

use std::fmt;

use crate::game::continuous::check_bounds;
use crate::{check_continuous_action, ContinuousGame, NumericBr};

use super::SolveError;

/// How a best-response computation ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BrStatus {
    /// The computation reached its tolerance. Only converged results may be
    /// used to judge an equilibrium or the convergence of an iteration.
    Converged,
    /// The iteration limit was reached before the tolerance.
    MaxIter,
    /// The computation could not be carried out.
    Failed,
}

/// The best response of one role.
#[derive(Clone, Debug, PartialEq)]
pub struct BrResult {
    /// The best-response action, one component per dimension of the bounds.
    pub action: Vec<f64>,
    /// A method-specific accuracy measure. For the golden-section search it is
    /// the width of the final bracket; for an analytic best response it is `0`.
    pub residual: f64,
    /// How the computation ended.
    pub status: BrStatus,
}

/// A user-supplied best-response solver, for example a global optimizer for a
/// multi-dimensional game without an analytic best response.
pub trait BestResponseOracle<G: ContinuousGame>: Send + Sync {
    /// Returns the best response of `role` to `profile`.
    ///
    /// `profile` holds the actions of all roles, indexed by role, and
    /// `profile[role]` must not be read. A result whose status is not
    /// [`BrStatus::Converged`] is treated as a failure by the callers in this
    /// crate.
    fn best_response(&self, game: &G, role: usize, profile: &[Vec<f64>]) -> BrResult;
}

/// Configuration for [`golden_section_best_response`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GoldenSectionOptions {
    tol_width: Option<f64>,
    max_iter: u32,
}

impl Default for GoldenSectionOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl GoldenSectionOptions {
    /// Constructs the default options: the search stops when the bracket is no
    /// wider than `1e-12 * max(1, hi - lo)`, and at most `200` reductions are
    /// performed.
    pub fn new() -> Self {
        Self {
            tol_width: None,
            max_iter: 200,
        }
    }

    /// Sets the absolute bracket width at which the search stops.
    pub fn tol_width(mut self, tol_width: f64) -> Self {
        self.tol_width = Some(tol_width);
        self
    }

    /// Sets the maximum number of bracket reductions.
    pub fn max_iter(mut self, max_iter: u32) -> Self {
        self.max_iter = max_iter;
        self
    }

    fn tol_width_for(&self, width: f64) -> f64 {
        self.tol_width.unwrap_or(1e-12 * width.max(1.0))
    }
}

/// Finds the best response of `role` by golden-section search over its
/// one-dimensional bounds.
///
/// The search is only valid for payoffs that are unimodal in the role's own
/// action, so it requires [`ContinuousGame::numeric_br`] to return
/// [`NumericBr::Unimodal1D`] and the role's bounds to have exactly one
/// dimension; otherwise the status is [`BrStatus::Failed`]. The status is also
/// `Failed` when `role` or `profile` does not match the game, when the bounds
/// are invalid, or when a payoff is not finite.
///
/// After the bracket has shrunk, the payoffs at both ends of the bounds, both
/// ends of the final bracket, and its midpoint are compared, and the best point
/// is returned (ties go to the smaller action). A best response projected onto
/// an end of the bounds is therefore returned exactly. Because maxima are found
/// by comparing payoff values, the location is resolved only to about
/// `sqrt(f64::EPSILON) * |x|`.
pub fn golden_section_best_response<G: ContinuousGame>(
    game: &G,
    role: usize,
    profile: &[Vec<f64>],
    options: &GoldenSectionOptions,
) -> BrResult {
    let failed = |action: Vec<f64>| BrResult {
        action,
        residual: f64::INFINITY,
        status: BrStatus::Failed,
    };
    if game.numeric_br() != NumericBr::Unimodal1D
        || role >= game.n_roles()
        || profile.len() != game.n_roles()
        || check_bounds(game).is_err()
    {
        return failed(Vec::new());
    }
    let bounds = game.bounds(role);
    if bounds.len() != 1 {
        return failed(Vec::new());
    }
    let (lo, hi) = bounds[0];
    let tol_width = options.tol_width_for(hi - lo);
    if !(tol_width.is_finite() && tol_width > 0.0) {
        return failed(vec![lo]);
    }

    let mut scratch = profile.to_vec();
    let mut evaluate = |x: f64| -> f64 {
        scratch[role] = vec![x];
        game.payoff(role, &scratch)
    };

    // 1 / phi, where phi is the golden ratio.
    let inv_phi = (5.0_f64.sqrt() - 1.0) / 2.0;
    let (mut a, mut b) = (lo, hi);
    let mut x1 = b - inv_phi * (b - a);
    let mut x2 = a + inv_phi * (b - a);
    let mut f1 = evaluate(x1);
    let mut f2 = evaluate(x2);
    if !f1.is_finite() || !f2.is_finite() {
        return failed(vec![lo]);
    }
    let mut iterations = 0_u32;
    while b - a > tol_width && iterations < options.max_iter {
        if f1 >= f2 {
            b = x2;
            x2 = x1;
            f2 = f1;
            x1 = b - inv_phi * (b - a);
            f1 = evaluate(x1);
            if !f1.is_finite() {
                return failed(vec![lo]);
            }
        } else {
            a = x1;
            x1 = x2;
            f1 = f2;
            x2 = a + inv_phi * (b - a);
            f2 = evaluate(x2);
            if !f2.is_finite() {
                return failed(vec![lo]);
            }
        }
        iterations += 1;
    }
    let width = b - a;
    let status = if width <= tol_width {
        BrStatus::Converged
    } else {
        BrStatus::MaxIter
    };

    let mid = (a + 0.5 * width).clamp(lo, hi);
    let candidates = [lo, a, mid, b, hi];
    let mut best = lo;
    let mut best_value = f64::NEG_INFINITY;
    for x in candidates {
        let value = evaluate(x);
        if !value.is_finite() {
            return failed(vec![lo]);
        }
        if value > best_value {
            best = x;
            best_value = value;
        }
    }
    BrResult {
        action: vec![best],
        residual: width,
        status,
    }
}

/// Configuration for [`best_response_iteration`].
pub struct BrIterationOptions<'a, G: ContinuousGame> {
    damping: f64,
    adaptive: bool,
    min_damping: f64,
    tol: Option<f64>,
    max_iter: u32,
    oracle: Option<&'a dyn BestResponseOracle<G>>,
}

impl<G: ContinuousGame> Clone for BrIterationOptions<'_, G> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<G: ContinuousGame> Copy for BrIterationOptions<'_, G> {}

impl<G: ContinuousGame> fmt::Debug for BrIterationOptions<'_, G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrIterationOptions")
            .field("damping", &self.damping)
            .field("adaptive", &self.adaptive)
            .field("min_damping", &self.min_damping)
            .field("tol", &self.tol)
            .field("max_iter", &self.max_iter)
            .field("oracle", &self.oracle.is_some())
            .finish()
    }
}

impl<G: ContinuousGame> Default for BrIterationOptions<'_, G> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, G: ContinuousGame> BrIterationOptions<'a, G> {
    /// Constructs the default options: damping `0.5`, adaptive halving on,
    /// minimum damping `2^-10`, the default tolerance described in
    /// [`BrIterationOptions::tol`], `10_000` updates per
    /// damping value, and no oracle.
    pub fn new() -> Self {
        Self {
            damping: 0.5,
            adaptive: true,
            min_damping: 1.0 / 1024.0,
            tol: None,
            max_iter: 10_000,
            oracle: None,
        }
    }

    /// Sets the initial damping factor, in `(0, 1]`.
    pub fn damping(mut self, damping: f64) -> Self {
        self.damping = damping;
        self
    }

    /// Sets whether the damping is halved after an attempt that does not
    /// converge. When `false`, only the initial damping is tried.
    pub fn adaptive(mut self, adaptive: bool) -> Self {
        self.adaptive = adaptive;
        self
    }

    /// Sets the smallest damping factor that is tried.
    pub fn min_damping(mut self, min_damping: f64) -> Self {
        self.min_damping = min_damping;
        self
    }

    /// Sets the relative residual tolerance. An iterate is accepted when
    /// `max |BR(x) - x| <= tol * max(1, largest bounds width)`.
    ///
    /// When no tolerance is set, it is chosen when the iteration starts: `1e-7`
    /// if some role's best response at the initial profile comes from the
    /// golden-section search (no analytic best response and no oracle), and
    /// `1e-12` otherwise. The numerical search locates a maximum only to about
    /// `sqrt(f64::EPSILON) * |x|`, so a tighter residual is not attainable on
    /// that path.
    pub fn tol(mut self, tol: f64) -> Self {
        self.tol = Some(tol);
        self
    }

    /// Sets the maximum number of damped updates per damping value.
    pub fn max_iter(mut self, max_iter: u32) -> Self {
        self.max_iter = max_iter;
        self
    }

    /// Sets the oracle used when the game has no analytic best response.
    pub fn oracle(mut self, oracle: &'a dyn BestResponseOracle<G>) -> Self {
        self.oracle = Some(oracle);
        self
    }
}

/// The result of [`best_response_iteration`].
#[derive(Clone, Debug, PartialEq)]
pub struct BrIteration {
    /// The final profile, one action per role.
    pub profile: Vec<Vec<f64>>,
    /// The infinity-norm residual `max |BR(x) - x|` at `profile`.
    pub residual: f64,
    /// [`BrStatus::Converged`] when the residual reached the tolerance, and
    /// [`BrStatus::MaxIter`] when no damping value converged. Only a converged
    /// profile is an approximate equilibrium.
    pub status: BrStatus,
    /// The damping factor of the last attempt.
    pub damping: f64,
    /// The total number of damped updates over all attempts.
    pub iterations: u64,
}

/// Performs one simultaneous, damped best-response update.
///
/// Every role's best response is computed from the same `profile`, then
/// `(1 - damping) x + damping BR(x)` is projected onto the bounds. With
/// `damping = 1` this is the undamped simultaneous best-response map. Best
/// responses are taken in the order described in the module documentation, and
/// a best response that is not [`BrStatus::Converged`] is an error.
pub fn best_response_step<G: ContinuousGame>(
    game: &G,
    profile: &[Vec<f64>],
    damping: f64,
    oracle: Option<&dyn BestResponseOracle<G>>,
) -> Result<Vec<Vec<f64>>, SolveError> {
    validate_game(game)?;
    validate_damping(damping)?;
    validate_profile(game, profile)?;
    let responses = all_best_responses(game, profile, oracle)?;
    Ok(damped_update(game, profile, &responses, damping))
}

/// Finds an equilibrium of a continuous game by damped simultaneous
/// best-response iteration from `x0`.
///
/// Each attempt runs up to `max_iter` damped updates. When an attempt does not
/// reach the tolerance and adaptive halving is on, the damping is halved and
/// the iteration restarts from `x0`; once the damping falls below
/// `min_damping` the result has status [`BrStatus::MaxIter`]. The residual is
/// computed with the same best responses as the updates, so a numerical best
/// response limits the attainable residual to its own accuracy. Unless a
/// tolerance is set explicitly, the default tolerance is therefore relative
/// `1e-7` when some role uses the numerical search at `x0`, and `1e-12` when
/// every role uses an analytic best response or the oracle (see
/// [`BrIterationOptions::tol`]).
pub fn best_response_iteration<G: ContinuousGame>(
    game: &G,
    x0: &[Vec<f64>],
    options: &BrIterationOptions<'_, G>,
) -> Result<BrIteration, SolveError> {
    validate_game(game)?;
    validate_options(options)?;
    validate_profile(game, x0)?;

    let mut width = 1.0_f64;
    for role in 0..game.n_roles() {
        for &(lo, hi) in game.bounds(role) {
            width = width.max(hi - lo);
        }
    }
    let tol = options.tol.unwrap_or_else(|| {
        if uses_numeric_search(game, x0, options.oracle) {
            1e-7
        } else {
            1e-12
        }
    });
    let threshold = tol * width;

    let mut damping = options.damping;
    let mut iterations = 0_u64;
    loop {
        let mut profile = x0.to_vec();
        let mut residual = f64::INFINITY;
        for step in 0..=options.max_iter {
            let responses = all_best_responses(game, &profile, options.oracle)?;
            residual = sup_distance(&profile, &responses);
            if residual <= threshold {
                return Ok(BrIteration {
                    profile,
                    residual,
                    status: BrStatus::Converged,
                    damping,
                    iterations,
                });
            }
            if step == options.max_iter {
                break;
            }
            profile = damped_update(game, &profile, &responses, damping);
            iterations += 1;
        }
        let next = damping / 2.0;
        if !options.adaptive || next < options.min_damping {
            return Ok(BrIteration {
                profile,
                residual,
                status: BrStatus::MaxIter,
                damping,
                iterations,
            });
        }
        damping = next;
    }
}

fn validate_game<G: ContinuousGame>(game: &G) -> Result<(), SolveError> {
    check_bounds(game).map_err(|violation| SolveError::InvalidGame(violation.to_string()))
}

fn validate_damping(damping: f64) -> Result<(), SolveError> {
    if damping.is_finite() && 0.0 < damping && damping <= 1.0 {
        Ok(())
    } else {
        Err(SolveError::InvalidOptions(
            "damping must be finite and in (0, 1]".into(),
        ))
    }
}

fn validate_options<G: ContinuousGame>(
    options: &BrIterationOptions<'_, G>,
) -> Result<(), SolveError> {
    validate_damping(options.damping)?;
    if !(options.min_damping.is_finite()
        && 0.0 < options.min_damping
        && options.min_damping <= options.damping)
    {
        return Err(SolveError::InvalidOptions(
            "min_damping must be finite, positive, and no larger than damping".into(),
        ));
    }
    if options
        .tol
        .is_some_and(|tol| !(tol.is_finite() && tol > 0.0))
    {
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

fn validate_profile<G: ContinuousGame>(game: &G, profile: &[Vec<f64>]) -> Result<(), SolveError> {
    if profile.len() != game.n_roles() {
        return Err(SolveError::InvalidOptions(format!(
            "profile has {} actions, expected {}",
            profile.len(),
            game.n_roles()
        )));
    }
    for (role, action) in profile.iter().enumerate() {
        check_continuous_action(game, role, action)
            .map_err(|error| SolveError::InvalidOptions(format!("invalid profile: {error}")))?;
    }
    Ok(())
}

/// Returns whether some role's best response at `profile` would come from the
/// golden-section search, following the order of [`role_best_response`].
fn uses_numeric_search<G: ContinuousGame>(
    game: &G,
    profile: &[Vec<f64>],
    oracle: Option<&dyn BestResponseOracle<G>>,
) -> bool {
    oracle.is_none()
        && game.numeric_br() == NumericBr::Unimodal1D
        && (0..game.n_roles())
            .any(|role| game.bounds(role).len() == 1 && game.best_response(role, profile).is_none())
}

/// Computes every role's best response to the same profile.
fn all_best_responses<G: ContinuousGame>(
    game: &G,
    profile: &[Vec<f64>],
    oracle: Option<&dyn BestResponseOracle<G>>,
) -> Result<Vec<Vec<f64>>, SolveError> {
    (0..game.n_roles())
        .map(|role| role_best_response(game, role, profile, oracle))
        .collect()
}

/// Takes the best response of one role from the analytic best response, the
/// oracle, or the golden-section search, in that order, and accepts it only
/// when it is converged, has the right dimension, and is finite.
fn role_best_response<G: ContinuousGame>(
    game: &G,
    role: usize,
    profile: &[Vec<f64>],
    oracle: Option<&dyn BestResponseOracle<G>>,
) -> Result<Vec<f64>, SolveError> {
    let result = if let Some(action) = game.best_response(role, profile) {
        BrResult {
            action,
            residual: 0.0,
            status: BrStatus::Converged,
        }
    } else if let Some(oracle) = oracle {
        oracle.best_response(game, role, profile)
    } else if game.numeric_br() == NumericBr::Unimodal1D && game.bounds(role).len() == 1 {
        golden_section_best_response(game, role, profile, &GoldenSectionOptions::new())
    } else {
        return Err(SolveError::NoBestResponse { role });
    };
    let dims = game.bounds(role).len();
    if result.status != BrStatus::Converged
        || result.action.len() != dims
        || result.action.iter().any(|value| !value.is_finite())
    {
        return Err(SolveError::NoBestResponse { role });
    }
    Ok(result.action)
}

fn sup_distance(profile: &[Vec<f64>], responses: &[Vec<f64>]) -> f64 {
    profile
        .iter()
        .zip(responses)
        .flat_map(|(action, response)| action.iter().zip(response))
        .map(|(&x, &y)| (y - x).abs())
        .fold(0.0, f64::max)
}

fn damped_update<G: ContinuousGame>(
    game: &G,
    profile: &[Vec<f64>],
    responses: &[Vec<f64>],
    damping: f64,
) -> Vec<Vec<f64>> {
    profile
        .iter()
        .zip(responses)
        .enumerate()
        .map(|(role, (action, response))| {
            action
                .iter()
                .zip(response)
                .zip(game.bounds(role))
                .map(|((&x, &y), &(lo, hi))| ((1.0 - damping) * x + damping * y).clamp(lo, hi))
                .collect()
        })
        .collect()
}
