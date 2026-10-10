use serde::{Deserialize, Serialize};

use crate::{DecisionContext, ExperimentState, FiniteGame, StageGame, StepRecord};

pub trait ObservationProvider<G: StageGame, X: ExperimentState>: Send + Sync {
    type Obs;

    fn observe(&self, ctx: &DecisionContext<'_, G, X>) -> Self::Obs;
}

#[derive(Clone, Copy, Debug)]
pub struct OwnHistoryObs {
    window: usize,
}

impl OwnHistoryObs {
    pub fn new(window: usize) -> Self {
        Self { window }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnHistory<A> {
    pub t: u64,
    pub role: Option<usize>,
    pub mine: Vec<StepRecord<A>>,
    pub opponents_last: Vec<Option<A>>,
}

impl<G: StageGame, X: ExperimentState> ObservationProvider<G, X> for OwnHistoryObs {
    type Obs = OwnHistory<G::Action>;

    fn observe(&self, ctx: &DecisionContext<'_, G, X>) -> Self::Obs {
        let history = ctx.my_history();
        let skip = history.len().saturating_sub(self.window);
        let mine = history.iter().skip(skip).cloned().collect();
        let opponents_last = ctx
            .opponents()
            .iter()
            .map(|&opponent| {
                ctx.opponent_history(opponent)
                    .and_then(|history| history.last())
                    .flatten()
                    .cloned()
            })
            .collect();
        OwnHistory {
            t: ctx.t(),
            role: ctx.role(),
            mine,
            opponents_last,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CountsObs;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionCounts {
    pub role: Option<usize>,
    pub my_last: Option<usize>,
    pub counts: Vec<u32>,
}

impl<G: FiniteGame, X: ExperimentState> ObservationProvider<G, X> for CountsObs {
    type Obs = ActionCounts;

    fn observe(&self, ctx: &DecisionContext<'_, G, X>) -> Self::Obs {
        let action_count = (0..ctx.game().n_roles())
            .map(|role| ctx.game().n_actions(role))
            .max()
            .unwrap_or(0);
        let mut counts = vec![0_u32; action_count];
        for action in ctx.opponents().iter().filter_map(|&opponent| {
            ctx.opponent_history(opponent)
                .and_then(|history| history.last())
                .flatten()
        }) {
            if let Some(count) = counts.get_mut(*action) {
                *count = count.saturating_add(1);
            }
        }
        ActionCounts {
            role: ctx.role(),
            my_last: ctx.my_history().last().and_then(|record| record.action),
            counts,
        }
    }
}
