//! General, reusable mechanism catalog for `socsim`.
//!
//! This is `socsim`'s **general** (domain-agnostic) mechanism catalog.  Where
//! crates such as `socsim-hr-lifecycle` model one specific scenario, this crate
//! provides reusable *building blocks* that operate over any world implementing
//! the capability traits from `socsim-core`
//! ([`ScalarOpinions`](socsim_core::ScalarOpinions),
//! [`BinaryState`](socsim_core::BinaryState),
//! [`CultureVectors`](socsim_core::CultureVectors), each paired with
//! [`Neighbors`](socsim_core::Neighbors)).
//!
//! The catalog is split into three Cargo **feature families** (all on by
//! default): `opinion-dynamics`, `contagion` and `cultural`.  Disable default
//! features and opt in to compile only the families you need.
//!
//! Three mechanism families ship here:
//!
//! - **Opinion dynamics** ([`opinion`] module) — scalar-opinion updates:
//!   - [`HegselmannKrauseMechanism`] — Hegselmann–Krause bounded confidence
//!     (synchronous mean of the ε-confidence set).
//!   - [`DeffuantMechanism`] — Deffuant pairwise bounded confidence.
//!   - [`SocialJudgementMechanism`] — assimilation/repulsion (polarising); math
//!     from `mou2024`.
//!   - [`LorenzMechanism`] — assimilation + polarisation; math from `mou2024`.
//! - **Network contagion** ([`contagion`] module) — binary-state diffusion:
//!   - [`SiContagionMechanism`] — SI per-edge β infection; math from
//!     `granovetter1973`.
//!   - [`ThresholdContagionMechanism`] — Granovetter (1978) threshold; math from
//!     `granovetter1973`.
//! - **Cultural dissemination** ([`culture`] module):
//!   - [`AxelrodMechanism`] — Axelrod (1997) feature copying; math from
//!     `wang2025`.
//!
//! For hybrid models (e.g. `mou2024`, an LLM core + ABM periphery) the bare
//! message-set **Δ-form** opinion updates ([`updates`] module —
//! [`bounded_confidence_update`], [`hk_update`], [`social_judgement_update`],
//! [`lorenz_update`]) are exposed directly: they take a `messages: &[f64]` set
//! and return a delta `Δa` the caller clamps onto `a_i`.  These are ported
//! byte-for-byte from `mou2024` and are distinct from the standalone mechanisms
//! above (the SJ / Lorenz mechanisms route through them as the single source of
//! truth).
//!
//! Convergence for the opinion family is decided by the **driver / world**,
//! matching the `hegselmann2005` reference; this crate offers the free helper
//! [`max_abs_delta`] and an optional [`ConvergenceMechanism`] (a `PostStep`
//! mechanism that calls `request_stop` once `max|Δx| < tol`).  The contagion
//! mechanisms self-stop on saturation; the Axelrod mechanism's absorbing state
//! can be tested with [`culture::is_absorbing`].
//!
//! # Usage (library mode)
//! ```ignore
//! use socsim_mechanisms::{HegselmannKrauseMechanism, MeanOperator};
//! let hk = HegselmannKrauseMechanism::new(0.2, MeanOperator::Arithmetic);
//! // register `hk` with the engine for a world: ScalarOpinions + Neighbors
//! ```

#[cfg(feature = "contagion")]
pub mod contagion;
#[cfg(feature = "cultural")]
pub mod culture;
#[cfg(feature = "opinion-dynamics")]
pub mod means;
#[cfg(feature = "opinion-dynamics")]
pub mod opinion;
#[cfg(feature = "opinion-dynamics")]
pub mod updates;

#[cfg(feature = "contagion")]
pub use contagion::{SiContagionMechanism, ThresholdContagionMechanism};
#[cfg(feature = "cultural")]
pub use culture::{axelrod_event, is_absorbing, AxelrodMechanism};
#[cfg(feature = "opinion-dynamics")]
pub use means::{apply_mean, parse_mean, MeanOperator};
#[cfg(feature = "opinion-dynamics")]
pub use opinion::{
    DeffuantMechanism, HegselmannKrauseMechanism, LorenzMechanism, SocialJudgementMechanism,
};
#[cfg(feature = "opinion-dynamics")]
pub use updates::{
    bounded_confidence_update, clamp_attitude, f_message, hk_update, lorenz_update,
    social_judgement_update,
};

