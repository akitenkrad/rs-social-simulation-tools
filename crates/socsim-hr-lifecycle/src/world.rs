//! World state for the HR lifecycle ABM.
//!
//! Defines [`HrWorld`], [`Employee`], and [`Team`].

use std::collections::BTreeMap;

use rand::Rng;
use rand_distr::{Distribution, Normal};
use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, SimClock, SimRng, WorldState};
use socsim_net::SocialNetwork;

use crate::calibration::{LAMBDA_LEARN, P_TOXIC, THETA_FLOOR, THETA_MEAN, THETA_SD};

// ── Employee ──────────────────────────────────────────────────────────────────

/// Per-employee agent state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    /// True ability draw (positive scale, ~N(THETA_MEAN, THETA_SD) at hiring).
    pub theta: f64,
    /// Months of tenure in the current organisation.
    pub tenure: u32,
    /// Index into [`HrWorld::teams`].
    pub team: usize,
    /// Socialisation quality [0, 1]; set during on-boarding PostStep.
    pub socialization: f64,
    /// Job embeddedness: attachment to the current role/organisation [0, 1].
    pub embeddedness: f64,
    /// Person–organisation fit [0, 1].
    pub po_fit: f64,
    /// Person–job fit [0, 1].
    pub pj_fit: f64,
    /// Current job satisfaction [0, 1].
    pub satisfaction: f64,
    /// Whether this employee exhibits toxic behaviour.
    pub is_toxic: bool,
    /// Cumulative performance reward accumulated over all steps.
    pub cum_reward: f64,
    /// Effective productivity contribution (updated each step).
    pub productivity: f64,
    /// Number of network neighbours who quit in the *previous* month.  Drives
    /// the Krackhardt cluster-turnover cascade; reset each month after use.
    pub(crate) recent_quit_neighbors: u32,
}

impl Employee {
    /// Create a new employee at hire time.
    ///
    /// `theta` is the true ability (expected positive).  All other fields are
    /// initialised to plausible defaults that mechanisms then refine.
    ///
    /// New hires start with a higher embeddedness floor than the toxic/quit
    /// dynamics would otherwise allow, reflecting initial commitment; the
    /// `socialization` mechanism refines this for first-month hires.
    pub fn new(theta: f64, team: usize, is_toxic: bool, rng: &mut SimRng) -> Self {
        Self {
            theta,
            tenure: 0,
            team,
            socialization: 0.0,
            embeddedness: rng.gen_range(0.5_f64..0.8),
            po_fit: rng.gen_range(0.4_f64..0.8),
            pj_fit: rng.gen_range(0.4_f64..0.8),
            satisfaction: 0.6,
            is_toxic,
            cum_reward: 0.0,
            productivity: theta * (1.0 - (-LAMBDA_LEARN * 0.0_f64).exp()),
            recent_quit_neighbors: 0,
        }
    }

    /// Draw a positive true-ability value `θ ~ N(THETA_MEAN, THETA_SD)`,
    /// floored at [`THETA_FLOOR`] to stay strictly positive.
    pub(crate) fn draw_theta(normal01: &Normal<f64>, rng: &mut SimRng) -> f64 {
        let z = normal01.sample(rng);
        (THETA_MEAN + THETA_SD * z).max(THETA_FLOOR)
    }
}

// ── Team ──────────────────────────────────────────────────────────────────────

/// Aggregate team state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Team {
    /// Team knowledge stock K_team (grows via OCB, shrinks on departure).
    pub knowledge_stock: f64,
    /// Mean θ of current team members (cached; updated by `recompute_team_means`).
    pub mean_theta: f64,
}

// ── HrWorld ───────────────────────────────────────────────────────────────────

/// World state for the HR lifecycle ABM.
///
/// Holds the employee roster, team aggregate states, the inter-agent social
/// network, and the simulation clock.
#[derive(Clone, Serialize, Deserialize)]
pub struct HrWorld {
    /// Simulation clock.
    pub clock: SimClock,
    /// Aggregate state per team.
    pub teams: Vec<Team>,
    /// Live employee states keyed by [`AgentId`].
    pub employees: BTreeMap<AgentId, Employee>,
    /// Inter-employee social network (undirected).
    pub network: SocialNetwork,
    /// Aggregate organisational performance (updated each Reward phase).
    pub org_performance: f64,
    /// Counter for assigning unique [`AgentId`]s.
    pub(crate) next_id: u64,
    /// Baseline mean θ across all employees at time 0 (used in peer-effect
    /// normalisation).
    pub base_mean_theta: f64,
    /// Target headcount per team (used by the `hiring` mechanism).
    pub target_team_size: usize,
    /// Ids of employees hired during the current step (cleared at PostStep end).
    pub(crate) new_hires_this_step: Vec<AgentId>,
    /// Employees who quit during the current step (cleared at PostStep end):
    /// `(id, theta, tenure_months, team_idx)`.
    pub(crate) departed_this_step: Vec<(AgentId, f64, u32, usize)>,
    /// Headcount captured at the start of the current step's turnover phase,
    /// used as the denominator for `turnover_rate` (avoids the `prev_count = 0`
    /// artifact on the first step).
    pub(crate) headcount_at_step_start: usize,
}

