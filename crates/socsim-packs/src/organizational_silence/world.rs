//! World state for the organizational-silence ABM.
//!
//! Defines [`SilenceWorld`], [`Employee`], [`Team`], [`Expression`], and
//! [`Motive`].  See §4.1–4.3 of `組織的沈黙のLLM-Agentシミュレーション設計.md`
//! for the conceptual model.

use std::collections::BTreeMap;

use rand::Rng;
use rand_distr::{Distribution, Normal};
use serde::{Deserialize, Serialize};
use socsim_core::{AgentId, SimClock, SimRng, WorldState};
use socsim_net::SocialNetwork;

use crate::organizational_silence::calibration::{
    F_MEAN, F_SD, PSAFETY_MEAN, PSAFETY_SD, SIGMA_BASE, THETA_VOICE_MEAN, THETA_VOICE_SD,
};

// ── Expression / Motive enums ────────────────────────────────────────────────

/// Public expression channel for an [`Employee`] at a given step.
///
/// `Neutral` is the initial state and represents an agent who has not yet
/// committed to either voice or silence in the current step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Expression {
    /// Speaks up: makes the private concern publicly known.
    Voice,
    /// Stays silent despite holding a private concern.
    Silence,
    /// Default before any decision is made for the step.
    Neutral,
}

/// The motive behind a silence choice, following Van Dyne, Ang & Botero (2003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Motive {
    /// "It won't change anything" — resignation / disengagement.
    Acquiescent,
    /// "Speaking up is too risky" — fear-driven.
    Defensive,
    /// "Speaking up would hurt others" — other-protective.
    Prosocial,
}

// ── Employee ─────────────────────────────────────────────────────────────────

/// Per-employee agent state.
///
/// Field meanings follow §4.1 of the design doc.  All numeric fields are
/// scaled to either `[0, 1]` (probabilities, intensities) or `[-1, 1]`
/// (private concerns).  Determinism: every mechanism that iterates over
/// employees must sort by [`AgentId`] before any RNG draw or `f64` sum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    /// Hierarchy level: 1 = frontline … L = executive.
    pub level: u8,
    /// Months of tenure in the organisation.
    pub tenure: u32,
    /// Index into [`SilenceWorld::teams`].
    pub team: usize,
    /// Private concern `b_i ∈ [-1, 1]`.  Negative = critical of the status
    /// quo (the value that turns silence into "concealed dissent").
    pub private_concern: f64,
    /// Current public expression.
    pub expression: Expression,
    /// Fear-trait `f_i ∈ [0, 1]` (Kish-Gephart et al. 2009).
    pub fear: f64,
    /// Perceived psychological safety `ψ_i ∈ [0, 1]` (Edmondson 1999).
    pub psych_safety: f64,
    /// Implicit voice theory strength `ι_i ∈ [0, 1]` (Detert & Edmondson 2011).
    pub ivt_strength: f64,
    /// Motive assigned by the decision mechanism on a `Silence` choice.
    pub silence_motive: Option<Motive>,
    /// Voice threshold `θ_i ∈ [0, 1]` (Kuran 1995).  When the neighbour voice
    /// ratio exceeds this, a silent-with-negative-concern agent flips to
    /// `Voice` in the preference-falsification cascade.
    pub voice_threshold: f64,
    /// Snapshot of the neighbour silence ratio `ρ_i ∈ [0, 1]` taken by
    /// `SilenceSpiralMechanism` at the end of step `t`; read by the next
    /// step's `VoiceDecision*Mechanism` so updates within a phase are
    /// synchronous.
    pub(crate) neighbor_silence_ratio: f64,
}

impl Employee {
    /// Draw an [`Employee`] with priors taken from `calibration`.
    ///
    /// `ivt_strength` is drawn `U[0, 1]` (a Beta-like prior could be plugged
    /// in later; the design doc lists this as tunable).  All other fields use
    /// truncated-normal priors so the realised values stay in their domains.
    ///
    /// `private_concern ~ N(0, 0.3)` so most agents are nearly neutral, with
    /// a long-tailed minority of strongly critical/positive members.
    fn new(
        level: u8,
        team: usize,
        normal01: &Normal<f64>,
        rng: &mut SimRng,
    ) -> Self {
        let private_concern = {
            let z: f64 = normal01.sample(rng);
            (0.3 * z).clamp(-1.0, 1.0)
        };
        let fear = (F_MEAN + F_SD * normal01.sample(rng)).clamp(0.0, 1.0);
        let psych_safety =
            (PSAFETY_MEAN + PSAFETY_SD * normal01.sample(rng)).clamp(0.0, 1.0);
        let ivt_strength = rng.gen::<f64>();
        let voice_threshold =
            (THETA_VOICE_MEAN + THETA_VOICE_SD * normal01.sample(rng)).clamp(0.0, 1.0);

        Self {
            level,
            tenure: 0,
            team,
            private_concern,
            expression: Expression::Neutral,
            fear,
            psych_safety,
            ivt_strength,
            silence_motive: None,
            voice_threshold,
            neighbor_silence_ratio: 0.0,
        }
    }
}

