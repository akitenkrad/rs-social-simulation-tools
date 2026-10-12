//! Decision rules for games with continuous actions.
//!
//! Both rules move simultaneously: the actions of the other roles are read
//! from the step history, so every agent responds to the actions played in the
//! previous step, whatever order agents are activated in. Held actions of peers
//! are never read, because the decision mechanism updates them in activation
//! order within a step. A peer that sat out a step is represented by the last
//! action it played. When the action of some other role is not known yet (the
//! first step), the agent repeats its own held action.

use std::fmt;
use std::sync::Arc;

use socsim_core::SimRng;

use crate::game::continuous::check_bounds;
use crate::solve::{
    golden_section_best_response, BestResponseOracle, BrStatus, GoldenSectionOptions,
};
use crate::{
    ContinuousGame, ContinuousState, DecisionContext, DecisionRule, ExperimentState, LearnContext,
    NumericBr, RuleError, RuleInitContext, Slot,
};

/// Damped simultaneous best response,
/// `x <- (1 - alpha) x + alpha BR(x_-i)`, projected onto the bounds.
///
/// The opponents' actions are those of the previous step (see the module
/// documentation), and `x` is the action this agent last played. The best
/// response is taken from the game's analytic
/// [`ContinuousGame::best_response`], then from an injected
/// [`BestResponseOracle`], then from a golden-section search when the game
/// declares [`NumericBr::Unimodal1D`] and the role's action is one-dimensional.
/// A best response whose status is not [`BrStatus::Converged`] stops the step
/// with [`RuleError::BestResponse`]. Undamped simultaneous best responses need
/// not converge (Theocharis 1960), which is what the damping is for.
///
/// The rule name is `damped_best_response`.
pub struct DampedBestResponse<G: ContinuousGame> {
    alpha: f64,
    oracle: Option<Arc<dyn BestResponseOracle<G>>>,
}

impl<G: ContinuousGame> Clone for DampedBestResponse<G> {
    fn clone(&self) -> Self {
        Self {
            alpha: self.alpha,
            oracle: self.oracle.clone(),
        }
    }
}

impl<G: ContinuousGame> fmt::Debug for DampedBestResponse<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DampedBestResponse")
            .field("alpha", &self.alpha)
            .field("oracle", &self.oracle.is_some())
            .finish()
    }
}

impl<G: ContinuousGame> DampedBestResponse<G> {
    /// Constructs the rule with damping `alpha` (`br_damping`), which must be
    /// finite and in `(0, 1]`.
    pub fn new(alpha: f64) -> Result<Self, RuleError> {
        if !(alpha.is_finite() && 0.0 < alpha && alpha <= 1.0) {
            return Err(RuleError::Unsupported(format!(
                "br_damping must be finite and in (0, 1], got {alpha}"
            )));
        }
        Ok(Self {
            alpha,
            oracle: None,
        })
    }

    /// Uses `oracle` for roles without an analytic best response.
    pub fn with_oracle(mut self, oracle: Arc<dyn BestResponseOracle<G>>) -> Self {
        self.oracle = Some(oracle);
        self
    }

    /// Returns the damping.
    pub fn alpha(&self) -> f64 {
        self.alpha
    }

    fn best_response(
        &self,
        game: &G,
        role: usize,
        profile: &[Vec<f64>],
    ) -> Result<Vec<f64>, RuleError> {
        let result = if let Some(action) = game.best_response(role, profile) {
            action
        } else if let Some(oracle) = &self.oracle {
            let result = oracle.best_response(game, role, profile);
            if result.status != BrStatus::Converged {
                return Err(RuleError::BestResponse(format!(
                    "the oracle did not converge for role {role} (status {:?})",
                    result.status
                )));
            }
            result.action
        } else if game.numeric_br() == NumericBr::Unimodal1D && game.bounds(role).len() == 1 {
            let result =
                golden_section_best_response(game, role, profile, &GoldenSectionOptions::new());
            if result.status != BrStatus::Converged {
                return Err(RuleError::BestResponse(format!(
                    "the golden-section search did not converge for role {role} (status {:?})",
                    result.status
                )));
            }
            result.action
        } else {
            return Err(RuleError::BestResponse(format!(
                "role {role} has no analytic best response, no oracle, and no numerical search"
            )));
        };
        let dims = game.bounds(role).len();
        if result.len() != dims {
            return Err(RuleError::BestResponse(format!(
                "the best response for role {role} has {} components, expected {dims}",
                result.len()
            )));
        }
        if let Some(dim) = result.iter().position(|value| !value.is_finite()) {
            return Err(RuleError::BestResponse(format!(
                "component {dim} of the best response for role {role} is not finite"
            )));
        }
        Ok(result)
    }
}

