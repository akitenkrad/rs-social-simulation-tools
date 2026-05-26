**English** | [日本語](00-getting-started.ja.md)

# T0 — Getting started

**What you'll build:** nothing in Rust — you'll drive the `socsim` CLI end to end: run two shipped scenarios, list what's available, scaffold a new scenario, and run a parameter sweep.
**Estimated time:** 15 minutes.

## Prerequisites

- A Rust toolchain (`cargo`) — used once to build the binary.
- No Rust knowledge required. We only run commands.

Build the binary once:

```sh
cargo build --release
```

This produces `target/release/socsim`. The commands below assume it is on your path (or call it as `./target/release/socsim`).

## Steps

### 1. Run the HR lifecycle baseline

socsim ships two ready-to-run scenarios in [`scenarios/`](../../scenarios). Run the calibrated HR-lifecycle one:

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
```

It prints a per-step metric table. Each column is a metric the scenario asked for (in `[output] metrics`):

```
Running 'hr_lifecycle_baseline' (pack=hr-lifecycle, t_max=60, seeds=[42], parallel=false)

Seed 42 — 82 events recorded

t             avg_tenure   knowledge_stock   org_performance     turnover_rate
10                9.1000           53.9517           32.1462            0.0000
20               14.6000           62.4468           35.7133            0.0000
30               21.5500           72.5042           40.4270            0.0250
40               25.9000           78.4727           40.2186            0.0000
50               30.0750           85.3493           40.8007            0.0000
60               35.6250           92.3841           41.8100            0.0000
```

Two concepts are already in play. A **scenario** is a `.toml` file naming a **module pack** (here `hr-lifecycle`), a seed, a step count, and a list of mechanisms. **Metrics** are the numeric series the run records each step.

### 2. See what's available: packs and mechanisms

A *pack* is a named bundle of mechanisms the CLI can run. List them:

```sh
socsim list packs
```

```
Available module packs:
  hr-lifecycle
  opinion-dynamics
```

And the mechanisms each pack registers:

```sh
socsim list mechanisms
```

```
Mechanisms by pack:
  [hr-lifecycle]
    fit
    hiring
    knowledge_loss
    learning_curve
    ocb
    org_performance
    peer_effect
    socialization
    toxic_spread
    turnover
  [opinion-dynamics]
    convergence
    deffuant
    hegselmann_krause
    lorenz
    opinion_metrics
    social_judgement
```

A scenario's `[[mechanism]]` blocks may only name mechanisms from its pack.

### 3. Run the opinion-dynamics scenario and watch clusters shrink

```sh
socsim run scenarios/opinion_dynamics_baseline.toml
```

```
Running 'opinion_dynamics_baseline' (pack=opinion-dynamics, t_max=60, seeds=[42], parallel=false)

Seed 42 — 0 events recorded

t               clusters         max_delta              mean            spread          variance
10               22.0000            0.1238            0.5092            0.9769            0.0360
20               18.0000            0.0331            0.5088            0.9769            0.0268
30               15.0000            0.0127            0.5094            0.9769            0.0243
40               12.0000            0.0049            0.5097            0.9769            0.0235
50               12.0000            0.0021            0.5098            0.9769            0.0233
60               12.0000            0.0010            0.5098            0.9769            0.0232
```

Watch the `clusters` column fall (22 → 12): under bounded confidence, agents that are close enough in opinion converge, so distinct opinion clusters merge over time. The `max_delta` column shrinking toward zero shows the system settling.

### 4. Scaffold a new scenario with `init`

You don't have to write a scenario from scratch — `init` emits a starter for any pack:

```sh
socsim init --module-pack opinion-dynamics --out scenarios/my_opinion.toml
```

```
Wrote starter scenario to 'scenarios/my_opinion.toml'
```

Open the file and try editing `epsilon` (the confidence radius) up to `0.4`, then `socsim run scenarios/my_opinion.toml` — a larger `epsilon` drives the agents toward a single cluster (full consensus).

### 5. Sweep a parameter

To ask "how does outcome X change as parameter P varies", use `sweep`. It runs the Cartesian product of the parameter values, each over a seed range:

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "toxic_spread.p_spread=0.2,0.7" \
    --seeds 0..2
```

```
Sweeping 'hr_lifecycle_baseline' over 1 axes × 2 seeds
  toxic_spread.p_spread = [0.2, 0.7]
  combo 0: toxic_spread.p_spread=0.2000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               35.3000      6.2000     29.1000     41.5000      2
knowledge_stock          91.5906      5.6123     85.9783     97.2030      2
org_performance          40.0188      2.2088     37.8100     42.2276      2
turnover_rate             0.0125      0.0125      0.0000      0.0250      2
  combo 1: toxic_spread.p_spread=0.7000
...
Wrote 2 CSV files to 'runs/sweep'
```

Each combo's cross-seed summary is printed and also written as a CSV under `runs/sweep/`.

## Run it

The four commands you just used, in order:

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
socsim list packs
socsim run scenarios/opinion_dynamics_baseline.toml
socsim init --module-pack opinion-dynamics --out scenarios/my_opinion.toml
socsim sweep scenarios/hr_lifecycle_baseline.toml --param "toxic_spread.p_spread=0.2,0.7" --seeds 0..2
```

## What you learned

- A **scenario** (`.toml`) selects a **module pack** and composes its **mechanisms**; **metrics** are the per-step series it records.
- `socsim list packs` / `list mechanisms` show what you can compose.
- Bounded-confidence opinions converge — fewer clusters over time.
- `socsim init` scaffolds a scenario; `socsim sweep` probes how a parameter moves outcomes.
- Every flag of every subcommand is in the [CLI reference](../cli.md).

## Next

[T1 — Your first model](01-first-model.md): drop the TOML and build a model in Rust, one `WorldState` and one `Mechanism` at a time.
