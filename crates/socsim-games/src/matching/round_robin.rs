use socsim_core::SimRng;

use crate::{
    ArenaWriter, Credit, DecisionScope, ExperimentState, GameError, MatchContext, Matching,
    MatchingState, StageGame,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct RoundRobin {
    both_orders: bool,
    self_play: bool,
}

impl RoundRobin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn both_orders(mut self, enabled: bool) -> Self {
        self.both_orders = enabled;
        self
    }

    pub fn self_play(mut self, enabled: bool) -> Self {
        self.self_play = enabled;
        self
    }
}

impl<G: StageGame, X: ExperimentState> Matching<G, X> for RoundRobin {
    fn name(&self) -> &str {
        "round_robin"
    }

    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError> {
        if ctx.game().n_roles() != 2 {
            return Err(GameError::InvalidMatching(
                "RoundRobin requires exactly two roles".to_owned(),
            ));
        }
        if self.self_play {
            return Err(GameError::InvalidMatching(
                "RoundRobin self-play is not supported".to_owned(),
            ));
        }
        if ctx.decision_scope() == DecisionScope::PerStep
            && (ctx.ids().len() >= 3 || self.both_orders)
        {
            return Err(GameError::InvalidMatching(
                "RoundRobin assigns more than one role to an agent in a per-step decision"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn init_state(&self, _ctx: &MatchContext<'_, G, X>) -> MatchingState {
        MatchingState::RoundRobin { cursor: 0 }
    }

    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        state: &mut MatchingState,
        _rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()> {
        let mut ids = ctx.ids().to_vec();
        ids.sort_unstable();
        for left in 0..ids.len() {
            for right in left + 1..ids.len() {
                out.push(&[ids[left], ids[right]], Credit::All)?;
                if self.both_orders {
                    out.push(&[ids[right], ids[left]], Credit::All)?;
                }
            }
        }

        let MatchingState::RoundRobin { cursor } = state else {
            return Err(socsim_core::SocsimError::Mechanism(
                "RoundRobin matching state is not initialized".to_owned(),
            ));
        };
        *cursor = cursor.checked_add(1).ok_or_else(|| {
            socsim_core::SocsimError::Mechanism("RoundRobin cursor overflow".to_owned())
        })?;
        Ok(())
    }
}
