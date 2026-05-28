**English** | [日本語](library.ja.md)

# Library API

`socsim` can be used as a Rust library: add the relevant crates to your `Cargo.toml` dependencies and compose simulations programmatically. This page covers the complete workflow from implementing a custom `Mechanism` to running the simulation.

---

## Core abstractions

All socsim logic is built on four traits defined in `socsim-core`:

| Trait | Purpose |
|---|---|
| `WorldState` | Owns all shared simulation state (agents, clock, domain data) |
| `Mechanism<W>` | One composable unit of research logic; runs in one or more `Phase`s |
| `Scheduler<W>` | Determines agent activation order each step |
| `Recorder` | Sink for metrics and structured events |

A `StepContext<'_, W>` is passed to every `Mechanism::apply` call and provides mutable access to the world, a copy of the clock, the RNG, the recorder, and the activation order for that step.

---

## Step 1 — Implement `WorldState`

`WorldState` must provide the agent roster and the clock. Everything else (domain state) is up to you.

```rust,ignore
use socsim_core::{AgentId, SimClock, WorldState};

struct CounterWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    pub value: f64,
}

impl CounterWorld {
    fn new(t_max: u64) -> Self {
        Self {
            clock: SimClock::new(t_max),
            agents: vec![AgentId(0)],
            value: 0.0,
        }
    }
}

impl WorldState for CounterWorld {
    fn agent_ids(&self) -> Vec<AgentId> { self.agents.clone() }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
```

---

## Step 2 — Implement `Mechanism`

A mechanism declares the `Phase`(s) it participates in and receives a `StepContext` during each of those phases.

```rust,ignore
use socsim_core::{Mechanism, Phase, Result, StepContext};

struct GrowthMechanism {
    rate: f64,
}

impl Mechanism<CounterWorld> for GrowthMechanism {
    fn name(&self) -> &str { "growth" }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Environment]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, CounterWorld>) -> Result<()> {
        ctx.world.value += self.rate;
        ctx.recorder.record_metric(ctx.clock.t(), "value", ctx.world.value);
        Ok(())
    }
}
```

The six phases, in execution order, are:

| Phase | Typical use |
|---|---|
| `PreStep` | Bookkeeping, reset per-step counters |
| `Environment` | Exogenous shocks, resource replenishment, learning curves |
| `Decision` | Agent decisions (turnover intent, hiring) |
| `Interaction` | Peer effects, network diffusion, contagion |
| `Reward` | Compute and apply rewards; record aggregate metrics |
| `PostStep` | Cleanup, socialisation, emit departure/hire events |

A mechanism can register multiple phases by returning a longer slice from `phases()`. It will be called once per registered phase in `Phase::ORDER`.

---

## Step 3 — Bundle into a `ModulePack` (recommended)

`ModulePack` groups related mechanisms into a named bundle. This matches the CLI's `--module-pack` concept and makes it easy to activate an entire research module in one call.

```rust,ignore
use socsim_config::{ModulePack, Params, Registry};

struct DemoPack;

impl ModulePack<CounterWorld> for DemoPack {
    fn pack_name(&self) -> &str { "demo" }

    fn register(&self, reg: &mut Registry<CounterWorld>) {
        reg.register("growth", |params| {
            let rate = params.get_f64("rate", 1.0);
            Ok(Box::new(GrowthMechanism { rate }))
        });
    }
}
```

`Params` provides typed, defaulted getters (`get_f64`, `get_u64`, `get_bool`, `get_str`, …) backed by a TOML table. Constructors should always supply a sensible default so that scenarios without an explicit value still work.

---

## Step 4 — Register and build mechanisms via `Registry`

```rust,ignore
use socsim_config::Params;

// Register via the pack
let mut reg: Registry<CounterWorld> = Registry::new();
DemoPack.register(&mut reg);

// Or register individual constructors directly
// reg.register("growth", |params| { ... });

// Instantiate from the registry
let params = Params::empty(); // or built from a TOML table
let growth: Box<dyn Mechanism<CounterWorld>> = reg.build("growth", &params).unwrap();
```

---

## Step 5 — Assemble and run via `SimulationBuilder`

`SimulationBuilder` is a fluent builder with sensible defaults:

| Option | Default |
|---|---|
| scheduler | `SequentialScheduler` (sorted `AgentId` order) |
| seed | `0` |
| recorder | `NullRecorder` (no-op) |

```rust,ignore
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let world = CounterWorld::new(10); // run for 10 steps
let mut sim = SimulationBuilder::new(world)
    .add_mechanism(growth)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)          // fixed seed → fully deterministic
    .build();

sim.run().unwrap();

println!("Final value: {}", sim.world().value);
```

`Simulation::run` loops until `world.clock().is_done()` **or** a mechanism requests an early stop. `Simulation::step` advances one step at a time if you need fine-grained control.

---

## Stopping early on convergence

