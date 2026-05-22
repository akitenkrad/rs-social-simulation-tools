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
| recorder | `InMemoryRecorder` |

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

`socsim-log` ships three implementations:

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

## Using the reference HR lifecycle module as a library

`socsim-hr-lifecycle` exports `HrWorld`, `HrLifecyclePack`, and the per-employee `Employee` and team `Team` structs. To use it programmatically without the CLI:

```rust,ignore
use socsim_hr_lifecycle::{HrWorld, HrLifecyclePack};
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

For the full printout version see `crates/socsim-hr-lifecycle/examples/hr_baseline.rs`.

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

Hold a `GridIndex` (or a bare `Grid`) inside your `WorldState` and drive moves from a `Mechanism`.

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

A worked engine-only example lives at `crates/socsim-engine/examples/engine_only.rs`.
