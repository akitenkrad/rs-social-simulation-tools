**English** | [日本語](01-first-model.ja.md)

# T1 — Your first model

**What you'll build:** a tiny "cooling" model from scratch in Rust — one `WorldState`, one `Mechanism` — that stops itself on convergence and writes its own CSV.
**Estimated time:** 30 minutes.

## Prerequisites

- [T0 — Getting started](00-getting-started.md) (so the concepts *pack / scenario / metric* are familiar).
- A working Rust toolchain. Basic Rust (structs, traits, `impl`) is enough.

The code below is the shipped example [`crates/socsim-engine/examples/engine_only.rs`](../../crates/socsim-engine/examples/engine_only.rs). Open it alongside this page — we narrate it top to bottom.

## Steps

### 1. The model

Each agent holds some "heat". One mechanism cools every agent by a fixed rate each step; the run is done once all heat reaches zero. No grid, no network — just enough state to see the control flow.

### 2. Define the `WorldState`

`WorldState` owns all shared state. The trait requires only two things: the agent roster and the clock. Everything else (here, the per-agent heat) is yours:

```rust
struct CoolingWorld {
    clock: SimClock,
    heat: BTreeMap<AgentId, f64>,
}

impl WorldState for CoolingWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        // BTreeMap keys are already sorted — matches the determinism convention.
        self.heat.keys().copied().collect()
    }
    fn clock(&self) -> &SimClock {
        &self.clock
    }
    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}
```

Note the comment on `agent_ids`: returning IDs in **sorted** order is part of socsim's determinism contract (a `BTreeMap` gives you that for free). The world also exposes a convergence check the driver will poll:

```rust
fn is_converged(&self) -> bool {
    self.heat.values().all(|h| *h <= 0.0)
}
```

### 3. Define one `Mechanism`

A mechanism is one unit of research logic. It declares which **phase(s)** of the 6-phase tick loop it runs in, and puts its logic in `apply`. Here cooling is a decision, so it runs in `Phase::Decision`:

```rust
impl Mechanism<CoolingWorld> for CoolingMechanism {
    fn name(&self) -> &str {
        "cooling"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, CoolingWorld>) -> Result<()> {
        let mut active = 0usize;
        let mut total = 0.0;
        for id in ctx.agent_order {
            if let Some(h) = ctx.world.heat.get_mut(id) {
                if *h > 0.0 {
                    *h = (*h - self.rate).max(0.0);
                    active += 1;
                }
                total += *h;
            }
        }

        // Hand the step's active count to the driver via step-scoped scratch.
        ctx.scratch.insert("active", active);

        // Wide tabular row — your own column schema.
        ctx.recorder.record_row(
            ctx.clock.t(),
            "cooling",
            &[("active", active as f64), ("total_heat", total)],
        );

        if active == 0 {
            ctx.request_stop();
        }
        Ok(())
    }
}
```

Three pieces of the **`StepContext`** appear here, and they're the heart of library mode:

- `ctx.agent_order` — the activation order for this step (the scheduler decides it). You iterate it instead of `agent_ids()` so the order is consistent and reproducible.
- `ctx.scratch` — a step-scoped key/value store the engine clears every step. Use it to pass a transient value (here `active`) out to the driver without adding a field to `WorldState`.
- `ctx.recorder.record_row(...)` — emit a wide tabular row with your own column names. `ctx.request_stop()` asks the engine to end after this step.

The six phases run in a fixed order every step: `PreStep → Environment → Decision → Interaction → Reward → PostStep`. A mechanism only fires in the phases it lists. See the [6-phase tick loop](../architecture.md#the-6-phase-tick-loop) for the full model.

### 4. Assemble with `SimulationBuilder` and a fixed seed

```rust
let world = CoolingWorld::new(5, 1_000); // t_max is a safety cap we never hit
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(SequentialScheduler))
    .seed(42)
    .recorder(Box::new(CsvRecorder::new()))
    .add_mechanism(Box::new(CoolingMechanism { rate: 1.0 }))
    .build();
```

`.seed(42)` makes the run **fully deterministic**: the same seed + same code reproduce the same trajectory bit-for-bit (socsim uses a seeded ChaCha20 RNG). The default recorder is a no-op `NullRecorder`; we opt into a `CsvRecorder` to capture the rows.

### 5. Drive it with `run_until` and write your own output

Many models reach a fixed point long before `t_max`. Instead of running blindly to the cap, drive the loop yourself and stop on convergence:

```rust
sim.run_until(|w| w.is_converged())
    .expect("simulation completed");

let last_active = sim.scratch().get::<usize>("active").copied().unwrap_or(0);
println!(
    "converged at t = {} (t_max = {}), stop_requested = {}, last active = {}",
    sim.world().clock().t(),
    sim.world().clock().t_max(),
    sim.stop_requested(),
    last_active,
);
```

`run_until(predicate)` checks the predicate against the world after each step and stops when it holds. After the run you read final state from `sim.world()` and the last step's scratch from `sim.scratch()`. Finally, pull the CSV straight out of the recorder — no JSONL, no runner:

```rust
let rec = sim
    .recorder()
    .as_any()
    .and_then(|a| a.downcast_ref::<CsvRecorder>())
    .expect("recorder is a CsvRecorder");
print!("{}", rec.table_csv("cooling").expect("table exists"));
```

## Run it

```sh
cargo run -p socsim-engine --example engine_only
```

```
converged at t = 6 (t_max = 1000), stop_requested = false, last active = 1

t,active,total_heat
1,5,15
2,5,10
3,4,6
4,3,3
5,2,1
6,1,0
```

The model converges at `t = 6` (long before the `t_max = 1000` cap), and the CSV is the recorder's own table. Run it again — the output is identical, because of the fixed seed.

## What you learned

- A model = a **`WorldState`** (shared state, sorted `agent_ids`, clock) + one or more **`Mechanism`s** (each in chosen phases).
- The **6-phase loop** runs mechanisms in a fixed order; `apply` gets a **`StepContext`** with `agent_order`, `scratch`, `recorder`, `rng`, and `world`.
- A fixed **seed** makes runs **deterministic and reproducible**.
- `run_until` stops on convergence; `ctx.request_stop()` does the same from inside a mechanism.
- You can capture output yourself (here `CsvRecorder`) with no scenario TOML or runner.

For the complete reference on each step, see the [Library API](../library.md).

## Next

[T2 — Opinion dynamics on a network](02-opinion-network.md): give agents a social graph and **reuse** a shipped mechanism and metrics instead of writing your own.
