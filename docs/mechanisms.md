**English** | [日本語](mechanisms.ja.md)

# Mechanism catalog

A **mechanism** is the unit of research logic in socsim. It implements the
[`Mechanism`](library.md) trait — declaring the phase(s) it participates in and
a single `apply` method — and is composed with other mechanisms over the shared
[6-phase tick loop](architecture.md#the-6-phase-tick-loop). Mechanisms compose
like neural-network layers: each reads and writes the `WorldState`, and the
engine runs them in a fixed order every step.

This catalog documents the **eleven** mechanisms that ship with socsim: the ten
reference [HR lifecycle](usecases.md) mechanisms (calibrated against published
empirical findings) and the learnable MARL `policy` mechanism.

## Overview

![Mechanisms across the 6-phase tick loop](assets/mechanisms-overview.svg)

The 6-phase order is fixed: `PreStep → Environment → Decision → Interaction →
Reward → PostStep`. A mechanism declares its phase(s) via `Mechanism::phases`;
within a phase, mechanisms fire in scenario/insertion order. The dashed green
arrows show **shared-state hand-offs** within a single step — e.g. `turnover`
populates `departed_this_step`, which `knowledge_loss` consumes in PostStep.

## The eleven mechanisms

| Mechanism | Phase | Source | Kind | Summary |
|---|---|---|---|---|
| [`learning_curve`](mechanisms/learning-curve.md) | Environment | Bahk & Gort (1993) | empirical | Tenure-driven learning-by-doing raises individual productivity. |
| [`peer_effect`](mechanisms/peer-effect.md) | Interaction | Mas & Moretti (2009) | empirical | Team ability lifts each member's effective productivity. |
| [`ocb`](mechanisms/ocb.md) | Interaction | calibration | tunable | Citizenship behaviour adds to the team knowledge stock. |
| [`fit`](mechanisms/fit.md) | Decision | Kristof-Brown et al. (2005) | empirical | P-J / P-O fit drives job satisfaction. |
| [`turnover`](mechanisms/turnover.md) | Decision | Kristof-Brown (2005) + Krackhardt | mixed | Logistic monthly quit hazard with a network cascade. |
| [`hiring`](mechanisms/hiring.md) | Decision | Schmidt & Hunter (1998) | empirical | Refills teams; selection observes ability through a validity signal. |
| [`socialization`](mechanisms/socialization.md) | PostStep | onboarding model | calibration | Onboards new hires, raising embeddedness. |
| [`knowledge_loss`](mechanisms/knowledge-loss.md) | PostStep | Nonaka (1994) | mixed | Departing veterans drain tacit team knowledge. |
| [`toxic_spread`](mechanisms/toxic-spread.md) | Interaction | Housman & Minor (2015) | empirical | Toxic behaviour spreads along network edges. |
| [`org_performance`](mechanisms/org-performance.md) | Reward | aggregation | — | Aggregates productivity and records the step metrics. |
| [`policy`](mechanisms/policy-mechanism.md) | Decision | MARL (§14.1) | learnable | A learned RL policy as a drop-in Decision mechanism (library-only). |

**Kind** distinguishes *empirical* influence strengths (fixed correlations ρ
from meta-analyses; do not tune) from *tunable* calibration scales that govern
the pace of monthly dynamics. See
[Calibration philosophy](architecture.md#calibration-philosophy).

## How mechanisms are applied

Both usage paths share the same engine and determinism guarantees.

### Scenario TOML (CLI path)

Each `[[mechanism]]` block names a registered mechanism, its `phase`, and
optional `params`. The array is order-preserving — composition order equals
declaration order, and within a phase mechanisms fire in that order.

```toml
[[mechanism]]
name  = "learning_curve"
phase = "environment"
[mechanism.params]
lambda_learn = 0.15
```

Valid `phase` strings: `pre_step`, `environment`, `decision`, `interaction`,
`reward`, `post_step`. Run with `socsim run scenarios/<file>.toml`.

### Library mode

Register a `ModulePack` into a `Registry`, build mechanisms by name, and add
them to a `SimulationBuilder`:

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let m = reg.build("learning_curve", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

See the [Library API](library.md) for writing your own mechanism and the
[CLI reference](cli.md) for the full scenario schema.

## Writing a new mechanism

Implement `Mechanism<W>` for your world type, declare `phases()`, and put your
logic in `apply()`. Register it in a `ModulePack` (for the CLI path) or add it
directly to a `SimulationBuilder` (library mode). Each per-mechanism page below
follows the same structure — theory and source, a data-flow diagram, phase
positioning, the state read/write contract, dependencies, parameters, and how
to apply it — and is a good template to copy.

## Documenting a new mechanism

**Every new mechanism added to the codebase must ship with matching
documentation** so this catalog stays in sync with what is implemented. Use the
[`learning_curve`](mechanisms/learning-curve.md) page and
[`mech-learning-curve.svg`](assets/mech-learning-curve.svg) as the gold-standard
template, and complete this checklist:

1. **English page** — `docs/mechanisms/<slug>.md`, canonical, following the
   eleven-section structure above (Overview; Theory & source with plain-text
   math in ` ```text ` blocks; Data flow; Position in the 6-phase loop; State
   read/write contract; Dependencies & ordering; Parameters — distinguishing
   empirical ρ from tunable scales; How to apply with TOML + library mode;
   Determinism & RNG; Expected behaviour; References). First line is the
   language switcher; include the back-link to this catalog.
2. **Japanese mirror** — `docs/mechanisms/<slug>.ja.md`, switcher
   `[English](<slug>.md) | **日本語**`. Translate prose only; keep code,
   formulas, identifiers, and SVG references verbatim.
3. **Diagram** — hand-author `docs/assets/mech-<slug>.svg` in the shared style:
   the 6-phase strip with the active phase highlighted, a *reads* box → formula
   box → *writes* box, and an `ctx.rng` tag only if it samples randomness.
4. **Catalog** — add a row to the table in **both** `mechanisms.md` and
   `mechanisms.ja.md`, and place the mechanism in its phase column in
   [`mechanisms-overview.svg`](assets/mechanisms-overview.svg).

Match the existing pages' conventions exactly (bilingual, no generated-by
footer).