use socsim_core::{Mechanism, Phase, Result, ScalarOpinions, StepContext};

// ── Convergence helpers ─────────────────────────────────────────────────────

/// Maximum absolute element-wise difference between two opinion snapshots.
///
/// Returns `+∞` if the slices differ in length (treated as "not converged").
/// A driver can stop the run once `max_abs_delta(prev, curr) < tol`.
pub fn max_abs_delta(prev: &[f64], curr: &[f64]) -> f64 {
    if prev.len() != curr.len() {
        return f64::INFINITY;
    }
    prev.iter()
        .zip(curr.iter())
        .map(|(&a, &b)| (a - b).abs())
        .fold(0.0_f64, f64::max)
}

/// Optional `PostStep` mechanism that stops the run once opinions stop moving.
///
/// It keeps the previous step's opinion snapshot internally; each `PostStep` it
/// compares the current opinions to that snapshot and, if `max|Δx| < tol`, calls
/// [`StepContext::request_stop`].  This mirrors the `hegselmann2005` convergence
/// idiom while leaving the stop decision in a (generic, driver-side) mechanism
/// rather than baked into the world.
///
/// Note: meaningful only for deterministic updates (e.g. HK with A/G/H/P).  A
/// stochastic update (the random mean, or Deffuant) need not reach a fixed
/// point, so pairing this with such a mechanism may stop early or never.
#[derive(Clone, Debug, Default)]
pub struct ConvergenceMechanism {
    /// Convergence tolerance: stop when `max|Δx| < tol`.
    pub tol: f64,
    /// Previous-step opinion snapshot (`None` until the first `PostStep`).
    prev: Option<Vec<f64>>,
}

impl ConvergenceMechanism {
    /// Create a convergence checker with tolerance `tol`.
    pub fn new(tol: f64) -> Self {
        Self { tol, prev: None }
    }
}