Many ABMs reach a fixed point long before `t_max`. Two mechanisms are provided so you don't have to abandon `run()` and hand-roll a `step()` loop:

- **From inside a mechanism**, call `ctx.request_stop()`. The current step finishes (all remaining mechanisms run), then `run()` terminates. Query it later with `sim.stop_requested()`.
- **From the driver**, use `run_until(predicate)`, which checks the predicate against the world *after* each step:

```rust,ignore
// Stop as soon as the world reports convergence (but always at least one step).
sim.run_until(|w| w.is_converged())?;
```

```rust,ignore
// Equivalent from inside a mechanism (PostStep is a good place to check):
fn apply(&mut self, _p: Phase, ctx: &mut StepContext<'_, MyWorld>) -> Result<()> {
    if ctx.world.no_agent_moved_this_step() {
        ctx.request_stop();
    }
    Ok(())
}
```

---

## Per-step observation: `run_observed` / `StepReport`

When you need a metric **after every step** — a convergence curve, a per-tick count, a live progress print — you can drive the loop yourself with `step()` and then read `world()` / `scratch()`. `run_observed` packages that pattern so you don't hand-roll the loop or rely on fragile stringly-typed scratch reads:

```rust,ignore
let mut history = Vec::new();
sim.run_observed(|report| {
    // report: StepReport { t, stopped, world, scratch }
    history.push(report.world.distinct_opinions());
})?;
```

The closure is called once per executed step with a `StepReport` reflecting the state **after** that step:

| Field | Meaning |
|---|---|
| `t` | clock time after the step |
| `stopped` | `true` if a mechanism requested stop during this step |
| `world` | shared `&W` after the step |
| `scratch` | shared `&Blackboard` after the step (per-step values mechanisms left) |

Termination matches `run()` (clock done **or** stop requested); the observer is called for the step in which stop is requested (its report has `stopped == true`) and **not** for any step afterward. For one step at a time, `step_reported()` returns the same `StepReport` for a single step.

This is the recommended per-step loop for library-mode models — see `crates/socsim-engine/examples/cellular_automata.rs`.

---

## Acting on a subset of agents

The scheduler returns an activation order over **all** agents. Many models, however, only act on a subset that satisfies some condition (the *dissatisfied* in a segregation model, the *infected* in a contagion model). The idiomatic pattern is to **snapshot the eligible set at the start of the step**, then filter the activation order against it:

```rust,ignore
fn apply(&mut self, _p: Phase, ctx: &mut StepContext<'_, MyWorld>) -> Result<()> {
    // Snapshot eligible agents BEFORE anyone acts, so mid-step state changes
    // (e.g. a neighbour moving away) don't pull extra agents into this step.
    let eligible: std::collections::HashSet<AgentId> = ctx.world
        .agent_ids().into_iter()
        .filter(|id| ctx.world.is_eligible(*id))
        .collect();

    for id in ctx.agent_order {              // shuffled by the scheduler
        if !eligible.contains(id) { continue; }
        if ctx.world.is_eligible(*id) {      // may have changed since snapshot
            ctx.world.act(*id);
        }
    }
    Ok(())
}
```

Filtering the (already shuffled) full order is statistically equivalent to shuffling just the eligible subset. **Synchronous vs. asynchronous semantics matter:** snapshotting the eligible set gives synchronous-style updates (the count of actors is fixed at step start); acting on whoever is currently eligible gives asynchronous updates. Choose deliberately — it changes the dynamics.

---

## Step-scoped scratch (`Blackboard`)

`ctx.scratch` is a type-erased key/value store the engine **clears at the start of every step**. Use it to pass transient values between mechanisms in the same step, or out to the driver, without adding per-step bookkeeping fields to `WorldState`:

```rust,ignore
// In a mechanism:
ctx.scratch.insert("n_moved", n_moved_usize);

// In a later mechanism the same step, or in the driver right after step():
let moved = sim.scratch().get::<usize>("n_moved").copied().unwrap_or(0);
```

Values written during a step remain readable until the next `step()` call, then they are cleared.

---

## Determinism

Determinism is guaranteed by these design choices:

1. **Seeded ChaCha20 RNG.** `SimRng::from_seed(seed)` creates a fully deterministic generator. The same seed + same code always produces the same trajectory.
2. **Sorted agent IDs.** `WorldState::agent_ids` should return IDs in sorted order; aggregations iterate a sorted copy. Hash-map iteration order never influences results.
3. **Child RNGs via `SimRng::derive`.** Mechanisms can derive independent child RNGs per agent or phase using `SimRng::derive(&[agent_id, phase_index])` without mutating the parent stream.

### Separating world-init RNG from the engine RNG

`SimulationBuilder::seed(seed)` builds the engine's RNG internally, but you often also need randomness **before** the builder exists — e.g. to place agents when constructing the world. Seeding two independent `SimRng`s with the *same* `seed` works but couples the two streams. The clean pattern is to treat `seed` as a **root** and derive labelled child seeds:

