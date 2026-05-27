**English** | [日本語](packs.ja.md)

# Module pack catalog

A **module pack** is socsim's unit of *a complete model*. Where a
[mechanism](mechanisms.md) is one piece of research logic, a pack bundles
everything needed to run a whole world: a concrete **world type** (the state
every agent shares), the **mechanisms** that move it forward, a **registration**
function that wires those mechanisms into a [`Registry`](library.md) by name, and
a **starter scenario** TOML you can run immediately.

The `socsim` CLI is **world-polymorphic**: a scenario selects a pack by name
(`[simulation] module_pack = "..."`), and the binary dispatches to that pack's
world type without ever naming a concrete world itself. Two packs ship today.

![Module packs overview](assets/packs-overview.svg)

## The two bundled packs

| Pack | World | What it models | Mechanisms | Page |
|---|---|---|---|---|
| [`hr-lifecycle`](packs/hr-lifecycle.md) | `HrWorld` | An employee-lifecycle organisation: hiring, learning, peer effects, satisfaction, turnover cascades, knowledge loss | ten, calibrated against published empirical findings | [→ hr-lifecycle](packs/hr-lifecycle.md) |
| [`opinion-dynamics`](packs/opinion-dynamics.md) | `OpinionWorld` | Bounded-confidence opinion formation on a social network (consensus, clustering, polarisation) | the `socsim-mechanisms` opinion family, reused via capability traits | [→ opinion-dynamics](packs/opinion-dynamics.md) |

List them at any time from the CLI:

```sh
socsim list packs        # pack names
socsim list mechanisms   # mechanisms grouped by pack
```

## How a pack is structured

Every pack is defined by two traits and a starter TOML:

1. **`ModulePack<W>`** ([`socsim-config`](library.md)) — the research-facing
   interface. It has a `pack_name()` and a `register(&mut Registry<W>)` method
   that adds every mechanism constructor by name. This is what library users
   call directly to activate an entire body of work in one line.

2. **`CliPack`** ([`socsim-cli`](cli.md)) — an object-safe, *world-erased*
   wrapper. It exposes `name()`, `starter_toml()`, `mechanism_names()`,
   `run_seeds()`, and `run_sweep()`, all returning world-agnostic result types.
   Each `CliPack` owns its concrete world internally and monomorphises the
   generic [`socsim-runner`](architecture.md) functions for it, so the CLI
   binary stays free of any one domain model.

Both packs are gated behind a Cargo feature of the same name
(`pack-hr-lifecycle`, `pack-opinion-dynamics`), both on by default, so a
downstream binary can compile in only the packs it needs. See the
[architecture overview](architecture.md#two-usage-paths-scenario-cli-vs-library-mode)
for how the pack layer sits between scenarios and the engine, and the
[T5 — A scenario pack](tutorials/05-scenario-pack.md) tutorial for a hands-on
build of a pack from scratch.

## Two ways to drive a pack

| Path | Entry point | Best for |
|---|---|---|
| **Scenario / CLI** | `module_pack = "..."` in a `.toml`, then `socsim run` | reproducible scenario files, parameter sweeps, no recompile |
| **Library** | `Pack.register(&mut reg)` + `SimulationBuilder` in Rust | custom drivers, embedding socsim in a larger program |

Each pack page below documents both paths, the world's data model, the
mechanism composition across the [6-phase tick loop](architecture.md#the-6-phase-tick-loop),
the starter scenario, and the metrics it records.

## Pages

- [**hr-lifecycle**](packs/hr-lifecycle.md) — the calibrated employee-lifecycle reference module (`HrWorld`, ten mechanisms).
- [**opinion-dynamics**](packs/opinion-dynamics.md) — bounded-confidence opinion dynamics (`OpinionWorld`).

## Adding a new pack

The CLI registry is designed so new packs slot in beside the existing two
without touching the run/sweep/validate/list pipeline:

1. Implement a concrete `World` + its mechanisms (or reuse the
   [`socsim-mechanisms`](mechanisms.md) catalog via capability traits, as the
   opinion pack does).
2. Implement `ModulePack<W>` for it (`pack_name` + `register`).
3. Implement a `struct FooCliPack;` that `impl CliPack`, behind a
   `pack-foo` Cargo feature.
4. Push it into `packs()` in `crates/socsim-cli/src/packs.rs`.

The [T5 tutorial](tutorials/05-scenario-pack.md) walks through steps 1–2 end to
end.

## See also

- [Mechanism catalog](mechanisms.md) — the individual mechanisms a pack composes.
- [CLI reference](cli.md) — `init`, `run`, `validate`, `list`, `sweep`, `summarize`.
- [Architecture](architecture.md) — crate graph, the 6-phase tick loop, calibration philosophy.
- [Use cases & recipes](usecases.md) — runnable workflows for both packs.