impl<W: ScalarOpinions> Mechanism<W> for ConvergenceMechanism {
    fn name(&self) -> &str {
        "convergence"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, W>) -> Result<()> {
        let curr: Vec<f64> = ctx
            .world
            .agent_ids()
            .iter()
            .map(|&id| ctx.world.opinion(id))
            .collect();

        if let Some(prev) = &self.prev {
            if max_abs_delta(prev, &curr) < self.tol {
                ctx.request_stop();
            }
        }
        self.prev = Some(curr);
        Ok(())
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use socsim_core::{
        AgentId, BinaryState, Blackboard, CultureVectors, Neighbors, NullRecorder, ScalarOpinions,
        SimClock, SimRng, StepContext, WorldState,
    };

    // ── topology ─────────────────────────────────────────────────────────────

    #[derive(Clone)]
    enum Topology {
        /// Every agent sees every *other* agent (non-spatial complete graph).
        Complete,
        /// Explicit per-agent adjacency (e.g. a ring or a grid).
        Adjacency(Vec<Vec<usize>>),
    }

    fn ring_adj(n: usize) -> Vec<Vec<usize>> {
        (0..n).map(|i| vec![(i + n - 1) % n, (i + 1) % n]).collect()
    }

    /// 4-neighbour (von Neumann) torus grid adjacency for a `w × h` lattice.
    fn grid_adj(w: usize, h: usize) -> Vec<Vec<usize>> {
        let idx = |x: usize, y: usize| y * w + x;
        let mut adj = vec![Vec::new(); w * h];
        for y in 0..h {
            for x in 0..w {
                let me = idx(x, y);
                adj[me].push(idx((x + w - 1) % w, y));
                adj[me].push(idx((x + 1) % w, y));
                adj[me].push(idx(x, (y + h - 1) % h));
                adj[me].push(idx(x, (y + 1) % h));
            }
        }
        adj
    }

    fn neighbors(topology: &Topology, n: usize, id: AgentId) -> Vec<AgentId> {
        match topology {
            Topology::Complete => (0..n as u64).map(AgentId).filter(|&j| j != id).collect(),
            Topology::Adjacency(adj) => adj[id.0 as usize]
                .iter()
                .map(|&j| AgentId(j as u64))
                .collect(),
        }
    }

    // ── opinion fixture world ─────────────────────────────────────────────────

    struct OpinionWorld {
        clock: SimClock,
        opinions: Vec<f64>,
        topology: Topology,
    }

    impl OpinionWorld {
        fn complete(opinions: Vec<f64>) -> Self {
            Self {
                clock: SimClock::new(10_000),
                opinions,
                topology: Topology::Complete,
            }
        }
        fn ring(opinions: Vec<f64>) -> Self {
            let n = opinions.len();
            Self {
                clock: SimClock::new(10_000),
                opinions,
                topology: Topology::Adjacency(ring_adj(n)),
            }
        }
    }

    impl WorldState for OpinionWorld {
        fn agent_ids(&self) -> Vec<AgentId> {
            (0..self.opinions.len() as u64).map(AgentId).collect()
        }
        fn clock(&self) -> &SimClock {
            &self.clock
        }
        fn clock_mut(&mut self) -> &mut SimClock {
            &mut self.clock
        }
    }
    impl ScalarOpinions for OpinionWorld {
        fn opinion(&self, id: AgentId) -> f64 {
            self.opinions[id.0 as usize]
        }
        fn set_opinion(&mut self, id: AgentId, value: f64) {
            self.opinions[id.0 as usize] = value;
        }
    }
    impl Neighbors for OpinionWorld {
        fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> {
            neighbors(&self.topology, self.opinions.len(), id)
        }
    }

    // ── binary-state fixture world (contagion) ─────────────────────────────────

    struct ContagionWorld {
        clock: SimClock,
        active: Vec<bool>,
        topology: Topology,
    }

    impl ContagionWorld {
        fn complete(active: Vec<bool>) -> Self {
            Self {
                clock: SimClock::new(10_000),
                active,
                topology: Topology::Complete,
            }
        }
        fn ring(active: Vec<bool>) -> Self {
            let n = active.len();
            Self {
                clock: SimClock::new(10_000),
                active,
                topology: Topology::Adjacency(ring_adj(n)),
            }
        }
        fn n_active(&self) -> usize {
            self.active.iter().filter(|&&a| a).count()
        }
    }

    impl WorldState for ContagionWorld {
        fn agent_ids(&self) -> Vec<AgentId> {
            (0..self.active.len() as u64).map(AgentId).collect()
        }
        fn clock(&self) -> &SimClock {
            &self.clock
        }
        fn clock_mut(&mut self) -> &mut SimClock {
            &mut self.clock
        }
    }
    impl BinaryState for ContagionWorld {
        fn is_active(&self, id: AgentId) -> bool {
            self.active[id.0 as usize]
        }
        fn set_active(&mut self, id: AgentId, active: bool) {
            self.active[id.0 as usize] = active;
        }
    }
    impl Neighbors for ContagionWorld {
        fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> {
            neighbors(&self.topology, self.active.len(), id)
        }
    }

    // ── culture fixture world (Axelrod) ────────────────────────────────────────

    struct CultureWorld {
        clock: SimClock,
        n_features: usize,
        // row-major: agent i's features at [i*F .. i*F+F]
        cells: Vec<u32>,
        topology: Topology,
    }

    impl CultureWorld {
        fn new(n_agents: usize, n_features: usize, cells: Vec<u32>, topology: Topology) -> Self {
            assert_eq!(cells.len(), n_agents * n_features);
            Self {
                clock: SimClock::new(1_000_000),
                n_features,
                cells,
                topology,
            }
        }
        fn n_agents(&self) -> usize {
            self.cells.len() / self.n_features
        }
    }

    impl WorldState for CultureWorld {
        fn agent_ids(&self) -> Vec<AgentId> {
            (0..self.n_agents() as u64).map(AgentId).collect()
        }
        fn clock(&self) -> &SimClock {
            &self.clock
        }
        fn clock_mut(&mut self) -> &mut SimClock {
            &mut self.clock
        }
    }
    impl CultureVectors for CultureWorld {
        fn n_features(&self) -> usize {
            self.n_features
        }
        fn feature(&self, id: AgentId, f: usize) -> u32 {
            self.cells[id.0 as usize * self.n_features + f]
        }
        fn set_feature(&mut self, id: AgentId, f: usize, value: u32) {
            self.cells[id.0 as usize * self.n_features + f] = value;
        }
    }
    impl Neighbors for CultureWorld {
        fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> {
            neighbors(&self.topology, self.n_agents(), id)
        }
    }

    // ── shared step runner ─────────────────────────────────────────────────────

    /// Run a mechanism for `steps` steps against `world`.  Returns the number of
    /// steps actually run before a `request_stop` (`steps` if never stopped).
    fn run<M, W>(mech: &mut M, world: &mut W, rng: &mut SimRng, steps: usize) -> usize
    where
        M: Mechanism<W>,
        W: WorldState,
    {
        let order = world.agent_ids();
        for s in 0..steps {
            let mut scratch = Blackboard::new();
            let mut stop = false;
            let mut rec = NullRecorder;
            let clock = *world.clock();
            let mut ctx = StepContext {
                world,
                clock,
                rng,
                recorder: &mut rec,
                agent_order: &order,
                scratch: &mut scratch,
                stop: &mut stop,
            };
            mech.apply(Phase::Interaction, &mut ctx).unwrap();
            if stop {
                return s + 1;
            }
        }
        steps
    }

    fn num_clusters(opinions: &[f64], tol: f64) -> usize {
        let mut sorted = opinions.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut clusters = 0;
        let mut last = f64::NEG_INFINITY;
        for &x in &sorted {
            if (x - last).abs() > tol {
                clusters += 1;
            }
            last = x;
        }
        clusters
    }

    fn spread(opinions: &[f64]) -> f64 {
        let lo = opinions.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = opinions.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        hi - lo
    }

    // ── MeanOperator correctness ─────────────────────────────────────────────

    #[test]
    fn mean_operator_values() {
        let mut r = SimRng::from_seed(0);
        let v = [0.2, 0.5, 0.9];
        let a = apply_mean(MeanOperator::Arithmetic, &v, &mut r);
        let g = apply_mean(MeanOperator::Geometric, &v, &mut r);
        let h = apply_mean(MeanOperator::Harmonic, &v, &mut r);
        assert!((a - (0.2 + 0.5 + 0.9) / 3.0).abs() < 1e-12);
        assert!(a >= g - 1e-12);
        assert!(g >= h - 1e-12);
        let p1 = apply_mean(MeanOperator::Power(1.0), &v, &mut r);
        assert!((p1 - a).abs() < 1e-12);
        for _ in 0..200 {
            let x = apply_mean(MeanOperator::Random, &v, &mut r);
            assert!((0.2..=0.9).contains(&x));
        }
    }

    // ── Hegselmann–Krause (after Neighbors refactor) ──────────────────────────

    #[test]
    fn hk_large_epsilon_reaches_consensus() {
        let opinions: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let mut world = OpinionWorld::complete(opinions);
        let mut rng = SimRng::from_seed(7);
        let mut hk = HegselmannKrauseMechanism::new(1.0, MeanOperator::Arithmetic);
        run(&mut hk, &mut world, &mut rng, 100);
        assert!(
            spread(&world.opinions) < 1e-9,
            "expected consensus, spread = {}",
            spread(&world.opinions)
        );
    }

    #[test]
    fn hk_small_epsilon_fragments() {
        let opinions: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let mut world = OpinionWorld::complete(opinions);
        let mut rng = SimRng::from_seed(7);
        let mut hk = HegselmannKrauseMechanism::new(0.05, MeanOperator::Arithmetic);
        run(&mut hk, &mut world, &mut rng, 200);
        let clusters = num_clusters(&world.opinions, 1e-3);
        assert!(
            clusters > 1,
            "expected fragmentation, got {} cluster(s)",
            clusters
        );
    }

    #[test]
    fn hk_is_deterministic_given_seed() {
        let opinions: Vec<f64> = (0..15).map(|i| i as f64 / 14.0).collect();
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut world = OpinionWorld::complete(opinions.clone());
            let mut rng = SimRng::from_seed(42);
            let mut hk = HegselmannKrauseMechanism::new(0.15, MeanOperator::Arithmetic);
            run(&mut hk, &mut world, &mut rng, 50);
            runs.push(world.opinions);
        }
        assert_eq!(runs[0], runs[1]);
    }