```rust,ignore
use socsim_core::{derive_seed, SimRng};

const RNG_WORLD_INIT: u64 = 0;
const RNG_ENGINE: u64 = 1;

let root = seed;
let mut init_rng = SimRng::from_seed(derive_seed(root, &[RNG_WORLD_INIT]));
let world = MyWorld::new(&mut init_rng);          // place agents, etc.

let mut sim = SimulationBuilder::new(world)
    .seed(derive_seed(root, &[RNG_ENGINE]))       // independent, labelled stream
    .build();
```

`derive_seed` (re-exported from `socsim-core`) is the same FNV-1a mix used by `SimRng::derive`, so the two streams are decorrelated yet fully reproducible from a single root seed.

#### RNG stream labeling convention

To avoid every model reinventing its own labels, socsim recommends a small fixed convention for the two streams almost every model needs:

| Label | Stream |
|---|---|
| `derive_seed(root, &[0])` | world initialisation (placing agents, randomising cells) |
| `derive_seed(root, &[1])` | the engine / scheduler (passed to `SimulationBuilder::seed`) |

```rust,ignore
let root = seed;
let mut init_rng = SimRng::from_seed(derive_seed(root, &[0])); // world init
let world = MyWorld::new(&mut init_rng);
let mut sim = SimulationBuilder::new(world)
    .seed(derive_seed(root, &[1]))                              // engine
    .build();
```

Reserve further labels (`&[2]`, `&[3]`, …) for any additional independent streams your model owns. The `cellular_automata` example follows exactly this convention.

To verify determinism in your own code, run two simulations with the same seed and compare outputs — the `custom_mechanism.rs` example does exactly this.

---

## Recording metrics and events

The `Recorder` trait has three recording methods:

```rust,ignore
fn record_metric(&mut self, t: u64, key: &str, value: f64);
fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value);
// Wide tabular row — many named columns sharing one t and table:
fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]);
```

`record_row` is the natural shape for `metrics.csv`-style output with many columns; the default implementation fans a row out into `record_metric` calls keyed `"{table}.{column}"`, so recorders that don't override it keep working.

The engine's **default** recorder is `NullRecorder` (defined in `socsim-core`), a no-op sink that discards everything. Because of this, the engine no longer depends on `socsim-log`: a pure-library model that does its own output (like the `cellular_automata` example) needs only `socsim-core` / `socsim-engine` / `socsim-grid` and never has to pull in a concrete recorder. Add `socsim-log` and call `SimulationBuilder::recorder(...)` only when you actually want metric/event capture.

`socsim-log` ships three concrete implementations:

| Type | Use |
|---|---|
| `InMemoryRecorder` | Tests; inspect `metrics()` and `events()` after the run |
| `JsonlRecorder<W>` | Production; writes one JSON line per record to any `Write` sink |
| `CsvRecorder` | Tabular output; accumulates `record_row` calls per table and renders column-aligned CSV (plus long-format `metrics_csv()`) |

```rust,ignore
use socsim_core::Recorder;
use socsim_log::CsvRecorder;

let mut rec = CsvRecorder::new();
rec.record_row(0, "metrics", &[("avg_same", 0.53), ("n_moved", 0.0)]);
rec.record_row(1, "metrics", &[("avg_same", 0.64), ("n_moved", 21.0)]);
let csv = rec.table_csv("metrics").unwrap();   // "t,avg_same,n_moved\n0,0.53,0\n1,0.64,21\n"
std::fs::write("metrics.csv", csv).unwrap();
```

By default `CsvRecorder` discovers columns in the order they are first observed. To pin a **caller-defined** column order and schema — useful when a downstream tool expects an exact header — call `set_columns` before rendering:

```rust,ignore
rec.set_columns("metrics", &["n_moved", "avg_same"]);  // fixed order
let csv = rec.table_csv("metrics").unwrap();            // header: "t,n_moved,avg_same"
```

Columns not listed in the schema are omitted; schema columns missing from a given row render as empty fields. `set_columns` only affects rendering, not which rows are stored.

To inspect the recorder after `sim.run()`:

```rust,ignore
use socsim_log::InMemoryRecorder;

let rec = sim.recorder()
    .as_any()
    .and_then(|a| a.downcast_ref::<InMemoryRecorder>())
    .unwrap();

for row in rec.metrics() {
    println!("t={} {}={}", row.t, row.key, row.value);
}
```

---

## Reusable metrics with `socsim-metrics`

Rather than reimplementing common summary statistics, the **`socsim-metrics`** crate provides them as a reusable, library-only layer. Metrics are pure observation functions (no RNG, no state mutation), so they never affect a model's reproducibility.

