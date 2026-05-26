**English** | [日本語](index.ja.md)

# Tutorials

Learning-oriented, follow-along lessons. Each one has you **build and run something** end to end, narrating real code from a CI-compiled example so what you read always matches what compiles. Start at the top and work down.

## The learning path

```
T0 ──▶ T1 ──┬──▶ T2
            ├──▶ T3   ──▶ T5
            └──▶ T4
```

- Do **T0** first to get the tool under your fingers (no Rust).
- Do **T1** next — every library tutorial builds on its concepts.
- **T2 / T3 / T4** branch off T1; take them in any order (they each teach one extra crate family).
- **T5** ties everything into the full-stack CLI path.

| Tutorial | What you build | Mode |
|---|---|---|
| [T0 — Getting started](00-getting-started.md) | Run, sweep, and scaffold scenarios from the `socsim` CLI | CLI, no Rust |
| [T1 — Your first model](01-first-model.md) | A converging "cooling" model from scratch: one `WorldState`, one `Mechanism` | Library |
| [T2 — Opinion dynamics on a network](02-opinion-network.md) | Bounded-confidence consensus, reusing `socsim-mechanisms` + `socsim-net` + `socsim-metrics` | Library |
| [T3 — A spatial grid model](03-spatial-grid.md) | An event-driven voter CA on a lattice, with a spatial metric | Library |
| [T4 — An LLM-driven agent](04-llm-agent.md) | A gossip model whose agents decide via an LLM, kept deterministic | Library + `socsim-llm` |
| [T5 — A scenario pack](05-scenario-pack.md) | Package mechanisms into a `ModulePack`, drive it from scenario TOML | Full-stack |

## Where tutorials fit (Diátaxis)

These tutorials are **learning-oriented**: they teach concepts by having you build something. They are deliberately distinct from the other doc types — reach for those once you know the basics:

- **How-to** — [Use-cases & recipes](../usecases.md): task-oriented runbooks ("how do I run a sweep / resume a run / train a policy") for when you already know the tool.
- **Reference** — [CLI](../cli.md), [Library API](../library.md), [Mechanism catalog](../mechanisms.md): exhaustive descriptions of every flag, trait, and mechanism.
- **Explanation** — [Design](../design.md), [Architecture](../architecture.md): the *why* — the 6-phase model, crate graph, and calibration philosophy.

Tutorials link out to these rather than repeating them, so when a step says "see the CLI reference for every flag", follow it.