    #[test]
    fn hk_respects_ring_topology() {
        let opinions: Vec<f64> = (0..10).map(|i| i as f64 / 9.0).collect();
        let mut world = OpinionWorld::ring(opinions);
        let mut rng = SimRng::from_seed(1);
        let mut hk = HegselmannKrauseMechanism::new(0.2, MeanOperator::Arithmetic);
        run(&mut hk, &mut world, &mut rng, 50);
        assert!(world.opinions.iter().all(|&x| (0.0..=1.0).contains(&x)));
    }

    // ── Deffuant (after Neighbors refactor) ───────────────────────────────────

    #[test]
    fn deffuant_large_epsilon_reaches_consensus() {
        let opinions: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let mut world = OpinionWorld::complete(opinions);
        let mut rng = SimRng::from_seed(3);
        let mut d = DeffuantMechanism::new(1.0, 0.5, 200);
        run(&mut d, &mut world, &mut rng, 500);
        assert!(
            spread(&world.opinions) < 1e-6,
            "expected consensus, spread = {}",
            spread(&world.opinions)
        );
    }

    #[test]
    fn deffuant_small_epsilon_fragments() {
        let opinions: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let mut world = OpinionWorld::complete(opinions);
        let mut rng = SimRng::from_seed(3);
        let mut d = DeffuantMechanism::new(0.05, 0.5, 200);
        run(&mut d, &mut world, &mut rng, 500);
        let clusters = num_clusters(&world.opinions, 1e-3);
        assert!(
            clusters > 1,
            "expected fragmentation, got {} cluster(s)",
            clusters
        );
    }