- **Zero-dependency numeric core** (`socsim_metrics::stats`, always compiled): `mean`, `variance`, `std_dev`, `spread`, `min_max`, `gini`, `shannon_entropy`, `hhi`, `simpson_diversity`, `distinct_clusters(values, tol)`, `bimodality_coefficient`, `polarization`, `extremeness`, `max_abs_delta` / `mean_abs_delta`, `num_distinct` / `largest_share`. Each documents its exact formula.
- **`core` feature** (→ `socsim-core`): extractors that read a `W: ScalarOpinions` directly (`opinion_mean`, `opinion_variance`, `opinion_spread`, `opinion_clusters`, …) plus a generic `MetricsMechanism<W>` that records a configurable set of named metrics each `PostStep`.
- **`network` feature** (→ `socsim-net`): `mean_degree`, `global_clustering_coefficient`, `component_sizes`, `largest_component_fraction`, `cascade_size` / `reach_fraction`.
- **`spatial` feature** (→ `socsim-grid`): `segregation_index`, `local_similarity` over a label accessor.

The default build pulls in no socsim crates — depend on it as `socsim-metrics = { …, default-features = false }` for just `stats`, and enable `core` / `network` / `spatial` as needed.

`MetricsMechanism<W>` records metrics declaratively (it calls `recorder.record_metric` for you each step):

```rust,ignore
use socsim_metrics::opinion::{MetricsMechanism, opinion_variance, opinion_spread, opinion_clusters};

let metrics = MetricsMechanism::new()
    .with("variance", |w| opinion_variance(w))
    .with("spread",   |w| opinion_spread(w))
    .with("clusters", |w| opinion_clusters(w, 0.01));
builder.add_mechanism(metrics);   // fires in PostStep, one record_metric per entry
```

> **Keep paper-specific metrics local.** Reuse `socsim-metrics` only for *canonical* statistics; a metric with a model-specific definition (e.g. a polarization measure defined as the product of extreme-opinion fractions, or a cascade-size aggregation over domain events) belongs in the replication — sharing it would change its meaning. The opinion-dynamics pack's `OpinionMetricsMechanism` (in `socsim-packs`) is a worked example of delegating only the canonical parts (`mean` / `variance` / `spread` / `distinct_clusters`) to `socsim-metrics::stats`.

---

## Using the reference HR lifecycle module as a library

The `socsim-packs` `hr_lifecycle` module exports `HrWorld`, `HrLifecyclePack`, and the per-employee `Employee` and team `Team` structs. To use it programmatically without the CLI:

```rust,ignore
use socsim_packs::hr_lifecycle::{HrWorld, HrLifecyclePack};
use socsim_config::{ModulePack, Params, Registry};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};
use socsim_core::SimRng;

let seed = 42u64;
let mut rng = SimRng::from_seed(seed);
let mut world = HrWorld::new(5, 8, 4, 0.1, &mut rng);
world.clock = socsim_core::SimClock::new(60);

let mut reg = Registry::new();
HrLifecyclePack.register(&mut reg);

let p = Params::empty();
let names = ["learning_curve","peer_effect","ocb","fit",
              "turnover","knowledge_loss","toxic_spread",
              "hiring","socialization","org_performance"];

let mut builder = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(seed);

for name in &names {
    builder = builder.add_mechanism(reg.build(name, &p).unwrap());
}

let mut sim = builder.build();
sim.run().unwrap();

println!("org_performance = {}", sim.world().org_performance);
```

For the full printout version see `crates/socsim-packs/examples/hr_baseline.rs`.

---

## Network models with `socsim-net`

For social-graph models (opinion dynamics, influence, contagion on a network, follow/unfollow dynamics), `socsim-net` provides an `AgentId`-keyed graph with reproducible generators, so you don't reimplement Erdős–Rényi / Watts–Strogatz / Barabási–Albert or neighbour lookups:

```rust,ignore
use socsim_net::SocialNetwork;
use socsim_core::AgentId;

let ids: Vec<AgentId> = (0..200u64).map(AgentId).collect();
let net = SocialNetwork::watts_strogatz(&ids, 6, 0.1, &mut init_rng); // k=6, beta=0.1

let nbrs = net.neighbors(AgentId(0));          // owned Vec
let deg  = net.degree(AgentId(0));
let comps = net.connected_components();
```

| Item | Purpose |
|---|---|
| `SocialNetwork::erdos_renyi / watts_strogatz / barabasi_albert / empty` | reproducible generators (all take `&mut SimRng`) |
| `add_node` / `add_edge` / `remove_node` / `remove_edge` | dynamic graph mutation |
| `neighbors` / `neighbors_into(&mut buf)` / `neighbors_iter` | neighbour access (allocating / zero-alloc / iterator) |
| `degree` / `node_count` / `edge_count` / `contains` | basic queries |
| `edges` / `degree_sequence` / `degree_distribution` | export & degree analysis |
| `average_path_length` / `average_clustering_coefficient` / `local_bridges` | network metrics (Granovetter, small-world) |
| `connected_components` / `component_membership` / `largest_component_size` | connectivity |
| `Network<E, Ty>` + `DiSocialNetwork` / `WeightedNetwork<E>` | generic over edge payload `E` and directedness `Ty` |