impl<G, X> DecisionRule<G, X> for DampedBestResponse<G>
where
    G: ContinuousGame,
    X: ExperimentState,
    X::PolicyState: Slot<ContinuousState>,
{
    fn name(&self) -> &str {
        "damped_best_response"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        check_bounds(game).map_err(|violation| RuleError::Unsupported(violation.to_string()))?;
        if self.oracle.is_none() {
            let probe = midpoint_profile(game);
            for role in 0..game.n_roles() {
                let numeric =
                    game.numeric_br() == NumericBr::Unimodal1D && game.bounds(role).len() == 1;
                if !numeric && game.best_response(role, &probe).is_none() {
                    return Err(RuleError::Unsupported(format!(
                        "DampedBestResponse needs a best response for role {role}: the game has \
                         no analytic best response, no oracle was given, and the numerical \
                         search requires a one-dimensional action declared unimodal"
                    )));
                }
            }
        }
        Ok(<X::PolicyState as Slot<ContinuousState>>::from_slot(
            ContinuousState::default(),
        ))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        _rng: &mut SimRng,
    ) -> Result<Vec<f64>, RuleError> {
        let state = <X::PolicyState as Slot<ContinuousState>>::slot(state)
            .ok_or(RuleError::MissingSlot("ContinuousState"))?;
        let game = ctx.game();
        let role = assigned_role(ctx, "DampedBestResponse")?;
        let own = own_action(state, ctx);
        let Some(profile) = previous_step_profile(ctx, role, &own)? else {
            return Ok(ctx.my_held().clone());
        };
        let response = self.best_response(game, role, &profile)?;
        Ok(own
            .iter()
            .zip(&response)
            .zip(game.bounds(role))
            .map(|((&x, &y), &(lo, hi))| ((1.0 - self.alpha) * x + self.alpha * y).clamp(lo, hi))
            .collect())
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        remember_played(state, ctx)
    }
}

/// Projected gradient ascent on the agent's own payoff,
/// `x <- Proj(x + step * grad_x u_i(x, x_-i))`.
///
/// The opponents' actions are those of the previous step (see the module
/// documentation), and `x` is the action this agent last played. The gradient
/// is the game's analytic [`ContinuousGame::gradient`] when it returns `Some`.
/// Otherwise it is a central difference with step
/// `h = 1e-6 * max(1, |x_d|)`; near a bound it switches to the one-sided
/// difference that stays inside the bounds (forward when `x_d - h < lo`,
/// backward when `x_d + h > hi`), and in a dimension narrower than the step on
/// both sides it uses half the width of the dimension. The payoff is never
/// evaluated outside the bounds. A NaN or infinite gradient component stops the
/// step with [`RuleError::NonFiniteGradient`].
///
/// The rule name is `gradient`.
#[derive(Clone, Copy, Debug)]
pub struct GradientAscent {
    step: f64,
}

impl GradientAscent {
    /// Constructs the rule with step size `step` (`grad_step`), which must be
    /// finite and positive.
    pub fn new(step: f64) -> Result<Self, RuleError> {
        if !(step.is_finite() && step > 0.0) {
            return Err(RuleError::Unsupported(format!(
                "grad_step must be finite and positive, got {step}"
            )));
        }
        Ok(Self { step })
    }

    /// Returns the step size.
    pub fn step(&self) -> f64 {
        self.step
    }
}

impl<G, X> DecisionRule<G, X> for GradientAscent
where
    G: ContinuousGame,
    X: ExperimentState,
    X::PolicyState: Slot<ContinuousState>,
{
    fn name(&self) -> &str {
        "gradient"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        check_bounds(game).map_err(|violation| RuleError::Unsupported(violation.to_string()))?;
        Ok(<X::PolicyState as Slot<ContinuousState>>::from_slot(
            ContinuousState::default(),
        ))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        _rng: &mut SimRng,
    ) -> Result<Vec<f64>, RuleError> {
        let state = <X::PolicyState as Slot<ContinuousState>>::slot(state)
            .ok_or(RuleError::MissingSlot("ContinuousState"))?;
        let game = ctx.game();
        let role = assigned_role(ctx, "GradientAscent")?;
        let own = own_action(state, ctx);
        let Some(mut profile) = previous_step_profile(ctx, role, &own)? else {
            return Ok(ctx.my_held().clone());
        };
        let gradient = match game.gradient(role, &profile) {
            Some(gradient) => {
                let dims = game.bounds(role).len();
                if gradient.len() != dims {
                    return Err(RuleError::Unsupported(format!(
                        "the gradient for role {role} has {} components, expected {dims}",
                        gradient.len()
                    )));
                }
                gradient
            }
            None => numeric_gradient(game, role, &mut profile),
        };
        if let Some(dim) = gradient.iter().position(|value| !value.is_finite()) {
            return Err(RuleError::NonFiniteGradient { dim });
        }
        Ok(own
            .iter()
            .zip(&gradient)
            .zip(game.bounds(role))
            .map(|((&x, &g), &(lo, hi))| (x + self.step * g).clamp(lo, hi))
            .collect())
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        remember_played(state, ctx)
    }
}