    #[test]
    fn deffuant_contracts_and_conserves_mean_when_all_interact() {
        let opinions: Vec<f64> = (0..11).map(|i| i as f64 / 10.0).collect();
        let mean0: f64 = opinions.iter().sum::<f64>() / opinions.len() as f64;
        let spread0 = spread(&opinions);
        let mut world = OpinionWorld::complete(opinions);
        let mut rng = SimRng::from_seed(9);
        let mut d = DeffuantMechanism::new(1.0, 0.5, 50);
        run(&mut d, &mut world, &mut rng, 100);
        let mean1: f64 = world.opinions.iter().sum::<f64>() / world.opinions.len() as f64;
        assert!((mean0 - mean1).abs() < 1e-9, "mean not conserved");
        assert!(
            spread(&world.opinions) <= spread0 + 1e-12,
            "spread increased"
        );
    }

    #[test]
    fn deffuant_is_deterministic_given_seed() {
        let opinions: Vec<f64> = (0..15).map(|i| i as f64 / 14.0).collect();
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut world = OpinionWorld::complete(opinions.clone());
            let mut rng = SimRng::from_seed(101);
            let mut d = DeffuantMechanism::new(0.3, 0.5, 30);
            run(&mut d, &mut world, &mut rng, 80);
            runs.push(world.opinions);
        }
        assert_eq!(runs[0], runs[1]);
    }

    // ── Social Judgement (mirrors mou2024 sj tests) ────────────────────────────

    #[test]
    fn sj_assimilates_in_acceptance_region() {
        // Single agent i=0 at 0.0, one neighbour at 0.2 within ε=0.4 → moves up.
        let mut world = OpinionWorld::complete(vec![0.0, 0.2]);
        let mut rng = SimRng::from_seed(0);
        // Make agent 1 a fixed source by only checking agent 0's move direction.
        let before = world.opinions[0];
        let mut sj = SocialJudgementMechanism::new(0.4, 0.5, 0.8, 0.2);
        run(&mut sj, &mut world, &mut rng, 1);
        assert!(
            world.opinions[0] > before,
            "SJ should assimilate toward nearby message"
        );
    }

    #[test]
    fn sj_repels_in_rejection_region() {
        // Agent 0 at 0.0, neighbour at 0.9 (> rejection 0.8) → repelled negative.
        let mut world = OpinionWorld::complete(vec![0.0, 0.9]);
        let mut rng = SimRng::from_seed(0);
        let before = world.opinions[0];
        let mut sj = SocialJudgementMechanism::new(0.4, 0.5, 0.8, 0.2);
        run(&mut sj, &mut world, &mut rng, 1);
        assert!(
            world.opinions[0] < before,
            "SJ should repel from far message"
        );
    }

    #[test]
    fn sj_stays_in_range_and_is_deterministic() {
        let opinions: Vec<f64> = (0..21).map(|i| -1.0 + 2.0 * i as f64 / 20.0).collect();
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut world = OpinionWorld::complete(opinions.clone());
            let mut rng = SimRng::from_seed(5);
            let mut sj = SocialJudgementMechanism::default();
            run(&mut sj, &mut world, &mut rng, 50);
            assert!(world.opinions.iter().all(|&x| (-1.0..=1.0).contains(&x)));
            runs.push(world.opinions);
        }
        assert_eq!(runs[0], runs[1]);
    }

    // ── Lorenz (mirrors mou2024 lorenz test) ───────────────────────────────────

    #[test]
    fn lorenz_polarizes_extremes() {
        // An already-extreme positive agent, with only a far-negative neighbour
        // (outside ε), is pushed further positive by the polarisation term.
        let mut world = OpinionWorld::complete(vec![0.8, -0.9]);
        let mut rng = SimRng::from_seed(0);
        let before = world.opinions[0];
        let mut lz = LorenzMechanism::new(0.4, 0.5, 0.2);
        run(&mut lz, &mut world, &mut rng, 1);
        assert!(
            world.opinions[0] > before,
            "Lorenz should push an extreme attitude further out"
        );
    }

    #[test]
    fn lorenz_stays_in_range_and_is_deterministic() {
        let opinions: Vec<f64> = (0..21).map(|i| -1.0 + 2.0 * i as f64 / 20.0).collect();
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut world = OpinionWorld::complete(opinions.clone());
            let mut rng = SimRng::from_seed(11);
            let mut lz = LorenzMechanism::default();
            run(&mut lz, &mut world, &mut rng, 30);
            assert!(world.opinions.iter().all(|&x| (-1.0..=1.0).contains(&x)));
            runs.push(world.opinions);
        }
        assert_eq!(runs[0], runs[1]);
    }

    // ── SI contagion ───────────────────────────────────────────────────────────

    #[test]
    fn si_single_seed_spreads_to_whole_complete_graph() {
        let mut active = vec![false; 12];
        active[0] = true;
        let mut world = ContagionWorld::complete(active);
        let mut rng = SimRng::from_seed(7);
        let mut si = SiContagionMechanism::new(0.5);
        run(&mut si, &mut world, &mut rng, 1000);
        assert_eq!(
            world.n_active(),
            12,
            "β>0 on a complete graph should infect everyone"
        );
    }

    #[test]
    fn si_beta_zero_never_spreads() {
        let mut active = vec![false; 8];
        active[0] = true;
        let mut world = ContagionWorld::complete(active);
        let mut rng = SimRng::from_seed(7);
        let mut si = SiContagionMechanism::new(0.0);
        run(&mut si, &mut world, &mut rng, 100);
        assert_eq!(world.n_active(), 1, "β=0 must not spread");
    }

    #[test]
    fn si_is_deterministic_given_seed() {
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut active = vec![false; 20];
            active[0] = true;
            let mut world = ContagionWorld::ring(active);
            let mut rng = SimRng::from_seed(123);
            let mut si = SiContagionMechanism::new(0.4);
            run(&mut si, &mut world, &mut rng, 100);
            runs.push(world.active);
        }
        assert_eq!(runs[0], runs[1]);
    }

    #[test]
    fn si_self_stops_on_saturation() {
        let mut active = vec![false; 6];
        active[0] = true;
        let mut world = ContagionWorld::complete(active);
        let mut rng = SimRng::from_seed(1);
        let mut si = SiContagionMechanism::new(1.0); // β=1 → full in 1 round.
        let steps = run(&mut si, &mut world, &mut rng, 1000);
        assert_eq!(world.n_active(), 6);
        assert!(
            steps < 1000,
            "should request_stop on saturation (ran {steps})"
        );
    }

    // ── Threshold contagion ────────────────────────────────────────────────────

    #[test]
    fn threshold_low_theta_cascades_fully() {
        // Complete graph of 10, one seed; θ tiny → one active neighbour suffices.
        let mut active = vec![false; 10];
        active[0] = true;
        let mut world = ContagionWorld::complete(active);
        let mut rng = SimRng::from_seed(0);
        let mut th = ThresholdContagionMechanism::new(0.05);
        run(&mut th, &mut world, &mut rng, 100);
        assert_eq!(world.n_active(), 10, "low θ should cascade fully");
    }

    #[test]
    fn threshold_high_theta_stalls() {
        // Ring (degree 2), one seed; θ=1.0 needs both neighbours active. A single
        // seed has each neighbour with only 1/2 active neighbours → never fires.
        let mut active = vec![false; 12];
        active[0] = true;
        let mut world = ContagionWorld::ring(active);
        let mut rng = SimRng::from_seed(0);
        let mut th = ThresholdContagionMechanism::new(1.0);
        run(&mut th, &mut world, &mut rng, 100);
        assert_eq!(world.n_active(), 1, "high θ should stall at the seed");
    }

    #[test]
    fn threshold_is_deterministic() {
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut active = vec![false; 16];
            active[0] = true;
            active[8] = true;
            let mut world = ContagionWorld::ring(active);
            let mut rng = SimRng::from_seed(99);
            let mut th = ThresholdContagionMechanism::new(0.5);
            run(&mut th, &mut world, &mut rng, 100);
            runs.push(world.active);
        }
        assert_eq!(runs[0], runs[1]);
    }

    // ── Axelrod culture ────────────────────────────────────────────────────────

    /// Build a culture world with random features in `[0, q)`, on a grid.
    fn random_culture(w: usize, h: usize, f: usize, q: u32, seed: u64) -> CultureWorld {
        let mut rng = SimRng::from_seed(seed);
        let n = w * h;
        let mut cells = Vec::with_capacity(n * f);
        use rand::Rng;
        for _ in 0..n * f {
            cells.push(rng.gen_range(0..q));
        }
        CultureWorld::new(n, f, cells, Topology::Adjacency(grid_adj(w, h)))
    }

    #[test]
    fn axelrod_reaches_absorbing_state() {
        // Small grid, moderate F / small q → reaches an absorbing state.
        let mut world = random_culture(5, 5, 5, 3, 42);
        let mut rng = SimRng::from_seed(1);
        let mut ax = AxelrodMechanism::new(200);
        // Run plenty of events; assert it eventually stops changing.
        for _ in 0..2000 {
            run(&mut ax, &mut world, &mut rng, 1);
            if is_absorbing(&world) {
                break;
            }
        }
        assert!(
            is_absorbing(&world),
            "Axelrod should reach an absorbing state"
        );
    }

    #[test]
    fn axelrod_identical_grid_is_fixed_point() {
        // Every agent identical → similarity 1 on every edge → absorbing, and no
        // event can change anything.
        let n = 9;
        let f = 4;
        let cells = vec![2u32; n * f];
        let mut world = CultureWorld::new(n, f, cells.clone(), Topology::Adjacency(grid_adj(3, 3)));
        assert!(is_absorbing(&world));
        let mut rng = SimRng::from_seed(5);
        let mut ax = AxelrodMechanism::new(500);
        run(&mut ax, &mut world, &mut rng, 50);
        assert_eq!(world.cells, cells, "identical grid must be a fixed point");
    }

    #[test]
    fn axelrod_identical_neighbours_stay_identical() {
        // Two adjacent agents identical, rest different: the identical pair never
        // diverges (events only ever copy, making cultures more alike).
        // Use a complete graph of 3 where 0 and 1 are identical.
        let f = 3;
        let cells = vec![
            1, 1, 1, // agent 0
            1, 1, 1, // agent 1 (identical to 0)
            2, 2, 2, // agent 2 (disjoint)
        ];
        let mut world = CultureWorld::new(3, f, cells, Topology::Complete);
        let mut rng = SimRng::from_seed(3);
        let mut ax = AxelrodMechanism::new(100);
        run(&mut ax, &mut world, &mut rng, 50);
        // 0 and 1 share nothing-to-copy between themselves (sim=1); the only way
        // they change is via agent 2, but 2 shares 0 features with them (sim=0),
        // so no copy ever happens → all three unchanged.
        let a0: Vec<u32> = (0..f).map(|k| world.feature(AgentId(0), k)).collect();
        let a1: Vec<u32> = (0..f).map(|k| world.feature(AgentId(1), k)).collect();
        assert_eq!(a0, a1, "identical neighbours must stay identical");
    }

    #[test]
    fn axelrod_is_deterministic_given_seed() {
        let mut finals = Vec::new();
        for _ in 0..2 {
            let mut world = random_culture(4, 4, 4, 4, 77);
            let mut rng = SimRng::from_seed(202);
            let mut ax = AxelrodMechanism::new(50);
            run(&mut ax, &mut world, &mut rng, 100);
            finals.push(world.cells);
        }
        assert_eq!(finals[0], finals[1]);
    }

    // ── convergence helper ───────────────────────────────────────────────────

    #[test]
    fn max_abs_delta_basics() {
        assert_eq!(max_abs_delta(&[1.0, 2.0], &[1.0, 2.5]), 0.5);
        assert!(max_abs_delta(&[1.0], &[1.0, 2.0]).is_infinite());
        assert_eq!(max_abs_delta(&[1.0, 1.0], &[1.0, 1.0]), 0.0);
    }
}
