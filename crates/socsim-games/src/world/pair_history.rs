use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PairHistory<A> {
    len: usize,
    keys: Vec<(AgentId, AgentId)>,
    rings: Vec<PairRing<A>>,
}

impl<A> PairHistory<A> {
    pub(crate) fn new(len: usize) -> Self {
        debug_assert!(len > 0);
        Self {
            len,
            keys: Vec::new(),
            rings: Vec::new(),
        }
    }

    pub(crate) fn push(
        &mut self,
        t: u64,
        first: AgentId,
        second: AgentId,
        first_action: A,
        second_action: A,
    ) {
        let (key, actions) = if first <= second {
            ((first, second), [first_action, second_action])
        } else {
            ((second, first), [second_action, first_action])
        };
        let index = match self.keys.binary_search(&key) {
            Ok(index) => index,
            Err(index) => {
                self.keys.insert(index, key);
                self.rings.insert(index, PairRing::new(self.len));
                index
            }
        };
        self.rings[index].push(PairRecord { t, actions });
    }

    pub(crate) fn view(&self, first: AgentId, second: AgentId) -> PairView<'_, A> {
        let (key, swapped) = if first <= second {
            ((first, second), false)
        } else {
            ((second, first), true)
        };
        let ring = self
            .keys
            .binary_search(&key)
            .ok()
            .map(|index| &self.rings[index]);
        PairView { ring, swapped }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PairRing<A> {
    head: usize,
    filled: usize,
    data: Vec<Option<PairRecord<A>>>,
}

impl<A> PairRing<A> {
    fn new(len: usize) -> Self {
        Self {
            head: 0,
            filled: 0,
            data: (0..len).map(|_| None).collect(),
        }
    }

    fn push(&mut self, record: PairRecord<A>) {
        self.data[self.head] = Some(record);
        self.head = (self.head + 1) % self.data.len();
        self.filled = (self.filled + 1).min(self.data.len());
    }

    fn iter(&self) -> impl Iterator<Item = &PairRecord<A>> {
        let start = if self.filled == self.data.len() {
            self.head
        } else {
            0
        };
        (0..self.filled).map(move |offset| {
            let index = (start + offset) % self.data.len();
            self.data[index]
                .as_ref()
                .expect("filled pair-history slots always contain records")
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PairRecord<A> {
    t: u64,
    actions: [A; 2],
}

/// Read-only history of encounters between one agent and one other agent.
///
/// Records are yielded from oldest to newest. Actions are oriented to the
/// first agent used to request the view, regardless of the agents' numeric
/// ordering in the stored record.
#[derive(Clone, Copy, Debug)]
pub struct PairView<'a, A> {
    ring: Option<&'a PairRing<A>>,
    swapped: bool,
}

impl<'a, A> PairView<'a, A> {
    /// Returns the number of retained encounters for this pair.
    pub fn len(&self) -> usize {
        self.ring.map_or(0, |ring| ring.filled)
    }

    /// Returns `true` when this pair has no retained encounters.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates over `(t, my_action, other_action)` from oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &'a A, &'a A)> + 'a {
        let swapped = self.swapped;
        self.ring
            .into_iter()
            .flat_map(PairRing::iter)
            .map(move |record| {
                let (mine, other) = if swapped {
                    (&record.actions[1], &record.actions[0])
                } else {
                    (&record.actions[0], &record.actions[1])
                };
                (record.t, mine, other)
            })
    }
}
