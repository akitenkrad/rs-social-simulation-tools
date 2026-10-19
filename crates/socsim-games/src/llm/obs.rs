use serde::{Deserialize, Serialize};

use crate::{DecisionContext, ExperimentState, FiniteGame, StageGame, StepRecord};

/// Projects a decision context into the information rendered for an LLM.
///
/// Observation authors implement this trait and compose it with a
/// [`GameRenderer`](super::GameRenderer). The driver calls [`observe`](Self::observe)
/// once per prepared decision. Implementations should use the bounded
/// [`DecisionContext`] interface unless explicitly opting out of its
/// information structure.
pub trait ObservationProvider<G: StageGame, X: ExperimentState>: Send + Sync {
    /// Owned observation passed to the paired renderer.
    type Obs;

    /// Captures the information visible for one decision unit.
    fn observe(&self, ctx: &DecisionContext<'_, G, X>) -> Self::Obs;
}

#[derive(Clone, Copy, Debug)]
/// Observation provider retaining a bounded window of the agent's own history.
pub struct OwnHistoryObs {
    window: usize,
}

impl OwnHistoryObs {
    /// Creates a provider retaining at most `window` completed own-step records.
    pub fn new(window: usize) -> Self {
        Self { window }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Own-history observation rendered by [`super::MarkdownTableRenderer`].
pub struct OwnHistory<A> {
    /// Current zero-based simulation step.
    pub t: u64,
    /// Role of this decision, or `None` for a shared per-step decision.
    pub role: Option<usize>,
    /// Up to the configured number of own records, oldest first.
    pub mine: Vec<StepRecord<A>>,
    /// Last observed action of each current opponent in opponent order.
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
/// Provider that counts the current opponents' most recently observed actions.
pub struct CountsObs;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Finite-action count observation rendered by [`super::CountsRenderer`].
pub struct ActionCounts {
    /// Role of this decision, or `None` for a shared per-step decision.
    pub role: Option<usize>,
    /// Agent's own last recorded action, if it has played before.
    pub my_last: Option<usize>,
    /// Opponent counts indexed by zero-based action; missing histories add nothing.
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
