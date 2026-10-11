use serde::{Deserialize, Serialize};

/// One retained row in the aggregate-history ring.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AggregateRow {
    t: u64,
    values: Vec<Option<f64>>,
}

/// Aggregate values retained in chronological order by a bounded ring.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct AggregateTable {
    names: Vec<String>,
    len: usize,
    head: usize,
    filled: usize,
    rows: Vec<AggregateRow>,
}

impl AggregateTable {
    pub(crate) fn new(names: Vec<String>, len: usize) -> Self {
        debug_assert!(len > 0);
        Self {
            names,
            len,
            head: 0,
            filled: 0,
            rows: Vec::with_capacity(len),
        }
    }

    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }

    pub(crate) fn filled(&self) -> usize {
        self.filled
    }

    pub(crate) fn push(&mut self, t: u64, values: Vec<Option<f64>>) {
        debug_assert_eq!(values.len(), self.names.len());
        let row = AggregateRow { t, values };
        if self.filled < self.len {
            self.rows.push(row);
            self.filled += 1;
        } else {
            self.rows[self.head] = row;
        }
        self.head = (self.head + 1) % self.len;
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, &[Option<f64>])> {
        let start = if self.filled == self.len {
            self.head
        } else {
            0
        };
        (0..self.filled).map(move |offset| {
            let row = &self.rows[(start + offset) % self.len];
            (row.t, row.values.as_slice())
        })
    }

    pub(crate) fn last(&self) -> Option<(u64, &[Option<f64>])> {
        self.filled.checked_sub(1).map(|offset| {
            let start = if self.filled == self.len {
                self.head
            } else {
                0
            };
            let row = &self.rows[(start + offset) % self.len];
            (row.t, row.values.as_slice())
        })
    }
}