// ── Team ─────────────────────────────────────────────────────────────────────

/// Aggregate state for one team in the organisation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    /// Supervisor openness `u_k ∈ [-1, 1]`.  Positive = the supervisor
    /// visibly welcomes voicing concerns; negative = penalises it.
    pub supervisor_openness: f64,
    /// Team knowledge stock (grows from voiced concerns + double-loop
    /// learning, decays in silent climates).
    pub knowledge_stock: f64,
}

// ── SilenceWorld ─────────────────────────────────────────────────────────────

/// World state for the organizational-silence ABM.
///
/// Holds the employee roster, team aggregates, the inter-employee social
/// network, the simulation clock, and four macro variables tracked across
/// the run.  `Clone`/`Serialize`/`Deserialize` are derived so
/// [`Snapshot<SilenceWorld>`](socsim_engine::Snapshot) save/resume works
/// out of the box; `Debug` is derived for ad-hoc introspection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SilenceWorld {
    /// Simulation clock.
    pub clock: SimClock,
    /// Aggregate state per team.
    pub teams: Vec<Team>,
    /// Live employee states keyed by [`AgentId`].
    pub employees: BTreeMap<AgentId, Employee>,
    /// Inter-employee social network (Watts–Strogatz small-world).
    pub network: SocialNetwork,
    /// Issue salience σ(t) ∈ [0, 1].
    pub issue_salience: f64,
    /// Climate of silence C(t): fraction of agents who are silent and hold a
    /// negative private concern.
    pub climate_of_silence: f64,
    /// Voice volume V(t): fraction of agents currently expressing Voice.
    pub voice_volume: f64,
    /// Aggregate organisational performance Π(t).
    pub org_performance: f64,
    /// Agents who experienced retaliation this step (cleared at PostStep end).
    pub(crate) retaliation_this_step: Vec<AgentId>,
}