`SocialNetwork` is the undirected, unweighted default (`Network<(), Undirected>`). For directed follow-graphs use `DiSocialNetwork` (`out_neighbors` / `in_neighbors`); for weighted ties use `add_edge_weighted(a, b, w)` + `edge_weight(a, b)`.

### Worked example: bounded-confidence opinion dynamics

Hold the network in a `WorldState`, give each agent a continuous opinion, and update it from its neighbours in the `Interaction` phase. This is a **bounded-confidence DeGroot** model: each agent moves a fraction `mu` toward the mean opinion of the neighbours it still trusts (those within a confidence radius `epsilon`).

> The `socsim-mechanisms` crate ships ready-made opinion mechanisms (`HegselmannKrauseMechanism`, `DeffuantMechanism`, `SocialJudgementMechanism`, `LorenzMechanism`) generic over `ScalarOpinions + Neighbors`, plus network contagion (`SiContagionMechanism`, `ThresholdContagionMechanism`) and `AxelrodMechanism`. For **hybrid models** that update opinions from a custom message set *inside their own* mechanism (e.g. injecting external broadcasts), it also exposes the bare message-set Δ functions `socsim_mechanisms::{bounded_confidence_update, hk_update, social_judgement_update, lorenz_update}` so you can reuse the update math without adopting a standalone mechanism. Initial distributions for paper-style ε-profile runs are covered by the free helper `socsim_mechanisms::regular_profile(n)` (equispaced `x_i = i/(n−1)`). The worked example below builds the bounded-confidence update from scratch to show the mechanics.

```rust,ignore
use socsim_core::{AgentId, Mechanism, Phase, Result, SimClock, SimRng, StepContext, WorldState};
use socsim_engine::SimulationBuilder;
use socsim_net::SocialNetwork;
use rand::Rng;

struct OpinionWorld {
    clock: SimClock,
    net: SocialNetwork,
    opinions: Vec<f64>,          // indexed by AgentId.0 as usize
    last_max_delta: f64,
}

impl OpinionWorld {
    fn new(n: usize, k: usize, beta: f64, init_rng: &mut SimRng) -> Self {
        let ids: Vec<AgentId> = (0..n as u64).map(AgentId).collect();
        let net = SocialNetwork::watts_strogatz(&ids, k, beta, init_rng); // built from the world-init stream
        let opinions = (0..n).map(|_| init_rng.gen::<f64>()).collect();
        Self { clock: SimClock::new(u64::MAX), net, opinions, last_max_delta: f64::INFINITY }
    }
}

impl WorldState for OpinionWorld {
    fn agent_ids(&self) -> Vec<AgentId> { (0..self.opinions.len() as u64).map(AgentId).collect() }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}

struct BoundedConfidence { epsilon: f64, mu: f64, tol: f64 }

impl Mechanism<OpinionWorld> for BoundedConfidence {
    fn name(&self) -> &str { "bounded_confidence" }
    fn phases(&self) -> &'static [Phase] { &[Phase::Interaction] }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, OpinionWorld>) -> Result<()> {
        let n = ctx.world.opinions.len();
        let current = ctx.world.opinions.clone();   // synchronous update: read old, write new
        let mut next = current.clone();
        let mut buf: Vec<AgentId> = Vec::new();      // reused across agents — no per-agent alloc
        let mut max_delta = 0.0_f64;

        for i in 0..n {
            let xi = current[i];
            let (mut sum, mut count) = (xi, 1usize); // an agent always trusts itself
            ctx.world.net.neighbors_into(AgentId(i as u64), &mut buf);  // zero-alloc neighbour read
            for &AgentId(j) in &buf {
                let xj = current[j as usize];
                if (xj - xi).abs() <= self.epsilon { sum += xj; count += 1; }
            }
            next[i] = xi + self.mu * (sum / count as f64 - xi);
            max_delta = max_delta.max((next[i] - xi).abs());
        }

        ctx.world.opinions = next;
        ctx.world.last_max_delta = max_delta;
        if max_delta < self.tol { ctx.request_stop(); }   // converged: opinions stopped moving
        Ok(())
    }
}

// RNG-stream convention: one root seed, [0] = world/network init, [1] = engine.
let root = 7u64;
let mut init_rng = SimRng::from_seed(socsim_core::derive_seed(root, &[0]));
let world = OpinionWorld::new(200, 6, 0.1, &mut init_rng);

let mut sim = SimulationBuilder::new(world)
    .seed(socsim_core::derive_seed(root, &[1]))   // independent engine stream
    .add_mechanism(Box::new(BoundedConfidence { epsilon: 0.2, mu: 0.5, tol: 1e-4 }))
    .build();

sim.run()?;     // converges into a handful of opinion clusters
```

