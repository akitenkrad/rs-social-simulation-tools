#![cfg_attr(not(feature = "engine"), allow(dead_code))]

use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, SocsimError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EncounterId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Credit {
    All,
    Center,
}

#[derive(Clone, Copy, Debug)]
pub struct EncounterRef<'a> {
    pub id: EncounterId,
    pub seats: &'a [AgentId],
    pub credit: Credit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayArena<A> {
    pub(crate) n_roles: usize,
    pub(crate) seats: Vec<AgentId>,
    pub(crate) credit: Vec<Credit>,
    pub(crate) actions: Vec<Option<A>>,
    pub(crate) payoffs: Vec<f64>,
}

impl<A> PlayArena<A> {
    pub(crate) fn new(n_roles: usize) -> Self {
        Self {
            n_roles,
            seats: Vec::new(),
            credit: Vec::new(),
            actions: Vec::new(),
            payoffs: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.seats.clear();
        self.credit.clear();
        self.actions.clear();
        self.payoffs.clear();
    }

    pub fn n_roles(&self) -> usize {
        self.n_roles
    }

    pub fn n_encounters(&self) -> usize {
        self.credit.len()
    }

    pub fn encounter(&self, id: EncounterId) -> Option<EncounterRef<'_>> {
        let index = usize::try_from(id.0).ok()?;
        let start = index.checked_mul(self.n_roles)?;
        let end = start.checked_add(self.n_roles)?;
        Some(EncounterRef {
            id,
            seats: self.seats.get(start..end)?,
            credit: *self.credit.get(index)?,
        })
    }

    pub fn actions(&self, id: EncounterId) -> Option<&[Option<A>]> {
        let start = usize::try_from(id.0).ok()?.checked_mul(self.n_roles)?;
        self.actions.get(start..start.checked_add(self.n_roles)?)
    }

    pub fn payoffs(&self, id: EncounterId) -> Option<&[f64]> {
        let start = usize::try_from(id.0).ok()?.checked_mul(self.n_roles)?;
        self.payoffs.get(start..start.checked_add(self.n_roles)?)
    }
}

pub struct ArenaWriter<'a, A> {
    pub(crate) arena: &'a mut PlayArena<A>,
}

impl<'a, A> ArenaWriter<'a, A> {
    pub(crate) fn new(arena: &'a mut PlayArena<A>) -> Self {
        Self { arena }
    }

    pub fn n_roles(&self) -> usize {
        self.arena.n_roles
    }

    pub fn push(&mut self, seats: &[AgentId], credit: Credit) -> socsim_core::Result<EncounterId> {
        if seats.len() != self.arena.n_roles {
            return Err(SocsimError::Mechanism(format!(
                "encounter has {} seats, expected {}",
                seats.len(),
                self.arena.n_roles
            )));
        }
        let id = u32::try_from(self.arena.credit.len())
            .map_err(|_| SocsimError::Mechanism("encounter count exceeds u32::MAX".to_owned()))?;
        self.arena.seats.extend_from_slice(seats);
        self.arena.credit.push(credit);
        self.arena
            .actions
            .extend((0..self.arena.n_roles).map(|_| None));
        self.arena
            .payoffs
            .extend((0..self.arena.n_roles).map(|_| 0.0));
        Ok(EncounterId(id))
    }
}
