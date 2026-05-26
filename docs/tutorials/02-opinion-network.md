**English** | [日本語](02-opinion-network.ja.md)

# T2 — Opinion dynamics on a network

**What you'll build:** a bounded-confidence opinion model on a small-world social graph — first written by hand to see the mechanics, then **rebuilt by reusing** the shipped `socsim-mechanisms` and `socsim-metrics` crates instead of writing your own.
**Estimated time:** 40 minutes.

## Prerequisites

- [T1 — Your first model](01-first-model.md) (`WorldState`, `Mechanism`, `StepContext`, seeds).
- Familiarity with the idea of a social graph (nodes + edges).

This tutorial has two backing artifacts, both CI-compiled:

- the from-scratch version: [`crates/socsim-engine/examples/opinion_dynamics.rs`](../../crates/socsim-engine/examples/opinion_dynamics.rs);
- the reuse version: the `opinion-dynamics` pack at [`crates/socsim-packs/src/opinion.rs`](../../crates/socsim-packs/src/opinion.rs), which you already ran from the CLI in T0.

## Part A — from scratch, with `socsim-net`

### 1. A world that holds a graph

`socsim-net` gives you an `AgentId`-keyed graph with reproducible generators, so you never reimplement Watts–Strogatz or neighbour lookups. Hold one in your `WorldState` alongside a per-agent opinion:

```rust
struct OpinionWorld {
    clock: SimClock,
    net: SocialNetwork,
    /// Opinion of each agent, indexed by `AgentId.0 as usize`.
    opinions: Vec<f64>,
    /// Largest single-step opinion change in the previous step (convergence).
    last_max_delta: f64,
}

impl OpinionWorld {
    fn new(n: usize, k: usize, beta: f64, init_rng: &mut SimRng) -> Self {
        let ids: Vec<AgentId> = (0..n as u64).map(AgentId).collect();
        // The network is built from the *world-init* stream, not the engine's.
        let net = SocialNetwork::watts_strogatz(&ids, k, beta, init_rng);
        let opinions: Vec<f64> = (0..n).map(|_| init_rng.gen::<f64>()).collect();
        Self { clock: SimClock::new(u64::MAX), net, opinions, last_max_delta: f64::INFINITY }
    }
}
```

Note the RNG comment: the graph and initial opinions are drawn from a **world-init** stream that is separate from the engine's stream (more on this below).

### 2. The bounded-confidence update

Each step, every agent moves a fraction `mu` toward the mean opinion of the neighbours it still *trusts* — those within a confidence radius `epsilon`. The update is **synchronous**: read everyone's current opinion, then write the new ones:

```rust
fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, OpinionWorld>) -> Result<()> {
    let n = ctx.world.opinions.len();
    let current = ctx.world.opinions.clone();   // read old
    let mut next = current.clone();             // write new
    let mut buf: Vec<AgentId> = Vec::new();     // reused across agents — no per-agent alloc
    let mut max_delta = 0.0_f64;

    for i in 0..n {
        let xi = current[i];
        let mut sum = xi;                       // an agent always trusts itself
        let mut count = 1usize;

        ctx.world.net.neighbors_into(AgentId(i as u64), &mut buf); // zero-alloc neighbour read
        for &AgentId(j) in &buf {
            let xj = current[j as usize];
            if (xj - xi).abs() <= self.epsilon { sum += xj; count += 1; }
        }
        let mean = sum / count as f64;
        next[i] = xi + self.mu * (mean - xi);
        max_delta = max_delta.max((next[i] - xi).abs());
    }

    ctx.world.opinions = next;
    ctx.world.last_max_delta = max_delta;
    if max_delta < self.tol { ctx.request_stop(); }  // opinions stopped moving ⇒ converged
    Ok(())
}
```

`neighbors_into(id, &mut buf)` reads neighbours into a buffer you reuse across agents — no per-agent heap allocation in the hot loop. This mechanism runs in `Phase::Interaction` (neighbour influence is an interaction).

### 3. Two RNG streams from one seed

The example derives two **labelled** child streams from one root seed so the world-init randomness (graph + opinions) is decorrelated from the engine's scheduler stream:

```rust
let root = 7u64;
let mut init_rng = SimRng::from_seed(socsim_core::derive_seed(root, &[0])); // [0] = world init
let world = OpinionWorld::new(200, 6, 0.1, &mut init_rng);

let mut sim = SimulationBuilder::new(world)
    .seed(socsim_core::derive_seed(root, &[1]))                              // [1] = engine
    .add_mechanism(Box::new(BoundedConfidence { epsilon: 0.2, mu: 0.5, tol: 1e-4 }))
    .build();
```

This `&[0]` = world / `&[1]` = engine convention recurs across socsim models. The example also prints a one-line topology summary using `connected_components()` and `average_clustering_coefficient()` — two of the analysis helpers `socsim-net` provides for free.

