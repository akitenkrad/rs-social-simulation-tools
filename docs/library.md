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

`Simulation::run` loops until `world.clock().is_done()`. `Simulation::step` advances one step at a time if you need fine-grained control.

---

## Determinism

Determinism is guaranteed by three design choices:

1. **Seeded ChaCha20 RNG.** `SimRng::from_seed(seed)` creates a fully deterministic generator. The same seed + same code always produces the same trajectory.
2. **Sorted agent IDs.** `WorldState::agent_ids` in `HrWorld` returns IDs in sorted order; team-mean aggregations iterate a sorted copy. Hash-map iteration order never influences results.
3. **Child RNGs via `SimRng::derive`.** Mechanisms can derive independent child RNGs per agent or phase using `SimRng::derive(&[agent_id, phase_index])` without mutating the parent stream.

To verify determinism in your own code, run two simulations with the same seed and compare outputs — the `custom_mechanism.rs` example does exactly this.

---

## Recording metrics and events

The `Recorder` trait has two methods:

```rust,ignore
fn record_metric(&mut self, t: u64, key: &str, value: f64);
fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value);
```

`socsim-log` ships two implementations:

| Type | Use |
|---|---|
| `InMemoryRecorder` | Tests; inspect `metrics()` and `events()` after the run |
| `JsonlRecorder<W>` | Production; writes one JSON line per record to any `Write` sink |

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