impl HrWorld {
    /// Construct a new `HrWorld`.
    ///
    /// - `n_teams`: number of teams.
    /// - `team_size`: initial headcount per team.
    /// - `ws_k` / `ws_beta`: Watts–Strogatz network parameters (k=mean
    ///   degree, beta=rewiring probability).
    /// - `rng`: seeded RNG for reproducibility.
    pub fn new(
        n_teams: usize,
        team_size: usize,
        ws_k: usize,
        ws_beta: f64,
        rng: &mut SimRng,
    ) -> Self {
        let t_max = u64::MAX; // Caller sets the real t_max via SimulationBuilder.
        let clock = SimClock::new(t_max);

        let normal = Normal::<f64>::new(0.0, 1.0).expect("valid normal params");

        let mut employees = BTreeMap::new();
        let mut next_id = 0u64;
        let mut teams: Vec<Team> = (0..n_teams).map(|_| Team::default()).collect();

        for (team_idx, team) in teams.iter_mut().enumerate() {
            // Seed each team with a meaningful initial knowledge stock so the
            // baseline sits at a sane, interpretable order of magnitude.
            team.knowledge_stock = team_size as f64;

            for _ in 0..team_size {
                let theta = Employee::draw_theta(&normal, rng);
                let is_toxic = rng.gen::<f64>() < P_TOXIC;
                let id = AgentId(next_id);
                next_id += 1;
                employees.insert(id, Employee::new(theta, team_idx, is_toxic, rng));
            }
        }

        // Compute baseline mean θ — sort by key for deterministic f64 sum.
        let base_mean_theta = {
            let mut sorted: Vec<_> = employees.iter().collect();
            sorted.sort_by_key(|(id, _)| *id);
            let sum: f64 = sorted.iter().map(|(_, e)| e.theta).sum();
            let cnt = employees.len() as f64;
            if cnt > 0.0 {
                sum / cnt
            } else {
                1.0
            }
        };

        // Build all employee ids in insertion order.
        let all_ids: Vec<AgentId> = employees.keys().copied().collect();

        // Build Watts–Strogatz network over all employees.
        let network = SocialNetwork::watts_strogatz(&all_ids, ws_k, ws_beta, rng);

        let mut world = HrWorld {
            clock,
            teams,
            employees,
            network,
            org_performance: 0.0,
            next_id,
            base_mean_theta,
            target_team_size: team_size,
            new_hires_this_step: Vec::new(),
            departed_this_step: Vec::new(),
            headcount_at_step_start: n_teams * team_size,
        };

        world.recompute_team_means();
        world
    }

    // ── Helper methods ────────────────────────────────────────────────────────

    /// Return the [`AgentId`]s of employees belonging to `team_idx`, in sorted order.
    pub fn team_members(&self, team_idx: usize) -> Vec<AgentId> {
        let mut members: Vec<AgentId> = self
            .employees
            .iter()
            .filter(|(_, e)| e.team == team_idx)
            .map(|(id, _)| *id)
            .collect();
        members.sort();
        members
    }

    /// Re-derive `mean_theta` for every team from the current employee roster.
    ///
    /// Sorts by [`AgentId`] before summing for deterministic f64 results.
    pub fn recompute_team_means(&mut self) {
        for (i, team) in self.teams.iter_mut().enumerate() {
            let mut members: Vec<f64> = self
                .employees
                .iter()
                .filter(|(_, e)| e.team == i)
                .map(|(_, e)| e.theta)
                .collect();
            // Sort for determinism across hash-map iteration orders.
            members.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            team.mean_theta = if members.is_empty() {
                0.0
            } else {
                members.iter().sum::<f64>() / members.len() as f64
            };
        }
    }

    /// Allocate a fresh [`AgentId`].
    pub(crate) fn alloc_id(&mut self) -> AgentId {
        let id = AgentId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Total number of employees currently in the simulation.
    pub fn employee_count(&self) -> usize {
        self.employees.len()
    }

    /// Average tenure across all employees.
    pub fn avg_tenure(&self) -> f64 {
        if self.employees.is_empty() {
            return 0.0;
        }
        let sum: u32 = self.employees.values().map(|e| e.tenure).sum();
        sum as f64 / self.employees.len() as f64
    }

    /// Sum of knowledge stocks across all teams.
    pub fn total_knowledge_stock(&self) -> f64 {
        self.teams.iter().map(|t| t.knowledge_stock).sum()
    }
}

// ── WorldState impl ───────────────────────────────────────────────────────────

impl WorldState for HrWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        let mut ids: Vec<AgentId> = self.employees.keys().copied().collect();
        ids.sort();
        ids
    }

    fn clock(&self) -> &SimClock {
        &self.clock
    }

    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}