impl SilenceWorld {
    /// Build a new [`SilenceWorld`].
    ///
    /// Arguments:
    /// - `n_teams`: number of teams.
    /// - `team_size`: initial headcount per team.
    /// - `n_levels`: hierarchy depth `L`; employees are assigned to levels
    ///   `1..=n_levels` round-robin so each level has roughly equal mass.
    /// - `ws_k`, `ws_beta`: Watts–Strogatz parameters (mean degree, rewire
    ///   probability).
    /// - `supervisor_homogeneity`: η ∈ [0, 1].  At η=1 every team has the
    ///   same supervisor openness (the population mean ≈ 0); at η=0 supervisor
    ///   openness is spread uniformly in `[-1, 1]`.  Intermediate η linearly
    ///   blends the two extremes.
    /// - `rng`: seeded RNG for reproducibility.
    pub fn new(
        n_teams: usize,
        team_size: usize,
        n_levels: u8,
        ws_k: usize,
        ws_beta: f64,
        supervisor_homogeneity: f64,
        rng: &mut SimRng,
    ) -> Self {
        let t_max = u64::MAX; // Caller overrides via SimulationBuilder.
        let clock = SimClock::new(t_max);

        let normal = Normal::<f64>::new(0.0, 1.0).expect("valid normal params");

        // Build teams with supervisor openness blended between a common-mean
        // baseline (η = 1) and a U[-1, 1] spread (η = 0).
        let homogeneity = supervisor_homogeneity.clamp(0.0, 1.0);
        let mut teams: Vec<Team> = Vec::with_capacity(n_teams);
        for _ in 0..n_teams {
            let spread: f64 = rng.gen_range(-1.0_f64..=1.0);
            let openness = (1.0 - homogeneity) * spread; // η=1 ⇒ 0 (common mean)
            teams.push(Team {
                supervisor_openness: openness.clamp(-1.0, 1.0),
                knowledge_stock: team_size as f64,
            });
        }

        // Populate employees deterministically: each team in order, levels
        // round-robined within the team so the mass at each level is even.
        let mut employees = BTreeMap::new();
        let mut next_id = 0u64;
        let n_levels = n_levels.max(1);
        for team_idx in 0..n_teams {
            for slot in 0..team_size {
                let level = (slot as u8 % n_levels) + 1;
                let id = AgentId(next_id);
                next_id += 1;
                employees.insert(id, Employee::new(level, team_idx, &normal, rng));
            }
        }

        // Build the Watts–Strogatz small-world network.
        let all_ids: Vec<AgentId> = employees.keys().copied().collect();
        let network = SocialNetwork::watts_strogatz(&all_ids, ws_k, ws_beta, rng);

        SilenceWorld {
            clock,
            teams,
            employees,
            network,
            issue_salience: SIGMA_BASE,
            climate_of_silence: 0.0,
            voice_volume: 0.0,
            org_performance: 0.0,
            retaliation_this_step: Vec::new(),
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────

    /// Return the [`AgentId`]s of employees in `team_idx`, sorted.
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

    /// Fraction of `id`'s network neighbours currently expressing `Silence`.
    ///
    /// Returns 0.0 if `id` has no neighbours (avoids NaN).
    pub fn neighbor_silence_ratio(&self, id: AgentId) -> f64 {
        let neighbours = self.network.neighbors(id);
        if neighbours.is_empty() {
            return 0.0;
        }
        let mut silent = 0usize;
        let mut total = 0usize;
        for nb in neighbours {
            if let Some(emp) = self.employees.get(&nb) {
                if emp.expression == Expression::Silence {
                    silent += 1;
                }
                total += 1;
            }
        }
        if total == 0 {
            0.0
        } else {
            silent as f64 / total as f64
        }
    }

    /// Fraction of `id`'s network neighbours currently expressing `Voice`.
    pub fn neighbor_voice_ratio(&self, id: AgentId) -> f64 {
        let neighbours = self.network.neighbors(id);
        if neighbours.is_empty() {
            return 0.0;
        }
        let mut voicers = 0usize;
        let mut total = 0usize;
        for nb in neighbours {
            if let Some(emp) = self.employees.get(&nb) {
                if emp.expression == Expression::Voice {
                    voicers += 1;
                }
                total += 1;
            }
        }
        if total == 0 {
            0.0
        } else {
            voicers as f64 / total as f64
        }
    }

    /// Current fraction of employees in `Silence`.
    pub fn silence_rate(&self) -> f64 {
        if self.employees.is_empty() {
            return 0.0;
        }
        let silent = self
            .employees
            .values()
            .filter(|e| e.expression == Expression::Silence)
            .count();
        silent as f64 / self.employees.len() as f64
    }

    /// Current fraction of employees in `Voice` (V(t)).
    pub fn voice_volume_now(&self) -> f64 {
        if self.employees.is_empty() {
            return 0.0;
        }
        let voicers = self
            .employees
            .values()
            .filter(|e| e.expression == Expression::Voice)
            .count();
        voicers as f64 / self.employees.len() as f64
    }

    /// Sum of `knowledge_stock` across all teams.
    pub fn total_knowledge_stock(&self) -> f64 {
        // Teams are in insertion order, so a fold here is already deterministic.
        self.teams.iter().map(|t| t.knowledge_stock).sum()
    }

    /// Re-derive `climate_of_silence`, `voice_volume`, and `org_performance`
    /// from the current employee roster.
    ///
    /// Deterministic: iterates `employees` in BTreeMap order (sorted by
    /// [`AgentId`]) before summing.
    pub fn recompute_macro_aggregates(&mut self) {
        let n = self.employees.len();
        if n == 0 {
            self.climate_of_silence = 0.0;
            self.voice_volume = 0.0;
            return;
        }
        // BTreeMap iteration is sorted by key, so this is deterministic.
        let mut concealed_dissent = 0usize;
        let mut voicers = 0usize;
        for emp in self.employees.values() {
            if emp.expression == Expression::Silence && emp.private_concern < 0.0 {
                concealed_dissent += 1;
            }
            if emp.expression == Expression::Voice {
                voicers += 1;
            }
        }
        self.climate_of_silence = concealed_dissent as f64 / n as f64;
        self.voice_volume = voicers as f64 / n as f64;
    }
}

// ── WorldState impl ──────────────────────────────────────────────────────────

impl WorldState for SilenceWorld {
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
