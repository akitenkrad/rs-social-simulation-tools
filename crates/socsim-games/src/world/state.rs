use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

/// Experiment-specific state stored in [`crate::GameWorld`].
pub trait ExperimentState: Send + Sync + 'static {
    type PolicyState: Clone + Send + Sync + Debug + 'static;
    type View<'a>
    where
        Self: 'a;

    fn view_for(&self, me: AgentId) -> Self::View<'_>;
}

impl ExperimentState for () {
    type PolicyState = BuiltinPolicyState;
    type View<'a> = ();

    fn view_for(&self, _me: AgentId) -> Self::View<'_> {}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stateless;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TftState {
    pub last_opponent: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum BuiltinPolicyState {
    Stateless(Stateless),
    TitForTat(TftState),
}

pub trait Slot<T> {
    fn slot(&self) -> Option<&T>;
    fn slot_mut(&mut self) -> Option<&mut T>;
    fn from_slot(value: T) -> Self;
}

impl Slot<Stateless> for BuiltinPolicyState {
    fn slot(&self) -> Option<&Stateless> {
        match self {
            Self::Stateless(value) => Some(value),
            Self::TitForTat(_) => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut Stateless> {
        match self {
            Self::Stateless(value) => Some(value),
            Self::TitForTat(_) => None,
        }
    }

    fn from_slot(value: Stateless) -> Self {
        Self::Stateless(value)
    }
}

impl Slot<TftState> for BuiltinPolicyState {
    fn slot(&self) -> Option<&TftState> {
        match self {
            Self::TitForTat(value) => Some(value),
            Self::Stateless(_) => None,
        }
    }

    fn slot_mut(&mut self) -> Option<&mut TftState> {
        match self {
            Self::TitForTat(value) => Some(value),
            Self::Stateless(_) => None,
        }
    }

    fn from_slot(value: TftState) -> Self {
        Self::TitForTat(value)
    }
}
