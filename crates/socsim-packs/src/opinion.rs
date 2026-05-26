//! The `opinion-dynamics` pack: a concrete `OpinionWorld`, a metrics mechanism,
//! a [`register`] function wiring the `socsim-mechanisms` opinion mechanisms,
//! and the [`OPINION_DYNAMICS_STARTER`] starter scenario TOML.
//!
//! `OpinionWorld` is the CLI-side world the opinion-dynamics mechanisms in
//! `socsim-mechanisms` (Hegselmann–Krause, Deffuant, Social Judgement, Lorenz)
//! operate over.  It owns a [`SocialNetwork`] (built from `[world]` params), a
//! per-agent scalar opinion (`Vec<f64>`), a [`SimClock`], and a
//! `last_max_delta` carry-over for convergence diagnostics.
//!
//! It implements the three capability traits the mechanisms require:
//! [`WorldState`], [`ScalarOpinions`], and [`Neighbors`] (the latter delegates
//! to the social network's adjacency).
//!
//! # Opinion range
//!
//! The bounded-confidence mechanisms (HK / Deffuant) impose no range of their
//! own; the polarising ones (SJ / Lorenz) clamp to `[-1, 1]`.  The starter
//! scenario uses the bounded-confidence family, so this world initialises
//! opinions in **`[0, 1]`** (the canonical HK / Deffuant range) regardless of
//! `init_distribution`; `"polarized"` still lives in `[0, 1]` but concentrates
//! mass near the two extremes `0` and `1`.

use rand::Rng;

use socsim_core::{
    AgentId, Mechanism, Neighbors, Phase, Result, ScalarOpinions, SimClock, SimRng, StepContext,
    WorldState,
};
use socsim_net::SocialNetwork;

/// World state for the opinion-dynamics pack: a social network plus a per-agent
/// scalar opinion.
pub struct OpinionWorld {
    clock: SimClock,
    net: SocialNetwork,
    /// Opinion of each agent, indexed by `AgentId.0 as usize`.
    opinions: Vec<f64>,
    /// Largest single-step opinion change recorded by the metrics mechanism in
    /// the previous step (convergence diagnostic).
    last_max_delta: f64,
}

impl OpinionWorld {
    /// Build an `OpinionWorld` from `[world]` params and a per-trial seed.
    ///
    /// Recognised params:
    /// - `n_agents` (u64, default 100) — number of agents.
    /// - `network_model` (str, default `"watts_strogatz"`) — one of
    ///   `"watts_strogatz"`, `"erdos_renyi"`, `"barabasi_albert"`.
    /// - `network_k` (u64, default 6) — WS mean degree.
    /// - `network_beta` (f64, default 0.1) — WS rewiring probability.
    /// - `network_p` (f64, default 0.05) — ER edge probability.
    /// - `network_m` (u64, default 3) — BA attachment count.
    /// - `init_distribution` (str, default `"uniform"`) — one of `"uniform"`,
    ///   `"normal"`, `"polarized"`.
    pub fn new(params: &socsim_config::Params, seed: u64) -> Self {
        let n = params.get_u64("n_agents", 100) as usize;
        let model = params.get_str("network_model", "watts_strogatz").to_owned();
        let mut rng = SimRng::from_seed(seed);

        let ids: Vec<AgentId> = (0..n as u64).map(AgentId).collect();
        let net = match model.as_str() {
            "erdos_renyi" => {
                let p = params.get_f64("network_p", 0.05);
                SocialNetwork::erdos_renyi(&ids, p, &mut rng)
            }
            "barabasi_albert" => {
                let m = params.get_u64("network_m", 3) as usize;
                SocialNetwork::barabasi_albert(&ids, m, &mut rng)
            }
            // Default / "watts_strogatz".
            _ => {
                let k = params.get_u64("network_k", 6) as usize;
                let beta = params.get_f64("network_beta", 0.1);
                SocialNetwork::watts_strogatz(&ids, k, beta, &mut rng)
            }
        };

        let init = params.get_str("init_distribution", "uniform").to_owned();
        let opinions: Vec<f64> = (0..n).map(|_| init_opinion(&init, &mut rng)).collect();

        Self {
            clock: SimClock::new(u64::MAX),
            net,
            opinions,
            last_max_delta: f64::INFINITY,
        }
    }

    /// Current opinion snapshot (read-only), in agent-id order.
    pub fn opinions(&self) -> &[f64] {
        &self.opinions
    }
}

/// Draw one initial opinion in `[0, 1]` for the given distribution.
fn init_opinion(distribution: &str, rng: &mut SimRng) -> f64 {
    match distribution {
        // Approximately-normal bump centred at 0.5 via the mean of two uniforms
        // (a triangular distribution), clamped to [0, 1].  Avoids a rand_distr
        // dependency while still concentrating mass near the centre.
        "normal" => {
            let x = (rng.gen::<f64>() + rng.gen::<f64>()) / 2.0;
            x.clamp(0.0, 1.0)
        }
        // Bimodal: half the mass near 0, half near 1, each a narrow uniform.
        "polarized" => {
            if rng.gen::<bool>() {
                (rng.gen::<f64>() * 0.1).clamp(0.0, 1.0)
            } else {
                (1.0 - rng.gen::<f64>() * 0.1).clamp(0.0, 1.0)
            }
        }
        // Default / "uniform": U[0, 1].
        _ => rng.gen::<f64>(),
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
        self.net.neighbors(id)
    }
}

