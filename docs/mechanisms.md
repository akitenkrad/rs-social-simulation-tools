**English** | [日本語](mechanisms.ja.md)

# Mechanism catalog

A **mechanism** is the unit of research logic in socsim. It implements the
[`Mechanism`](library.md) trait — declaring the phase(s) it participates in and
a single `apply` method — and is composed with other mechanisms over the shared
[6-phase tick loop](architecture.md#the-6-phase-tick-loop). Mechanisms compose
like neural-network layers: each reads and writes the `WorldState`, and the
engine runs them in a fixed order every step.

This catalog documents the **twenty-eight** mechanisms that ship with socsim:
the ten reference [HR lifecycle](usecases.md) mechanisms (calibrated against
published empirical findings), the nine
[organisational-silence](packs/organizational-silence.md) mechanisms (silence
motives + spiral + threshold cascade on a hierarchical network), the learnable
MARL `policy` mechanism, and the eight social-dynamics mechanisms
(`hegselmann_krause`, `deffuant`, `social_judgement`, `lorenz`, `si_contagion`,
`threshold_contagion`, `axelrod`, `group_conformity`) — the general, non-HR
`socsim-mechanisms` crate.

Mechanisms are composed into runnable models by [module packs](packs.md), which
bundle the world data model, a default mechanism set, and starter scenarios.
The name `org_performance` is registered by **both** [`hr-lifecycle`](packs/hr-lifecycle.md)
and [`organizational-silence`](packs/organizational-silence.md), with
pack-specific bodies: the hr-lifecycle variant aggregates productivity,
tenure, knowledge stock, and turnover rate; the organizational-silence
variant aggregates silence rate, climate of silence, voice volume, knowledge
stock, opinion clusters, and headcount, and fires a `motive_mix` event. Both
share the [`org_performance`](mechanisms/org-performance.md) reference page,
which documents the hr-lifecycle body; the silence variant is documented in
the pack page's §3 row.

## Overview

![Mechanisms across the 6-phase tick loop](assets/mechanisms-overview.svg)

The 6-phase order is fixed: `PreStep → Environment → Decision → Interaction →
Reward → PostStep`. A mechanism declares its phase(s) via `Mechanism::phases`;
within a phase, mechanisms fire in scenario/insertion order. The dashed green
arrows show **shared-state hand-offs** within a single step — e.g. `turnover`
populates `departed_this_step`, which `knowledge_loss` consumes in PostStep.

## The twenty-eight mechanisms

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
| [`org_performance`](mechanisms/org-performance.md) | Reward | aggregation | — | Aggregates productivity and records the step metrics. Also registered by `organizational-silence` with a different body (silence metrics + `motive_mix` event). |
| [`issue_salience`](mechanisms/issue-salience.md) | Environment | scenario-driven | scenario-driven | Updates $\sigma(t)$; supports a mid-run step-function shock for triggering events. |
| [`retaliation_event`](mechanisms/retaliation-event.md) | Environment | Kish-Gephart et al. (2009) | stochastic | Low-probability per-step shock: picks a recent voicer and marks them + their neighbours as retaliated against. |
| [`fear_appraisal`](mechanisms/fear-appraisal.md) | Decision | Kish-Gephart et al. (2009) | empirical | Updates fear from this step's retaliation buffer, decay, and supervisor openness. |
| [`voice_decision_rule`](mechanisms/voice-decision-rule.md) | Decision | Van Dyne (2003) + Edmondson (1999) + Detert & Edmondson (2011) | mixed | Logistic Voice/Silence draw; on Silence assigns Acquiescent / Defensive / Prosocial motive. The LLM variant `voice_decision` shares the page. |
| [`silence_spiral`](mechanisms/silence-spiral.md) | Interaction | Noelle-Neumann (1974) | empirical | Snapshots $\rho_i$ and erodes $\psi$ by $\epsilon \cdot \rho \cdot 0.05$; carries spiral across steps. |
| [`prefalse_cascade`](mechanisms/prefalse-cascade.md) | Interaction | Kuran (1995) / Granovetter (1978) | mixed | Iterative voice-flip cascade on silent dissenters; records a `cascade` event past `cascade_threshold`. |
| [`psafety_update`](mechanisms/psafety-update.md) | PostStep | Edmondson (1999) | empirical | Per-step $\psi$ update from voice and retaliation events. |
| [`climate_silence`](mechanisms/climate-silence.md) | PostStep | Morrison & Milliken (2000) | aggregation | Recomputes $C(t)$ at end of step so the published value reflects PostStep changes. |
| [`org_learning`](mechanisms/org-learning.md) | PostStep | Argyris (1977) | calibration | Double-loop knowledge bump when voicing under salience; otherwise tacit-knowledge decay. |
| [`policy`](mechanisms/policy-mechanism.md) | Decision | MARL (§14.1) | learnable | A learned RL policy as a drop-in Decision mechanism (library-only). |
| [`hegselmann_krause`](mechanisms/hegselmann-krause.md) | Interaction | Hegselmann & Krause (2002, 2005) | bounded-confidence | Synchronous bounded-confidence update toward the chosen mean of opinions within ε (library-only). |
| [`deffuant`](mechanisms/deffuant.md) | Interaction | Deffuant et al. (2000) | bounded-confidence | Pairwise bounded-confidence update: two agents within ε converge by a rate μ (library-only). |
| [`social_judgement`](mechanisms/social-judgement.md) | Interaction | Social Judgement Theory | assimilation–contrast | Assimilate messages inside ε, repel those in the rejection region — drives polarisation (library-only). |
| [`lorenz`](mechanisms/lorenz.md) | Interaction | Lorenz et al. (2021) | assimilation + reinforcement | Assimilation plus a self-reinforcing term that amplifies extreme opinions (library-only). |
| [`si_contagion`](mechanisms/si-contagion.md) | Interaction | SI epidemic model | network contagion | Each active neighbour infects an inactive agent independently with probability β (library-only). |
| [`threshold_contagion`](mechanisms/threshold-contagion.md) | Interaction | Granovetter (1978) | network contagion | An inactive agent activates once its fraction of active neighbours reaches θ (library-only). |
| [`axelrod`](mechanisms/axelrod.md) | Interaction | Axelrod (1997) | cultural dissemination | On each encounter copy one differing feature with probability equal to similarity (library-only). |
| [`group_conformity`](mechanisms/group-conformity.md) | Interaction | DeGroot (1974) | within-group averaging | Each agent moves a fraction α toward its group's mean opinion; groups converge independently (library-only). |

The last eight rows are the members of the general (non-HR)
[`socsim-mechanisms`](architecture.md#crate-workspace) crate — reusable,
domain-agnostic social-dynamics building blocks (opinion dynamics, network contagion,
and cultural dissemination) distinct from the HR-lifecycle and
organisational-silence crates. All are **library-only** (no `ModulePack` /
scenario-TOML registration). The nine organisational-silence rows in the
middle of the table all ship from the
[`organizational-silence`](packs/organizational-silence.md) pack
(plus the optional LLM `voice_decision` variant under the
`pack-organizational-silence-llm` feature).

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
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
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
   eleven-section structure above (Overview; Theory & source with LaTeX math —
   `$$...$$` blocks and inline `$...$`; Data flow; Position in the 6-phase loop; State
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
