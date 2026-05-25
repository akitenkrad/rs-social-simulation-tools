<p align="center"><img src="docs/assets/hero.svg" width="100%"></p>

**English** | [日本語](README.ja.md)

# rs-social-simulation-tools

![Rust 2021](https://img.shields.io/badge/Rust-2021-orange)
![License: MIT](https://img.shields.io/badge/License-MIT-blue)
![tests: 220 passing](https://img.shields.io/badge/tests-220%20passing-brightgreen)

`socsim` is a composable agent-based social simulation platform written in Rust. It provides a trait-based mechanism system, deterministic reproducibility via seeded ChaCha20 RNG, a social-network layer, spatial-grid primitives, world-state snapshots for save/resume, optional learnable (MARL) policies, an optional LLM-agent layer (Ollama/OpenAI with prompt caching), result-output helpers, and a CLI for running, sweeping, and summarising scenarios — all in a fourteen-crate workspace. A reference HR lifecycle module ships with ten mechanisms calibrated against published empirical findings, and a general opinion-dynamics crate adds the Hegselmann–Krause and Deffuant bounded-confidence mechanisms.

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

## Documentation

| Document | Description |
|---|---|
| [Design overview](docs/design.md) | Concepts & design philosophy, core traits/structs, and the 6-phase execution model |
| [CLI reference](docs/cli.md) | Every subcommand, flags, JSONL output format |
| [Use-cases & recipes](docs/usecases.md) | Runbook for common research workflows |
| [Library API](docs/library.md) | Implement custom mechanisms and use socsim as a library |
| [Mechanism catalog](docs/mechanisms.md) | All thirteen mechanisms: theory, sources, diagrams, phase positioning, and how to apply each |
| [Architecture](docs/architecture.md) | Crate dependency graph, 6-phase tick loop, calibration philosophy |

## License

MIT — see [LICENSE](LICENSE).