Note the RNG-stream split: the network and the initial opinions are drawn from `derive_seed(root, &[0])` (world init), while the engine/scheduler gets the independent `derive_seed(root, &[1])` stream — the same labelling convention used for grid models above. The full runnable version (with a per-step cluster/Δ printout and a topology summary using `connected_components` / `average_clustering_coefficient`) lives at `crates/socsim-engine/examples/opinion_dynamics.rs`:

```bash
cargo run -p socsim-engine --example opinion_dynamics
```

---

## Spatial models with `socsim-grid`

For lattice-based models (segregation, contagion on a grid, diffusion), `socsim-grid` provides ready-made 2D space so you don't reimplement neighbourhoods and distances:

```rust,ignore
use socsim_grid::{Grid, GridIndex, Boundary, Neighborhood, Metric};
use socsim_core::AgentId;

let mut idx = GridIndex::new(Grid::new(13, 16, Boundary::Fixed));
idx.place(AgentId(0), 3, 4).unwrap();

let nbrs = idx.grid().neighbors(3, 4, Neighborhood::Moore);     // 8-neighbourhood
let occupied = idx.occupant_neighbors(3, 4, Neighborhood::Moore);
let target = idx.nearest_vacant((3, 4), Metric::Chebyshev);     // greedy relocation
idx.move_to(AgentId(0), target.unwrap().0, target.unwrap().1).unwrap();
```

| Type | Purpose |
|---|---|
| `Grid` | dimensions + `Boundary` (`Fixed` / `Toroidal`); `neighbors`, `neighbors_radius`, wrap-aware `distance` |
| `Neighborhood` | `Moore` (8) / `VonNeumann` (4) |
| `Metric` | `Chebyshev` / `Manhattan` / `Euclidean` |
| `GridIndex` | `AgentId ↔ cell` occupancy: `place`, `move_to`, `vacant_cells`, `nearest_vacant`, sorted `agent_ids` |
| `CellGrid<T>` | per-cell mutable state `T` for every cell (cellular-automata / lattice-attribute models) |
| `Adjacency` | precomputed CSR neighbour table for hot lattice loops |

Hold a `GridIndex` (or a bare `Grid`) inside your `WorldState` and drive moves from a `Mechanism`.

### Non-allocating neighbour queries

`Grid::neighbors` allocates a fresh `Vec` per call, which is fine for occasional lookups but wasteful in a hot loop. For per-step lattice code prefer one of:

- `Grid::neighbors_into(r, c, nbhd, &mut buf)` / `neighbors_radius_into(...)` — reuse one caller-owned `Vec` across calls (clears and refills it), avoiding per-call allocation.
- `Grid::neighbors_iter(r, c, nbhd)` — a radius-1 iterator that yields neighbours straight off the stack, with no heap allocation at all.
- `Grid::adjacency(nbhd)` / `adjacency_radius(nbhd, radius)` — **precompute the whole neighbour table once** as an `Adjacency` (CSR, flat row-major indices). `adj.neighbors(idx)` then returns the neighbours of cell `idx = r * cols + c` as an O(1) borrowed `&[usize]`. This is the recommended structure when the *same* neighbour sets are queried every tick (cellular automata, diffusion, contagion-on-a-grid): build it at world-construction time and store it in your `WorldState`.

All four return neighbours in the same deterministic sorted, row-major order, so results are interchangeable.

### Per-cell state with `CellGrid<T>`

Where `GridIndex` answers "*which agent* is in this cell", `CellGrid<T>` stores a value `T` for **every** cell — the primitive for cellular-automata and lattice-attribute models (each cell holding an opinion, strategy, or counter). It pairs the `Grid`'s boundary-aware neighbour queries with direct mutable access to the row-major backing `Vec<T>`:

```rust,ignore
use socsim_grid::{CellGrid, Grid, Boundary, Neighborhood};

// Build a grid of opinions; each cell starts from its coordinates (or an RNG).
let grid = Grid::new(16, 16, Boundary::Toroidal);
let adjacency = grid.adjacency(Neighborhood::Moore);   // precompute once
let mut cells: CellGrid<u8> = CellGrid::from_fn(grid, |r, c| ((r + c) % 4) as u8);

// Hot loop: O(1) neighbour lookups, direct cell mutation, no allocation.
let idx = 5 * 16 + 7;                       // cell (5, 7), flat row-major
let nbr = adjacency.neighbors(idx)[0];      // a neighbour's flat index
let opinion = *cells.get_idx(nbr).unwrap();
*cells.get_idx_mut(idx).unwrap() = opinion; // copy it over
```

Constructors: `CellGrid::new(grid, fill)` (every cell `= fill.clone()`) and `CellGrid::from_fn(grid, |r, c| ...)`. Access by coordinate (`get` / `get_mut`), by flat index (`get_idx` / `get_idx_mut`, matching `Adjacency`), or over the whole row-major slice (`cells` / `cells_mut`); `neighbors` / `neighbor_values` read the neighbourhood directly. A worked event-driven CA built on `CellGrid` + `Adjacency` lives at `crates/socsim-engine/examples/cellular_automata.rs`.

