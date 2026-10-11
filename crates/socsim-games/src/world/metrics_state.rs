use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct MetricsState {
    pub(crate) prev: Option<Vec<f64>>,
    pub(crate) below: u32,
    pub(crate) cumulative: Option<Vec<Vec<u64>>>,
}

impl MetricsState {
    pub(crate) fn new(cumulative: Option<Vec<Vec<u64>>>) -> Self {
        Self {
            prev: None,
            below: 0,
            cumulative,
        }
    }
}
