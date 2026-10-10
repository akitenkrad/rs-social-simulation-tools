#![cfg_attr(not(feature = "engine"), allow(dead_code))]

use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepRecord<A> {
    pub action: Option<A>,
    pub payoff: f64,
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

    pub(crate) fn push_step(&mut self, records: &[StepRecord<A>]) {
        debug_assert_eq!(records.len(), self.n_agents);
        for (agent, record) in records.iter().enumerate() {
            self.data[agent * self.history_len + self.head] = record.clone();
        }
        self.head = (self.head + 1) % self.history_len;
        self.filled = (self.filled + 1).min(self.history_len);
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

#[derive(Clone, Copy, Debug)]
pub struct HistoryView<'a, T> {
    front: &'a [T],
    back: &'a [T],
}

impl<'a, T> HistoryView<'a, T> {
    pub fn len(&self) -> usize {
        self.front.len() + self.back.len()
    }

    pub fn is_empty(&self) -> bool {
        self.front.is_empty() && self.back.is_empty()
    }

    pub fn as_slices(&self) -> (&'a [T], &'a [T]) {
        (self.front, self.back)
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a T> + 'a {
        self.front.iter().chain(self.back.iter())
    }

    pub fn last(&self) -> Option<&'a T> {
        self.back.last().or_else(|| self.front.last())
    }
}

pub struct PeerHistory<'a, A> {
    view: HistoryView<'a, StepRecord<A>>,
}

impl<'a, A> PeerHistory<'a, A> {
    pub(crate) fn new(view: HistoryView<'a, StepRecord<A>>) -> Self {
        Self { view }
    }

    pub fn len(&self) -> usize {
        self.view.len()
    }

    pub fn is_empty(&self) -> bool {
        self.view.is_empty()
    }

    pub fn actions(&self) -> impl Iterator<Item = Option<&'a A>> + 'a {
        self.view.iter().map(|record| record.action.as_ref())
    }

    pub fn last(&self) -> Option<Option<&'a A>> {
        self.view.last().map(|record| record.action.as_ref())
    }
}