---

## Lightweight: engine-only usage (no TOML / Runner)

The `ModulePack` → `Registry` → scenario-TOML → `socsim-runner` path (Steps 3–4 above) is optional. If you already have your own CLI and output format — e.g. when porting an existing project — you can use **just the engine core** and skip TOML, the registry, and the runner entirely:

```rust,ignore
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

// 1. Build the world yourself (your own config struct, your own RNG).
let world = MyWorld::new(/* ... */);

// 2. Add mechanisms directly — no Registry, no ModulePack.
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(seed)
    .add_mechanism(Box::new(MyMechanism::new(/* ... */)))
    .build();

// 3. Drive it yourself and stop on convergence; write your own output.
sim.run_until(|w| w.is_converged())?;
write_my_csv(sim.world());          // your existing schema, no Recorder required
```

**When to choose which:**

| | Full-stack (ModulePack + TOML + Runner) | Engine-only |
|---|---|---|
| Config | scenario `.toml`, swept by `socsim-runner` | your own structs / CLI |
| Output | `JsonlRecorder` / runner summaries | whatever you write |
| Best for | new projects, parameter sweeps, reproducible scenario files | embedding the engine in an existing tool, custom output schemas |

### LLM agents and result output in library mode

Two small leaf crates round out engine-only library mode: `socsim-llm` for LLM-driven agents and `socsim-results` for writing the `results/` tree. Neither is wired into the `socsim` binary; depend on them directly.

**Build a client — use the shared harness, don't hand-roll a per-model `llm.rs`.** `socsim-llm` ships a reusable harness so LLM models don't reinvent the client wiring: `LlmSettings { temperature, seed, cache_path }`, the `LiveClient` type alias (`CachingClient<Box<dyn LlmClient>>`), `build_live_client_from_settings` (production, `live` feature), `wrap_client` (inject any backend — e.g. a test mock), and `llm_config` (a deterministic `LlmConfig` from the settings). Production and tests yield the same `LiveClient`:

```rust,ignore
use socsim_llm::{
    LlmSettings, LiveClient, build_live_client_from_settings, wrap_client, llm_config,
    PromptCache, mock::ScriptedClient,
};

let settings = LlmSettings { temperature: 0.0, seed: 42, cache_path: Some("runs/cache.json".into()) };

// Production: Ollama-first → OpenAI-fallback → caching (needs feature = "live").
let client: LiveClient = build_live_client_from_settings(&settings)?;

// Tests: a network-free scripted "model", wrapped into the same LiveClient shape.
let client: LiveClient = wrap_client(ScriptedClient::constant("test-model", "yes"), PromptCache::in_memory());

let cfg = llm_config(&settings);   // LlmConfig::deterministic() + temperature + seed
```

All eleven bundled LLM replications use this harness rather than a per-model client module. (The lower-level `build_live_client(cache_path: Option<&Path>)` is still available if you need it directly.)

**Confine the call to a `Decision`-phase mechanism.** `LlmClient::complete` is synchronous, so it slots straight into `apply`:

```rust,ignore
use socsim_core::{Mechanism, Phase, Result, StepContext};
use socsim_llm::LlmConfig;

struct LlmDecision { /* hold a &mut client or shared cell */ }

impl Mechanism<MyWorld> for LlmDecision {
    fn name(&self) -> &str { "llm_decision" }
    fn phases(&self) -> &'static [Phase] { &[Phase::Decision] }

    fn apply(&mut self, _p: Phase, ctx: &mut StepContext<'_, MyWorld>) -> Result<()> {
        let prompt = ctx.world.build_prompt();
        let resp = self.client.complete(&prompt, &LlmConfig::deterministic())?;  // temperature=0
        ctx.world.apply_choice(&resp.text);
        self.collector.record(resp.metadata);   // MetadataCollector → RunMetadata
        Ok(())
    }
}
```

A warm `PromptCache` (plus `temperature = 0`) replays identical responses, so a re-run is pseudo-deterministic on top of the seed-deterministic core.

**Write outputs.** `socsim-results` provides the timestamped-run + `latest`-symlink convention without any `Recorder`:

```rust,ignore
use socsim_results::{create_run_dir, refresh_latest_symlink, timestamp, write_csv, write_json};

let ts = timestamp();                          // "YYYYMMDD_HHMMSS"
let run_dir = create_run_dir("results")?;      // results/<ts>/
write_csv(&metric_rows, run_dir.join("metrics.csv"))?;
write_json(&collector.summary(), run_dir.join("llm_meta.json"))?;  // the RunMetadata sidecar
refresh_latest_symlink("results", &ts)?;       // results/latest → <ts>
```

