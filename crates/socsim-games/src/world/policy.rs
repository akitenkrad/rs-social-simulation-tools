use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RuleKindId(pub u32);

/// Policy state for active agents. Held agents have no state slot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyTable<S> {
    pub(crate) kinds: Vec<String>,
    pub(crate) slot: Vec<u32>,
    pub(crate) kind_of: Vec<RuleKindId>,
    pub(crate) states: Vec<S>,
}

impl<S> PolicyTable<S> {
    pub(crate) const HELD: u32 = u32::MAX;

    pub(crate) fn new(
        kinds: Vec<String>,
        slot: Vec<u32>,
        kind_of: Vec<RuleKindId>,
        states: Vec<S>,
    ) -> Self {
        Self {
            kinds,
            slot,
            kind_of,
            states,
        }
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    pub fn state(&self, agent: AgentId) -> Option<&S> {
        let state = *self.slot.get(agent.0 as usize)?;
        (state != Self::HELD).then(|| &self.states[state as usize])
    }

    pub fn kind(&self, agent: AgentId) -> Option<&str> {
        let state = *self.slot.get(agent.0 as usize)?;
        if state == Self::HELD {
            return None;
        }
        let kind = self.kind_of.get(state as usize)?;
        self.kinds.get(kind.0 as usize).map(String::as_str)
    }
}