// ── OpinionMetricsMechanism ──────────────────────────────────────────────────

/// A `PostStep` mechanism that records per-step scalar metrics for an opinion
/// world.
///
/// Each step it records, via the [`Recorder`](socsim_core::Recorder) on the
/// [`StepContext`], the following metrics keyed at the current time `t`:
/// - `clusters` — number of distinct opinion groups within `tol`;
/// - `variance` — population variance of opinions;
/// - `spread` — `max − min` of opinions;
/// - `mean` — arithmetic mean of opinions;
/// - `max_delta` — the largest single-step opinion change since the previous
///   step (a convergence diagnostic), also cached on the world.
#[derive(Clone, Debug)]
pub struct OpinionMetricsMechanism {
    /// Tolerance used to bucket opinions into clusters.
    tol: f64,
    /// Previous-step opinion snapshot (`None` until the first `PostStep`).
    prev: Option<Vec<f64>>,
}

impl OpinionMetricsMechanism {
    /// Create a metrics mechanism with cluster tolerance `tol`.
    pub fn new(tol: f64) -> Self {
        Self { tol, prev: None }
    }
}

impl Default for OpinionMetricsMechanism {
    fn default() -> Self {
        Self::new(0.01)
    }
}

impl Mechanism<OpinionWorld> for OpinionMetricsMechanism {
    fn name(&self) -> &str {
        "opinion_metrics"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::PostStep]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, OpinionWorld>) -> Result<()> {
        let curr: Vec<f64> = ctx.world.opinions().to_vec();
        let n = curr.len();
        let t = ctx.clock.t();

        let (mean, variance, spread, clusters) = if n == 0 {
            (0.0, 0.0, 0.0, 0.0)
        } else {
            let mean = curr.iter().sum::<f64>() / n as f64;
            let variance = curr.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
            let lo = curr.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = curr.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let spread = hi - lo;
            let clusters = distinct_clusters(&curr, self.tol) as f64;
            (mean, variance, spread, clusters)
        };

        let max_delta = match &self.prev {
            Some(prev) => socsim_mechanisms::max_abs_delta(prev, &curr),
            None => f64::INFINITY,
        };
        // Cache the convergence diagnostic on the world too.
        ctx.world.last_max_delta = max_delta;
        // A clean +∞ does not serialise well; record it as a sentinel on step 0.
        let recorded_delta = if max_delta.is_finite() { max_delta } else { 0.0 };

        ctx.recorder.record_metric(t, "clusters", clusters);
        ctx.recorder.record_metric(t, "variance", variance);
        ctx.recorder.record_metric(t, "spread", spread);
        ctx.recorder.record_metric(t, "mean", mean);
        ctx.recorder.record_metric(t, "max_delta", recorded_delta);

        self.prev = Some(curr);
        Ok(())
    }
}

/// Number of distinct opinion clusters at tolerance `tol`.
///
/// Sorts the opinions and starts a new cluster whenever the gap to the previous
/// value exceeds `tol`, so two opinions count as the same cluster iff they are
/// linked by a chain of ≤ `tol` gaps.
fn distinct_clusters(opinions: &[f64], tol: f64) -> usize {
    if opinions.is_empty() {
        return 0;
    }
    let mut sorted = opinions.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut clusters = 1usize;
    let mut last = sorted[0];
    for &x in &sorted[1..] {
        if (x - last).abs() > tol {
            clusters += 1;
        }
        last = x;
    }
    clusters
}

// ── registration ─────────────────────────────────────────────────────────────