For analysis/visualisation tooling, a shared Python package lives at [`tools/socsim_tools/`](../tools/socsim_tools/README.md) (a `build_dispatcher` CLI router plus `settings`/`io` helpers) for building each replication's `*-tools` CLI; adopt it as a `uv` git-subdirectory dependency.

---

## Snapshots: save & resume

If your world derives `serde`, you can capture and restore a run's **mutable state** (world + exact RNG stream + clock + stop flag). Mechanisms, the scheduler, and the recorder are *not* captured — they are code you supply when rebuilding the simulation (the PyTorch `state_dict` vs. architecture split).

```rust,ignore
use socsim_engine::{SimulationBuilder, Snapshot};

// 1. The world must be serde-serialisable (and Clone for in-memory snapshots).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct MyWorld { /* ... */ }

let mut sim = SimulationBuilder::new(MyWorld::new(/* ... */)).seed(7).build();
for _ in 0..100 { sim.step()?; }

// 2. Capture — in memory or to a JSON file.
let snap = sim.snapshot();            // requires W: Clone
snap.save("run.snapshot.json")?;      // requires W: Serialize

// 3. Later (even a fresh process): rebuild the SAME mechanisms, then restore.
let snap = Snapshot::load("run.snapshot.json")?;   // version-checked
let mut resumed = SimulationBuilder::new(MyWorld::placeholder())
    .seed(0)                          // overwritten by restore
    .add_mechanism(Box::new(MyMechanism::new(/* same as before */)))
    .build();
resumed.restore(snap);
resumed.run()?;                       // continues bit-identically from step 100
```

The methods are added by `impl` blocks gated on `W: Clone` / `Serialize` / `DeserializeOwned`, so the `WorldState` trait is unchanged — non-serde worlds simply lack them. The reference `HrWorld` (including its `SocialNetwork`, which serialises as a `{nodes, edges}` pair) is fully serde-able; see `examples/snapshot_resume.rs`. Restoring into a simulation wired with the same mechanisms reproduces the run bit-for-bit from the saved step onward, regardless of the new simulation's seed.

---

## Learnable policies (MARL)

The `Decision` phase can be made *learnable* with `socsim-marl` (Phase 6): a `PolicyMechanism` wraps a `Policy` and plugs into the six-phase loop like any other mechanism. The default `Policy` is `DiscretePolicyNet`, a small [`burn`](https://burn.dev) MLP trained with REINFORCE on CPU, with weights seeded from `SimRng` for bit-reproducibility. Because the policy operates on flat `&[f32]` features and `usize` actions, you bridge your world with three small traits:

```rust,ignore
use socsim_marl::{
    ActionApplier, DiscretePolicyNet, MarlTrainer, NetConfig, ObsEncoder,
    PolicyMechanism, RewardFn, TrainConfig, TrajectoryBuffer,
};

struct MyEncoder;          // world + agent → feature vector
impl ObsEncoder<MyWorld> for MyEncoder {
    fn obs_dim(&self) -> usize { 4 }
    fn encode(&self, w: &MyWorld, a: AgentId) -> Option<Vec<f32>> { /* ... */ }
}
struct MyApplier;          // chosen action index → world mutation
impl ActionApplier<MyWorld> for MyApplier {
    fn n_actions(&self) -> usize { 2 }
    fn apply(&self, w: &mut MyWorld, a: AgentId, action: usize, rng: &mut SimRng) { /* ... */ }
}
struct MyReward;           // per-agent reward, read after each step
impl RewardFn<MyWorld> for MyReward {
    fn reward(&self, w: &MyWorld, a: AgentId) -> f32 { /* ... */ }
}

// Outer learning loop: build a fresh sim per episode with a collect-mode policy.
let net = std::rc::Rc::new(std::cell::RefCell::new(
    DiscretePolicyNet::new(NetConfig::new(4, 2), &mut SimRng::from_seed(0))?,
));
let mut trainer = MarlTrainer::new(net);
let stats = trainer.train(
    &TrainConfig { episodes: 50, seed: 0 },
    |policy, buffer: std::rc::Rc<std::cell::RefCell<TrajectoryBuffer>>, seed| {
        SimulationBuilder::new(MyWorld::new(/* ... */))
            .seed(seed)
            .add_mechanism(Box::new(PolicyMechanism::collecting(
                policy, MyEncoder, MyApplier, buffer)))
            .build()
    },
    &MyReward,
)?;
```

After training, build the mechanism with `PolicyMechanism::inference(policy, …)` to run the **frozen** policy: it takes greedy actions, consumes no RNG, and stays bit-reproducible. `socsim-marl` pulls in `burn`, so the hr-lifecycle integration is gated behind a `marl` feature (`cargo run -p socsim-packs --features marl --example marl_turnover`).

Worked library-mode examples live at `crates/socsim-engine/examples/engine_only.rs` (a converging non-spatial model) and `crates/socsim-engine/examples/cellular_automata.rs` (an event-driven lattice CA on `CellGrid` + `Adjacency` using `run_observed`).
