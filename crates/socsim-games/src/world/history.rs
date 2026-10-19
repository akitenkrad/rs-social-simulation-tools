#![cfg_attr(not(feature = "engine"), allow(dead_code))]

use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// One agent's aggregate action and payoff record for a completed step.
pub struct StepRecord<A> {
    /// Action chosen during the step, or `None` when the agent did not decide.
    pub action: Option<A>,
    /// Sum or mean of credited encounter payoffs according to play options.
    pub payoff: f64,
    /// Number of credited encounters contributing to `payoff`.
    pub n_played: u32,
}

/// A flat agent-by-slot ring with one shared write position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StepHistory<A> {
    history_len: usize,
    n_agents: usize,
    head: usize,
    filled: usize,
    data: Vec<StepRecord<A>>,
}

impl<A: Clone> StepHistory<A> {
    pub(crate) fn new(n_agents: usize, history_len: usize) -> Self {
        let empty = StepRecord {
            action: None,
            payoff: 0.0,
            n_played: 0,
        };
        Self {
            history_len,
            n_agents,
            head: 0,
            filled: 0,
            data: vec![empty; n_agents.saturating_mul(history_len)],
        }
    }

    /// Returns a writer for the slot that the next step occupies (the oldest
    /// slot once the ring is full). Every row must be written before
    /// `commit`, which makes the slot the latest step.
    pub(crate) fn next_slot_mut(&mut self) -> NextSlot<'_, A> {
        NextSlot { history: self }
    }

    pub(crate) fn view(&self, agent: AgentId) -> Option<HistoryView<'_, StepRecord<A>>> {
        let agent = usize::try_from(agent.0).ok()?;
        if agent >= self.n_agents {
            return None;
        }
        let row = &self.data[agent * self.history_len..(agent + 1) * self.history_len];
        let (front, back) = if self.filled < self.history_len {
            (&row[..self.filled], &row[0..0])
        } else {
            (&row[self.head..], &row[..self.head])
        };
        Some(HistoryView { front, back })
    }
}

/// In-place writer of one step of a [`StepHistory`].
pub(crate) struct NextSlot<'a, A> {
    history: &'a mut StepHistory<A>,
}

impl<A> NextSlot<'_, A> {
    /// The record of row `agent` in the slot being written.
    pub(crate) fn get(&mut self, agent: usize) -> &mut StepRecord<A> {
        let history = &mut *self.history;
        &mut history.data[agent * history.history_len + history.head]
    }

    /// Makes the written slot the latest step.
    pub(crate) fn commit(self) {
        let history = self.history;
        history.head = (history.head + 1) % history.history_len;
        history.filled = (history.filled + 1).min(history.history_len);
    }
}

#[derive(Clone, Copy, Debug)]
/// Chronological borrowed view over a possibly wrapped history ring.
pub struct HistoryView<'a, T> {
    front: &'a [T],
    back: &'a [T],
}

impl<'a, T> HistoryView<'a, T> {
    /// Returns the number of retained records.
    pub fn len(&self) -> usize {
        self.front.len() + self.back.len()
    }

    /// Returns `true` when no completed record is retained.
    pub fn is_empty(&self) -> bool {
        self.front.is_empty() && self.back.is_empty()
    }

    /// Returns the one or two chronological slices that form this view.
    pub fn as_slices(&self) -> (&'a [T], &'a [T]) {
        (self.front, self.back)
    }

    /// Iterates from the oldest retained record to the newest.
    pub fn iter(&self) -> impl Iterator<Item = &'a T> + 'a {
        self.front.iter().chain(self.back.iter())
    }

    /// Returns the newest retained record.
    pub fn last(&self) -> Option<&'a T> {
        self.back.last().or_else(|| self.front.last())
    }
}

/// Action-only projection of another agent's retained step history.
pub struct PeerHistory<'a, A> {
    view: HistoryView<'a, StepRecord<A>>,
}

impl<'a, A> PeerHistory<'a, A> {
    pub(crate) fn new(view: HistoryView<'a, StepRecord<A>>) -> Self {
        Self { view }
    }

    /// Returns the number of retained steps.
    pub fn len(&self) -> usize {
        self.view.len()
    }

    /// Returns `true` when the peer has no retained records.
    pub fn is_empty(&self) -> bool {
        self.view.is_empty()
    }

    /// Iterates peer actions oldest first; idle steps yield `None`.
    pub fn actions(&self) -> impl Iterator<Item = Option<&'a A>> + 'a {
        self.view.iter().map(|record| record.action.as_ref())
    }

    /// Returns the newest retained action, distinguishing no record from an idle step.
    pub fn last(&self) -> Option<Option<&'a A>> {
        self.view.last().map(|record| record.action.as_ref())
    }
}
