<p align="center"><img src="docs/assets/hero.svg" width="100%"></p>

**English** | [日本語](README.ja.md)

# rs-social-simulation-tools

![Rust 2021](https://img.shields.io/badge/Rust-2021-orange)
![License: MIT](https://img.shields.io/badge/License-MIT-blue)
![tests: 307 passing](https://img.shields.io/badge/tests-307%20passing-brightgreen)

`socsim` is a composable agent-based social simulation platform written in Rust. It provides a trait-based mechanism system, deterministic reproducibility via seeded ChaCha20 RNG, a social-network layer, spatial-grid primitives, world-state snapshots for save/resume, optional learnable (MARL) policies, an optional LLM-agent layer (Ollama/OpenAI with prompt caching), result-output helpers, a reusable observation-metrics library, and a CLI for running, sweeping, and summarising scenarios — all in a fifteen-crate workspace. The CLI is **world-polymorphic**: a scenario selects a *module pack* by name, and three packs ship today — a reference **HR lifecycle** module (ten mechanisms calibrated against published empirical findings), an **opinion-dynamics** pack that runs bounded-confidence consensus models on a social network, and an **`organizational-silence`** pack that models the emergence of a climate of silence on a hierarchical network with LLM- or rule-based voice decisions. Reusable, domain-agnostic mechanisms live in the general **`socsim-mechanisms`** catalog — eight mechanisms across four feature families: opinion dynamics, network contagion, cultural dissemination, and group dynamics.

## Architecture overview

<p align="center"><img src="docs/assets/design-overview.svg" width="100%" alt="socsim at a glance — mechanisms compose like neural-net layers over a deterministic six-phase tick loop on a shared world; a Scheduler and seeded SimRng feed in; a Recorder emits metrics and events."></p>

A `Simulation` engine drives ordered, phase-tagged `Mechanism` layers that read and write a shared `WorldState`; a `Scheduler` and a seeded `SimRng` feed in; a `Recorder` emits metrics and events. The six-phase ribbon (`PreStep → Environment → Decision → Interaction → Reward → PostStep`) is the fixed order in which mechanisms act each step. For the crate-graph implementation view see the [architecture page](docs/architecture.md); for the design rationale behind the abstractions see the [design page](docs/design.md).

## Installation

Build from source (Rust toolchain required):

```sh
git clone https://github.com/akitenkrad/rs-social-simulation-tools.git
cd rs-social-simulation-tools
cargo build --release
```

The binary is placed at `target/release/socsim`.

Run the test suite:

```sh
cargo test --workspace
```

## Quick start

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
```

Sample output:

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

The `socsim` binary is world-polymorphic: scenarios select a **module pack** by name (`socsim list packs`). Three packs ship today — the calibrated `hr-lifecycle` reference module, a general `opinion-dynamics` pack that runs the bounded-confidence mechanisms from `socsim-mechanisms` on a social network, and an `organizational-silence` pack that models the emergence of a climate of silence on a hierarchical organisation:

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

Bounded-confidence opinions coalesce into fewer clusters over time (consensus); a larger `epsilon` drives full consensus.

```sh
socsim run scenarios/org_silence_baseline.toml
```

```
Running 'org_silence_baseline' (pack=organizational-silence, t_max=60, seeds=[42], parallel=false)

t              silence_rate   climate_of_silence       voice_volume      org_performance
10                 0.5500               0.2750             0.2750              28.5630
30                 0.7250               0.4250             0.1500              22.4173
60                 0.6500               0.3500             0.2250              25.9982
```

The climate of silence C(t) rises until the salience shock at t=24, then partly recovers as voicing under high σ feeds the team `knowledge_stock` via the Argyris double-loop learning mechanism.

## Documentation

| Document | Description |
|---|---|
| [Tutorials](docs/tutorials/index.md) | **Start here.** Learning-oriented, follow-along lessons: the CLI, your first model, networks, grids, LLM agents, and scenario packs |
| [Design overview](docs/design.md) | Concepts & design philosophy, core traits/structs, and the 6-phase execution model |
| [CLI reference](docs/cli.md) | Every subcommand, flags, JSONL output format |
| [Use-cases & recipes](docs/usecases.md) | Runbook for common research workflows |
| [Library API](docs/library.md) | Implement custom mechanisms and use socsim as a library |
| [Mechanism catalog](docs/mechanisms.md) | All nineteen mechanisms: theory, sources, diagrams, phase positioning, and how to apply each |
| [Module packs](docs/packs.md) | The three bundled packs (`hr-lifecycle`, `opinion-dynamics`, `organizational-silence`): world data model, mechanism composition, starter scenarios, and recorded metrics |
| [Architecture](docs/architecture.md) | Crate dependency graph, 6-phase tick loop, calibration philosophy |

## License

MIT — see [LICENSE](LICENSE).
