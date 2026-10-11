use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PolicyReports {
    t: u64,
    n_roles: usize,
    mixed: Vec<Option<Vec<f64>>>,
}

impl PolicyReports {
    pub(crate) fn new(t: u64, n_agents: usize, n_roles: usize) -> Option<Self> {
        let len = n_agents.checked_mul(n_roles)?;
        Some(Self {
            t,
            n_roles,
            mixed: vec![None; len],
        })
    }

    pub(crate) fn t(&self) -> u64 {
        self.t
    }

    pub(crate) fn reset(&mut self, t: u64) {
        self.t = t;
        self.mixed.fill(None);
    }

    pub(crate) fn set(&mut self, agent: usize, role: usize, value: Option<Vec<f64>>) -> bool {
        if role >= self.n_roles {
            return false;
        }
        let Some(index) = agent
            .checked_mul(self.n_roles)
            .and_then(|base| base.checked_add(role))
        else {
            return false;
        };
        let Some(slot) = self.mixed.get_mut(index) else {
            return false;
        };
        *slot = value;
        true
    }

    pub(crate) fn get(&self, agent: usize, role: usize) -> Option<&[f64]> {
        if role >= self.n_roles {
            return None;
        }
        let index = agent.checked_mul(self.n_roles)?.checked_add(role)?;
        self.mixed.get(index)?.as_deref()
    }
}
