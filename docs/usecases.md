**English** | [日本語](usecases.ja.md)

# Use Cases & Recipes

This page collects concrete, copy-paste-ready workflows for common research tasks.

---

## 1. Run the HR lifecycle baseline

The bundled scenario `scenarios/hr_lifecycle_baseline.toml` runs a 5-team, 40-agent HR lifecycle model for 60 monthly steps with seed 42.

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
```

The CLI prints a condensed metric series (every 10th step, plus the last) and writes a JSONL log to `runs/hr_lifecycle_baseline_42.jsonl`.

To re-read the JSONL as a CSV summary later, without re-running:

```sh
socsim summarize runs/hr_lifecycle_baseline_42.jsonl
```

---

## 2. Multi-seed reproducibility check

Run the same scenario over seeds 0–9 to verify that results are deterministic across executions and to measure stochastic variance:

```sh
socsim run scenarios/hr_lifecycle_baseline.toml --seeds 0..10
```

The CLI prints a cross-seed summary table with mean, standard deviation, min, and max for each metric. Results are deterministic: re-running the same command always yields identical numbers, because each seed initialises an independent ChaCha20 RNG.

For faster throughput on a multi-core machine:

```sh
socsim run scenarios/hr_lifecycle_baseline.toml --seeds 0..10 --parallel
```

---

## 3. Parameter sweep to probe a hypothesis

**Research question:** Does a higher toxic-spread probability (`toxic_spread.p_spread`) degrade organisational performance by increasing turnover?

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "toxic_spread.p_spread=0.2,0.46,0.7" \
    --seeds 0..10 \
    --out runs/toxic_cascade_sweep/
```

This runs 3 × 10 = 30 trials and writes one CSV per combination to `runs/toxic_cascade_sweep/`. Each CSV has columns `key,mean,std,min,max,n`.

Sample sweep output (3 seeds for illustration):

```
Sweeping 'hr_lifecycle_baseline' over 1 axes × 3 seeds
  toxic_spread.p_spread = [0.2, 0.46, 0.7]
  combo 0: toxic_spread.p_spread=0.2000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               35.3250      5.0624     29.1000     41.5000      3
knowledge_stock          91.9687      4.6135     85.9783     97.2030      3
org_performance          41.3058      2.5623     37.8100     43.8800      3
turnover_rate             0.0167      0.0118      0.0000      0.0250      3
  combo 2: toxic_spread.p_spread=0.7000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               37.7583      1.6872     35.3750     39.0500      3
knowledge_stock          95.8431      2.2273     92.7248     97.7876      3
org_performance          43.0002      1.4341     40.9778     44.1428      3
turnover_rate             0.0250      0.0204      0.0000      0.0500      3
```

**Multi-dimensional sweep** — sweep two axes simultaneously:

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "peer_effect.alpha_peer=0.1,0.17,0.3" \
    --param "turnover.quit_cascade_bump=0.1,0.3,0.5" \
    --seeds 0..10 --parallel
```

This generates 3 × 3 = 9 combinations × 10 seeds = 90 trials.

---

## 4. Authoring a new research module

To add a custom simulation domain, implement three items and wire them together via `SimulationBuilder`.

### Step 1 — Define a `WorldState`

```rust,ignore
use socsim_core::{AgentId, SimClock, WorldState};

struct EconWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    pub gdp: f64,
}

impl WorldState for EconWorld {
    fn agent_ids(&self) -> Vec<AgentId> { self.agents.clone() }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
```

### Step 2 — Implement a `Mechanism`

```rust,ignore
use socsim_core::{Mechanism, Phase, Result, StepContext};

struct GrowthMechanism { rate: f64 }

impl Mechanism<EconWorld> for GrowthMechanism {
    fn name(&self) -> &str { "growth" }
    fn phases(&self) -> &'static [Phase] { &[Phase::Environment] }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, EconWorld>) -> Result<()> {
        ctx.world.gdp *= 1.0 + self.rate;
        ctx.recorder.record_metric(ctx.clock.t(), "gdp", ctx.world.gdp);
        Ok(())
    }
}
```

### Step 3 — Bundle into a `ModulePack`

```rust,ignore
use socsim_config::{ModulePack, Params, Registry};

struct EconPack;

impl ModulePack<EconWorld> for EconPack {
    fn pack_name(&self) -> &str { "econ" }
    fn register(&self, reg: &mut Registry<EconWorld>) {
        reg.register("growth", |params| {
            let rate = params.get_f64("rate", 0.02);
            Ok(Box::new(GrowthMechanism { rate }))
        });
    }
}
```

### Step 4 — Assemble and run

```rust,ignore
use socsim_engine::SimulationBuilder;

let mut reg = socsim_config::Registry::new();
EconPack.register(&mut reg);

let world = EconWorld { clock: socsim_core::SimClock::new(24), agents: vec![], gdp: 1000.0 };
let growth = reg.build("growth", &socsim_config::Params::empty()).unwrap();

let mut sim = SimulationBuilder::new(world)
    .add_mechanism(growth)
    .seed(42)
    .build();

sim.run().unwrap();
println!("Final GDP: {}", sim.world().gdp);
```

For the complete, runnable version of this pattern see `crates/socsim-engine/examples/custom_mechanism.rs`.

---

## 5. Pause and resume a long run (snapshots)

Checkpoint a run to disk and resume it later — useful for long sweeps, crash recovery, or branching what-if analyses from a common state. The world must derive `serde`; the snapshot captures the world, the exact RNG stream, and the clock, but **not** the mechanisms (you rebuild those).

```rust,ignore
use socsim_engine::Snapshot;

// ... run partway ...
for _ in 0..12 { sim.step()?; }
sim.snapshot().save("checkpoint.json")?;

// Later: rebuild a simulation with the SAME mechanisms, then restore.
let snap = Snapshot::load("checkpoint.json")?;
let mut resumed = build_my_sim(/* any seed */);
resumed.restore(snap);
resumed.run()?;   // continues bit-identically from month 12
```

Runnable demo: `cargo run -p socsim-hr-lifecycle --example snapshot_resume`. See the [library guide](library.md#snapshots-save--resume) for details.

---

## 6. Train a learnable turnover policy (MARL)

Replace a fixed decision heuristic with a policy learned by REINFORCE. The reference module ships a learnable turnover policy behind the `marl` feature:

```sh
cargo run -p socsim-hr-lifecycle --features marl --example marl_turnover
```

This trains a `burn` policy network so employees learn to stay/quit by individual-rationality reward, reproducing rational turnover as an emergent policy. To wire MARL into your own world, implement `ObsEncoder` / `ActionApplier` / `RewardFn` and drive `MarlTrainer` — see the [library guide](library.md#learnable-policies-marl).