/// Register all `opinion-dynamics` mechanisms into a registry over
/// [`OpinionWorld`].
///
/// Mechanisms (all generic over `ScalarOpinions + Neighbors`, which
/// `OpinionWorld` implements):
/// - `hegselmann_krause` — HK bounded confidence (params: `epsilon`,
///   `mean` = `A`/`G`/`H`/`P<p>`/`R`, `p` fallback for bare `P`).
/// - `deffuant` — Deffuant pairwise (params: `epsilon`, `mu`,
///   `pairs_per_step`).
/// - `social_judgement` — assimilation/repulsion (params: `epsilon`,
///   `alpha`, `rejection`, `repulsion`).
/// - `lorenz` — assimilation + polarisation (params: `epsilon`,
///   `alpha`, `repulsion`).
/// - `convergence` — `PostStep` stop on `max|Δx| < tol` (param: `tol`).
/// - `opinion_metrics` — `PostStep` metrics recorder (param: `tol`).
pub fn register(reg: &mut socsim_config::Registry<OpinionWorld>) {
    use socsim_config::Params;
    use socsim_mechanisms::{
        parse_mean, ConvergenceMechanism, DeffuantMechanism, HegselmannKrauseMechanism,
        LorenzMechanism, MeanOperator, SocialJudgementMechanism,
    };

    reg.register("hegselmann_krause", |p: &Params| {
        let epsilon = p.get_f64("epsilon", 0.2);
        let p_fallback = p.get_f64("p", 1.0);
        let mean = parse_mean(p.get_str("mean", "A"), p_fallback)
            .map_err(socsim_core::SocsimError::Config)?;
        Ok(Box::new(HegselmannKrauseMechanism::new(epsilon, mean))
            as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
    });
    reg.register("deffuant", |p: &Params| {
        let epsilon = p.get_f64("epsilon", 0.2);
        let mu = p.get_f64("mu", 0.5);
        let pairs = p.get_u64("pairs_per_step", 1) as usize;
        Ok(Box::new(DeffuantMechanism::new(epsilon, mu, pairs))
            as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
    });
    reg.register("social_judgement", |p: &Params| {
        let epsilon = p.get_f64("epsilon", 0.4);
        let alpha = p.get_f64("alpha", 0.5);
        let rejection = p.get_f64("rejection", 0.8);
        let repulsion = p.get_f64("repulsion", 0.2);
        Ok(
            Box::new(SocialJudgementMechanism::new(
                epsilon, alpha, rejection, repulsion,
            )) as Box<dyn socsim_core::Mechanism<OpinionWorld>>,
        )
    });
    reg.register("lorenz", |p: &Params| {
        let epsilon = p.get_f64("epsilon", 0.4);
        let alpha = p.get_f64("alpha", 0.5);
        let repulsion = p.get_f64("repulsion", 0.2);
        Ok(Box::new(LorenzMechanism::new(epsilon, alpha, repulsion))
            as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
    });
    reg.register("convergence", |p: &Params| {
        let tol = p.get_f64("tol", 1e-4);
        Ok(Box::new(ConvergenceMechanism::new(tol))
            as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
    });
    reg.register("opinion_metrics", |p: &Params| {
        let tol = p.get_f64("tol", 0.01);
        Ok(Box::new(OpinionMetricsMechanism::new(tol))
            as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
    });
    // Silence the unused-import warning when no other path uses it.
    let _ = MeanOperator::Arithmetic;
}

/// Starter scenario TOML for the `opinion-dynamics` pack, emitted by
/// `socsim init --module-pack opinion-dynamics`.
pub const OPINION_DYNAMICS_STARTER: &str = r#"# Opinion Dynamics Scenario — generated by `socsim init`
# Bounded-confidence consensus (Hegselmann–Krause) on a small-world network.
# Opinions live in [0, 1]; with ε large enough relative to the spread, agents
# coalesce into a small number of clusters (consensus). Run with:
#   socsim run <this-file>

[simulation]
name        = "opinion_dynamics_baseline"
module_pack = "opinion-dynamics"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[world]
n_agents          = 200
network_model     = "watts_strogatz"
network_k         = 6
network_beta      = 0.1
init_distribution = "uniform"

[[mechanism]]
name  = "hegselmann_krause"
phase = "interaction"
[mechanism.params]
epsilon = 0.25
mean    = "A"

[[mechanism]]
name  = "opinion_metrics"
phase = "post_step"
[mechanism.params]
tol = 0.01

[[mechanism]]
name  = "convergence"
phase = "post_step"
[mechanism.params]
tol = 0.0001

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["clusters", "variance", "spread", "mean"]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_clusters_counts_groups() {
        // Two tight groups at ~0 and ~1.
        let ops = vec![0.0, 0.001, 0.002, 1.0, 1.001];
        assert_eq!(distinct_clusters(&ops, 0.01), 2);
        // All distinct beyond tol.
        let ops = vec![0.0, 0.5, 1.0];
        assert_eq!(distinct_clusters(&ops, 0.01), 3);
        // Empty.
        assert_eq!(distinct_clusters(&[], 0.01), 0);
    }

    #[test]
    fn world_builds_and_initializes_in_unit_range() {
        let params = socsim_config::Params::empty();
        let world = OpinionWorld::new(&params, 42);
        assert_eq!(world.opinions().len(), 100);
        assert!(world.opinions().iter().all(|&x| (0.0..=1.0).contains(&x)));
        assert_eq!(world.net.node_count(), 100);
    }

    #[test]
    fn polarized_init_concentrates_at_extremes() {
        let table: toml::Table =
            toml::from_str("n_agents = 200\ninit_distribution = \"polarized\"").unwrap();
        let params = socsim_config::Params::from(table);
        let world = OpinionWorld::new(&params, 7);
        // Every opinion should be near 0 or near 1.
        assert!(world
            .opinions()
            .iter()
            .all(|&x| x < 0.15 || x > 0.85));
    }
}
