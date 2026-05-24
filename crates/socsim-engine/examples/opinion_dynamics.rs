//! Opinion dynamics on a social network (`socsim-net`).
//!
//! The network-model counterpart of `cellular_automata.rs` (which uses
//! `socsim-grid`).  Agents sit on a Watts–Strogatz small-world graph and hold a
//! continuous opinion in `[0, 1]`.  Each step a **bounded-confidence DeGroot**
//! update moves every agent toward the average opinion of the neighbours it
//! still "trusts" — those within a confidence radius `epsilon`.  When opinions
//! stop moving, the run converges (clusters of agreement form).
//!
//! Demonstrated socsim-net APIs:
//! - construction via [`SocialNetwork::watts_strogatz`] (#18 generalised the
//!   backing graph; the undirected `SocialNetwork` alias is unchanged),
//! - zero-allocation neighbour access via `neighbors_into` (#19),
//! - analysis helpers `average_clustering_coefficient` / `connected_components`
//!   (#20) for a one-line topology summary.
//!
//! RNG-stream convention (issue #16): one root seed, with `derive_seed(root,
//! &[0])` initialising the world/network and `derive_seed(root, &[1])` driving
//! the engine.
//!
//! Run with: `cargo run -p socsim-engine --example opinion_dynamics`

use rand::Rng;

use socsim_core::{AgentId, Mechanism, Phase, Result, SimClock, SimRng, StepContext, WorldState};
use socsim_engine::SimulationBuilder;
use socsim_net::SocialNetwork;

/// World state: a social network plus a per-agent opinion.
struct OpinionWorld {
    clock: SimClock,
    net: SocialNetwork,
    /// Opinion of each agent, indexed by `AgentId.0 as usize`.
    opinions: Vec<f64>,
    /// Largest single-step opinion change in the previous step (convergence).
    last_max_delta: f64,
}

impl OpinionWorld {
    /// Build `n` agents on a Watts–Strogatz small-world graph, each with a
    /// random initial opinion drawn from `init_rng`.
    fn new(n: usize, k: usize, beta: f64, init_rng: &mut SimRng) -> Self {
        let ids: Vec<AgentId> = (0..n as u64).map(AgentId).collect();
        // The network is built from the *world-init* stream, not the engine's.
        let net = SocialNetwork::watts_strogatz(&ids, k, beta, init_rng);
        let opinions: Vec<f64> = (0..n).map(|_| init_rng.gen::<f64>()).collect();
        Self {
            clock: SimClock::new(u64::MAX),
            net,
            opinions,
            last_max_delta: f64::INFINITY,
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

/// Bounded-confidence DeGroot update.
///
/// Each agent moves a fraction `mu` of the way toward the mean opinion of the
/// neighbours within confidence radius `epsilon` (itself always included).
struct BoundedConfidence {
    epsilon: f64,
    mu: f64,
    /// Convergence threshold on the largest single-step change.
    tol: f64,
}

impl Mechanism<OpinionWorld> for BoundedConfidence {
    fn name(&self) -> &str {
        "bounded_confidence"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, OpinionWorld>) -> Result<()> {
        let n = ctx.world.opinions.len();
        // Synchronous update: read from the current opinions, write to a copy.
        let current = ctx.world.opinions.clone();
        let mut next = current.clone();
        let mut buf: Vec<AgentId> = Vec::new(); // reused across agents — no per-agent alloc (#19)
        let mut max_delta = 0.0_f64;

        for i in 0..n {
            let xi = current[i];
            let mut sum = xi; // an agent always trusts itself
            let mut count = 1usize;

            // Zero-allocation neighbour read into the reused buffer.
            ctx.world.net.neighbors_into(AgentId(i as u64), &mut buf);
            for &AgentId(j) in &buf {
                let xj = current[j as usize];
                if (xj - xi).abs() <= self.epsilon {
                    sum += xj;
                    count += 1;
                }
            }
            let mean = sum / count as f64;
            next[i] = xi + self.mu * (mean - xi);
            max_delta = max_delta.max((next[i] - xi).abs());
        }

        ctx.world.opinions = next;
        ctx.world.last_max_delta = max_delta;

        // Absorbing state: opinions have stopped moving ⇒ stop.
        if max_delta < self.tol {
            ctx.request_stop();
        }
        Ok(())
    }
}

/// Number of distinct opinion clusters at tolerance `tol` (rounded buckets).
fn distinct_clusters(opinions: &[f64], tol: f64) -> usize {
    let mut reps: Vec<f64> = Vec::new();
    for &x in opinions {
        if !reps.iter().any(|&r| (r - x).abs() <= tol) {
            reps.push(x);
        }
    }
    reps.len()
}

fn main() {
    // One root seed; derive labelled child streams (issue #16 convention):
    //   [0] → world / network initialisation, [1] → engine / scheduler.
    let root = 7u64;
    let mut init_rng = SimRng::from_seed(socsim_core::derive_seed(root, &[0]));

    let world = OpinionWorld::new(200, 6, 0.1, &mut init_rng);

    println!("=== socsim opinion_dynamics (bounded-confidence DeGroot on a graph) ===");
    println!(
        "200 agents, Watts–Strogatz(k=6, beta=0.1): {} component(s), avg clustering {:.3}\n",
        world.net.connected_components(),
        world.net.average_clustering_coefficient().unwrap_or(0.0),
    );

    let mut sim = SimulationBuilder::new(world)
        // Default recorder is NullRecorder — no socsim-log dependency needed.
        .seed(socsim_core::derive_seed(root, &[1]))
        .add_mechanism(Box::new(BoundedConfidence {
            epsilon: 0.2,
            mu: 0.5,
            tol: 1e-4,
        }))
        .build();

    println!("  t   clusters   max-delta");
    println!("  ----------------------------");
    sim.run_observed(|report| {
        if report.t <= 5 || report.t % 10 == 0 || report.stopped {
            let clusters = distinct_clusters(&report.world.opinions, 0.05);
            println!(
                "  {:>3}  {:>5}      {:.5}",
                report.t, clusters, report.world.last_max_delta
            );
        }
    })
    .expect("simulation completed");

    let clusters = distinct_clusters(&sim.world().opinions, 0.05);
    println!();
    println!(
        "Converged after {} steps into {} opinion cluster(s).",
        sim.world().clock.t(),
        clusters
    );
}