### Run Part A

```sh
cargo run -p socsim-engine --example opinion_dynamics
```

```
=== socsim opinion_dynamics (bounded-confidence DeGroot on a graph) ===
200 agents, Watts–Strogatz(k=6, beta=0.1): 1 component(s), avg clustering 0.447

  t   clusters   max-delta
  ----------------------------
    1     16      0.06395
    5     13      0.04117
  ...
  270      8      0.00011
  275      8      0.00010

Converged after 275 steps into 8 opinion cluster(s).
```

Distinct opinion clusters fall and `max-delta` shrinks below `tol`, at which point the mechanism requests a stop.

## Part B — don't write it yourself: reuse `socsim-mechanisms` + `socsim-metrics`

The hand-rolled `BoundedConfidence` above is exactly what the `socsim-mechanisms` crate already ships as `HegselmannKrauseMechanism` (generic over any world that can expose opinions and neighbours). The `opinion-dynamics` pack you ran in T0 is built this way. Two small **capability traits** are the contract that lets a generic mechanism drive *your* world — from `crates/socsim-packs/src/opinion.rs`:

```rust
impl ScalarOpinions for OpinionWorld {
    fn opinion(&self, id: AgentId) -> f64 { self.opinions[id.0 as usize] }
    fn set_opinion(&mut self, id: AgentId, value: f64) { self.opinions[id.0 as usize] = value; }
}

impl Neighbors for OpinionWorld {
    fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> { self.net.neighbors(id) }
}
```

Implement those two traits and you can drop in *any* opinion mechanism from the crate (`HegselmannKrauseMechanism`, `DeffuantMechanism`, `SocialJudgementMechanism`, `LorenzMechanism`) without writing update math. The pack registers HK by just constructing it:

```rust
reg.register("hegselmann_krause", |p: &Params| {
    let epsilon = p.get_f64("epsilon", 0.2);
    let p_fallback = p.get_f64("p", 1.0);
    let mean = parse_mean(p.get_str("mean", "A"), p_fallback)
        .map_err(socsim_core::SocsimError::Config)?;
    Ok(Box::new(HegselmannKrauseMechanism::new(epsilon, mean))
        as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
});
```

### Reuse the metrics too

The same idea applies to observation. Rather than reimplement mean / variance / spread / clustering, the pack's `OpinionMetricsMechanism` delegates the canonical statistics to `socsim-metrics`:

```rust
let mean = socsim_metrics::stats::mean(&curr);
let variance = socsim_metrics::stats::variance(&curr);
let spread = socsim_metrics::stats::spread(&curr);
let clusters = socsim_metrics::stats::distinct_clusters(&curr, self.tol) as f64;
```

`socsim-metrics` is a pure, library-only observation layer (no RNG, no state mutation), so reusing it can never change a model's reproducibility. For worlds that implement `ScalarOpinions`, the crate even offers a declarative `MetricsMechanism` so you can record a set of named metrics without writing a mechanism at all — see the [Library API metrics section](../library.md#reusable-metrics-with-socsim-metrics).

### Run Part B (via the CLI)

The pack is wired into the `socsim` binary, so you run it as a scenario (no Rust):

```sh
socsim run scenarios/opinion_dynamics_baseline.toml
```

```
Running 'opinion_dynamics_baseline' (pack=opinion-dynamics, t_max=60, seeds=[42], parallel=false)

t               clusters         max_delta              mean            spread          variance
10               22.0000            0.1238            0.5092            0.9769            0.0360
30               15.0000            0.0127            0.5094            0.9769            0.0243
60               12.0000            0.0010            0.5098            0.9769            0.0232
```

Same physics as Part A, but every line of update math and every statistic came from a shared crate. T5 shows how to build a pack like this yourself.

## What you learned

- `socsim-net` provides reproducible graph generators (`watts_strogatz`, …) and zero-alloc neighbour reads (`neighbors_into`), plus topology metrics (`connected_components`, `average_clustering_coefficient`).
- The `&[0]` world-init / `&[1]` engine RNG convention keeps two streams decorrelated yet reproducible from one seed.
- The **capability traits** `ScalarOpinions` + `Neighbors` let generic `socsim-mechanisms` (HK, Deffuant, …) drive your world — implement two methods, reuse the update math.
- `socsim-metrics` supplies canonical statistics as a pure observation layer; reuse it instead of reimplementing mean/variance/clusters.

See the [Mechanism catalog](../mechanisms.md) for every reusable mechanism and the [Library API](../library.md#network-models-with-socsim-net) for the full `socsim-net` surface.

## Next

[T3 — A spatial grid model](03-spatial-grid.md): swap the network for a lattice and run an event-driven cellular automaton.
