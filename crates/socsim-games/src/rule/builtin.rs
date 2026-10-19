use rand::Rng;
use socsim_core::SimRng;

use crate::{
    DecisionContext, DecisionRule, ExperimentState, FiniteGame, LearnContext, RuleError,
    RuleInitContext, Slot, StageGame, Stateless, TftState,
};

use super::pure_strategy;

#[derive(Clone, Debug)]
/// Rule that returns the same configured action on every decision.
pub struct Fixed<A> {
    action: A,
}

impl<A> Fixed<A> {
    /// Creates a fixed rule; initialization later validates the action for every role.
    pub fn new(action: A) -> Self {
        Self { action }
    }
}

impl<G, X> DecisionRule<G, X> for Fixed<G::Action>
where
    G: StageGame,
    X: ExperimentState,
    X::PolicyState: Slot<Stateless>,
{
    fn name(&self) -> &str {
        "fixed"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        for role in 0..game.n_roles() {
            game.check_action(role, &self.action)
                .map_err(|error| RuleError::InvalidAction(error.to_string()))?;
        }
        Ok(<X::PolicyState as Slot<Stateless>>::from_slot(Stateless))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        _ctx: &DecisionContext<'_, G, X>,
        _rng: &mut SimRng,
    ) -> Result<G::Action, RuleError> {
        <X::PolicyState as Slot<Stateless>>::slot(state)
            .ok_or(RuleError::MissingSlot("Stateless"))?;
        Ok(self.action.clone())
    }
}

#[derive(Clone, Copy, Debug, Default)]
/// Rule that samples uniformly from the finite actions of its assigned role.
pub struct UniformRandom;

impl<G, X> DecisionRule<G, X> for UniformRandom
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<Stateless>,
{
    fn name(&self) -> &str {
        "uniform_random"
    }

    fn init_state(&self, _ctx: &RuleInitContext, _game: &G) -> Result<X::PolicyState, RuleError> {
        Ok(<X::PolicyState as Slot<Stateless>>::from_slot(Stateless))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        state.slot().ok_or(RuleError::MissingSlot("Stateless"))?;
        let role = ctx.role().ok_or_else(|| {
            RuleError::Unsupported("UniformRandom requires an assigned role".to_owned())
        })?;
        let n_actions = ctx.game().n_actions(role);
        if n_actions == 0 {
            return Err(RuleError::Unsupported(format!(
                "role {role} has no available actions"
            )));
        }
        Ok(rng.gen_range(0..n_actions))
    }

    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> {
        <X::PolicyState as Slot<Stateless>>::slot(state)?;
        if role >= game.n_roles() {
            return None;
        }
        let n_actions = game.n_actions(role);
        if n_actions == 0 {
            return None;
        }
        Some(vec![1.0 / n_actions as f64; n_actions])
    }
}

#[derive(Clone, Copy, Debug)]
/// Two-player, two-action rule that copies the opponent's preceding action.
///
/// Before observing an opponent, the rule plays its configured cooperation action.
pub struct TitForTat {
    cooperate: usize,
}

impl TitForTat {
    /// Creates the rule with the zero-based action treated as cooperation.
    pub fn new(coop: usize) -> Self {
        Self { cooperate: coop }
    }
}

impl<G, X> DecisionRule<G, X> for TitForTat
where
    G: FiniteGame,
    X: ExperimentState,
    X::PolicyState: Slot<TftState>,
{
    fn name(&self) -> &str {
        "tit_for_tat"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        if game.n_roles() != 2 {
            return Err(RuleError::Unsupported(format!(
                "TitForTat requires exactly two roles, got {}",
                game.n_roles()
            )));
        }
        for role in 0..game.n_roles() {
            let n_actions = game.n_actions(role);
            if n_actions != 2 {
                return Err(RuleError::Unsupported(format!(
                    "TitForTat requires two actions for role {role}, got {n_actions}"
                )));
            }
            if game.check_action(role, &self.cooperate).is_err() {
                return Err(RuleError::Unsupported(format!(
                    "TitForTat cooperation action {} is invalid for role {role}",
                    self.cooperate
                )));
            }
        }
        Ok(<X::PolicyState as Slot<TftState>>::from_slot(
            TftState::default(),
        ))
    }

    fn decide(
        &self,
        state: &mut X::PolicyState,
        _ctx: &DecisionContext<'_, G, X>,
        _rng: &mut SimRng,
    ) -> Result<usize, RuleError> {
        let state = state.slot().ok_or(RuleError::MissingSlot("TftState"))?;
        Ok(state.last_opponent.unwrap_or(self.cooperate))
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        let state = state.slot_mut().ok_or(RuleError::MissingSlot("TftState"))?;
        for outcome in ctx.outcomes() {
            let opponent_role = match outcome.role {
                0 => 1,
                1 => 0,
                role => {
                    return Err(RuleError::Unsupported(format!(
                        "TitForTat requires a two-role encounter, got role {role}"
                    )))
                }
            };
            let action = outcome
                .actions
                .get(opponent_role)
                .and_then(Option::as_ref)
                .copied()
                .ok_or_else(|| {
                    RuleError::InvalidAction(
                        "TitForTat observed an unfilled opponent action".to_owned(),
                    )
                })?;
            state.last_opponent = Some(action);
        }
        Ok(())
    }

    fn mixed(&self, state: &X::PolicyState, role: usize, game: &G) -> Option<Vec<f64>> {
        let state = <X::PolicyState as Slot<TftState>>::slot(state)?;
        if role >= game.n_roles() {
            return None;
        }
        pure_strategy(
            game.n_actions(role),
            state.last_opponent.unwrap_or(self.cooperate),
        )
    }
}