/// Computes the finite-difference gradient of `role`'s payoff with respect to
/// its own action, `profile[role]`, without evaluating outside the bounds.
///
/// `profile[role]` is restored before returning.
fn numeric_gradient<G: ContinuousGame>(
    game: &G,
    role: usize,
    profile: &mut [Vec<f64>],
) -> Vec<f64> {
    let bounds = game.bounds(role);
    let mut gradient = Vec::with_capacity(bounds.len());
    for (dim, &(lo, hi)) in bounds.iter().enumerate() {
        let x = profile[role][dim];
        let mut h = 1e-6 * x.abs().max(1.0);
        let fits_above = x + h <= hi;
        let fits_below = x - h >= lo;
        let mut evaluate = |value: f64| {
            profile[role][dim] = value;
            game.payoff(role, profile)
        };
        let value = if fits_above && fits_below {
            (evaluate(x + h) - evaluate(x - h)) / (2.0 * h)
        } else if fits_above {
            (evaluate(x + h) - evaluate(x)) / h
        } else if fits_below {
            (evaluate(x) - evaluate(x - h)) / h
        } else {
            h = (hi - lo) / 2.0;
            if x + h <= hi {
                (evaluate(x + h) - evaluate(x)) / h
            } else {
                (evaluate(x) - evaluate(x - h)) / h
            }
        };
        profile[role][dim] = x;
        gradient.push(value);
    }
    gradient
}

fn assigned_role<G: ContinuousGame, X: ExperimentState>(
    ctx: &DecisionContext<'_, G, X>,
    rule: &str,
) -> Result<usize, RuleError> {
    let role = ctx
        .role()
        .ok_or_else(|| RuleError::Unsupported(format!("{rule} requires an assigned role")))?;
    if role >= ctx.game().n_roles() {
        return Err(RuleError::Unsupported(format!(
            "{rule} was assigned role {role}, but the game has {} roles",
            ctx.game().n_roles()
        )));
    }
    Ok(role)
}

/// Returns the action this agent last played, or its held action before it
/// has played.
fn own_action<G: ContinuousGame, X: ExperimentState>(
    state: &ContinuousState,
    ctx: &DecisionContext<'_, G, X>,
) -> Vec<f64> {
    state.last.clone().unwrap_or_else(|| ctx.my_held().clone())
}

/// Builds the profile `own` responds to: the other roles hold the last action
/// their occupants played in previous steps. Returns `None` when some other
/// role's action is not known yet.
fn previous_step_profile<G: ContinuousGame, X: ExperimentState>(
    ctx: &DecisionContext<'_, G, X>,
    role: usize,
    own: &[f64],
) -> Result<Option<Vec<Vec<f64>>>, RuleError> {
    let n_roles = ctx.game().n_roles();
    let seats: Vec<_> = match ctx.encounter() {
        Some(encounter) if encounter.seats.len() == n_roles => encounter.seats.to_vec(),
        _ if ctx.opponents().len() + 1 == n_roles => {
            let mut opponents = ctx.opponents().iter().copied();
            (0..n_roles)
                .map(|seat| {
                    if seat == role {
                        ctx.me()
                    } else {
                        opponents.next().expect("the opponent count was checked")
                    }
                })
                .collect()
        }
        _ => {
            return Err(RuleError::Unsupported(format!(
                "continuous rules need one occupant per other role, got {} opponents for {n_roles} \
                 roles",
                ctx.opponents().len()
            )))
        }
    };
    let mut profile = Vec::with_capacity(n_roles);
    for (seat, &occupant) in seats.iter().enumerate() {
        if seat == role {
            profile.push(own.to_vec());
            continue;
        }
        let last = ctx
            .opponent_history(occupant)
            .and_then(|history| history.actions().flatten().last());
        match last {
            Some(action) => profile.push(action.clone()),
            None => return Ok(None),
        }
    }
    Ok(Some(profile))
}

/// Records the action the agent actually played this step, if it played.
fn remember_played<G: ContinuousGame, X: ExperimentState>(
    state: &mut X::PolicyState,
    ctx: &LearnContext<'_, G, X>,
) -> Result<(), RuleError>
where
    X::PolicyState: Slot<ContinuousState>,
{
    let state = <X::PolicyState as Slot<ContinuousState>>::slot_mut(state)
        .ok_or(RuleError::MissingSlot("ContinuousState"))?;
    if let Some(action) = &ctx.record().action {
        state.last = Some(action.clone());
    }
    Ok(())
}

/// The profile in which every role plays the midpoint of its bounds, used to
/// probe whether the game offers an analytic best response.
fn midpoint_profile<G: ContinuousGame>(game: &G) -> Vec<Vec<f64>> {
    (0..game.n_roles())
        .map(|role| {
            game.bounds(role)
                .iter()
                .map(|&(lo, hi)| lo + (hi - lo) / 2.0)
                .collect()
        })
        .collect()
}
