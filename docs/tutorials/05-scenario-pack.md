**English** | [日本語](05-scenario-pack.ja.md)

# T5 — A scenario pack

**What you'll build:** the full-stack path — bundle mechanisms into a `ModulePack`, register them in a `Registry`, drive them from both Rust and a scenario `.toml`, and understand how a pack becomes a `socsim` CLI subcommand.
**Estimated time:** 50 minutes.

## Prerequisites

- [T1 — Your first model](01-first-model.md) (`Mechanism`, `SimulationBuilder`, seeds).
- T0 (so `socsim run` / `list` / `sweep` are familiar).

Backing artifacts, both CI-compiled: the library driver [`crates/socsim-packs/examples/hr_baseline.rs`](../../crates/socsim-packs/examples/hr_baseline.rs), and the pack it drives, `HrLifecyclePack` (and the `opinion-dynamics` pack at [`crates/socsim-packs/src/opinion.rs`](../../crates/socsim-packs/src/opinion.rs)).

## Two paths, one engine

So far you added mechanisms directly to a `SimulationBuilder` (T1–T4 — *engine-only* mode). The **full-stack** path inserts two layers between you and the engine: a `ModulePack` (a named bundle of mechanism constructors) and a `Registry` (which builds mechanisms by name from parameters). This is what lets a scenario `.toml` — or the CLI — compose a model without recompiling. See [Two usage paths](../architecture.md#two-usage-paths-scenario-cli-vs-library-mode).

## Steps

### 1. A `ModulePack` registers mechanism constructors

A `ModulePack<W>` has a name and a `register` method that adds named constructors to a `Registry<W>`. Each constructor reads typed parameters and returns a boxed mechanism. The `opinion-dynamics` pack is a compact real example — it registers each opinion mechanism by name:

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

The closure receives a `Params` (a typed, defaulted view over a TOML table): `get_f64("epsilon", 0.2)` reads the scenario's value or falls back to `0.2`. Always supply a default so a scenario that omits the key still works. The reference `HrLifecyclePack` does the same for its ten mechanisms.

### 2. Build mechanisms from the registry (library full-stack)

The shipped `hr_baseline.rs` shows the full-stack path *in Rust*: register a pack into a `Registry`, then build each mechanism by name and add it to the builder:

```rust
// Register all mechanisms.
let mut reg = socsim_config::Registry::new();
HrLifecyclePack.register(&mut reg);

let p = Params::empty();
let mechanism_names = [
    "learning_curve", "peer_effect", "ocb", "fit", "turnover",
    "knowledge_loss", "toxic_spread", "hiring", "socialization", "org_performance",
];

let mut builder = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(SEED)
    .recorder(Box::new(shared_rec));

for name in &mechanism_names {
    let m = reg.build(name, &p).expect("mechanism registered");
    builder = builder.add_mechanism(m);
}

let mut sim = builder.build();
sim.run().expect("simulation completed without error");
```

`reg.build(name, &params)` is the bridge: it looks up the constructor you registered and instantiates the mechanism with those params. This is exactly what the CLI does internally when it reads a scenario's `[[mechanism]]` blocks — only here you spell the names out in Rust.

### Run the library driver

```sh
cargo run -p socsim-packs --example hr_baseline
```

```
=== HR Lifecycle ABM — Baseline Run ===
Teams: 5  |  Initial team size: 8  |  T_max: 60  |  Seed: 42

Initial employees: 40  |  Base mean θ: 0.9516

   t  org_performance    avg_tenure   turnover_rate   knowledge_stock
----------------------------------------------------------------------
   1          6.2045          1.00          0.0000             42.61
   ...
  60         41.8100         35.62          0.0000             92.38
```

### 3. The same model as a scenario `.toml`

Instead of naming mechanisms in Rust, a scenario file names the pack and lists `[[mechanism]]` blocks. This is `scenarios/hr_lifecycle_baseline.toml`:

```toml
[simulation]
name        = "hr_lifecycle_baseline"
module_pack = "hr-lifecycle"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[[mechanism]]
name  = "learning_curve"
phase = "environment"
[mechanism.params]
lambda_learn = 0.15

# ... eight more [[mechanism]] blocks ...

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["org_performance", "avg_tenure", "turnover_rate", "knowledge_stock"]
```

`module_pack` selects the pack; each `[mechanism.params]` table is the `Params` the constructor reads. The array is order-preserving — composition order equals declaration order. Run it via the CLI (same numbers as the Rust driver, because the seed and params match):

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
socsim sweep scenarios/hr_lifecycle_baseline.toml --param "toxic_spread.p_spread=0.2,0.7" --seeds 0..2
```

The full scenario schema and every flag are in the [CLI reference](../cli.md). For task recipes (multi-seed checks, sweeps, resuming runs), see the [Use-cases & recipes](../usecases.md).

### 4. (Optional) expose your pack to the `socsim` binary as a `CliPack`

The three shipped packs appear in `socsim list packs` because each is also wrapped in a **`CliPack`** — an object-safe, world-erased adapter the world-polymorphic binary dispatches on. To add your own pack to the CLI you:

1. implement a `struct FooCliPack;` that `impl CliPack` (it owns your concrete world internally and exposes `name`, `starter_toml`, `mechanism_names`, `run_seeds`, `run_sweep`);
2. add a Cargo feature `pack-foo = ["dep:socsim-foo"]`;
3. gate the impl behind `#[cfg(feature = "pack-foo")]`;
4. push it into the `packs()` registry.

That checklist lives at the top of [`crates/socsim-cli/src/packs.rs`](../../crates/socsim-cli/src/packs.rs). Once wired, your pack shows up in `socsim list packs` and `socsim init --module-pack foo` scaffolds a starter. Until then, the library full-stack path (Step 2) runs any pack you write without touching the CLI binary at all.

## Run it

```sh
cargo run -p socsim-packs --example hr_baseline      # library full-stack
socsim run scenarios/hr_lifecycle_baseline.toml      # same model, scenario TOML
socsim list packs                                    # packs exposed as CliPacks
```

## What you learned

- A **`ModulePack`** registers named mechanism constructors into a **`Registry`**; `reg.build(name, &params)` instantiates them — the indirection that lets TOML/CLI compose models without recompiling.
- **`Params`** gives typed, defaulted reads of a scenario's `[mechanism.params]` table.
- The library full-stack path (`hr_baseline.rs`) and the scenario-TOML path run the *same* registered mechanisms through the same engine.
- A pack reaches the `socsim` binary by implementing **`CliPack`** behind a `pack-*` feature; until then it still runs as a library.

## Next

You've completed the path. From here:

- [Use-cases & recipes](../usecases.md) — task-oriented runbooks for real research workflows.
- [Mechanism catalog](../mechanisms.md) — every shipped mechanism to compose.
- [Architecture](../architecture.md) — the crate graph and the *why* behind the design.
