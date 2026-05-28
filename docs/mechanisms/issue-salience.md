**English** | [日本語](issue-salience.ja.md)

# Issue salience (`issue_salience`)

> The world-level issue salience $\sigma(t)$ is held flat at a baseline level until
> a triggering event lifts it by a fixed delta — a deterministic step-function
> shock that lets a scenario simulate a regulator inquiry, accusation, or
> earnings surprise mid-run.
> **Phase:** Environment. **Source:** scenario-driven (designer-set $\sigma$ trajectory). **Kind:** scenario-driven.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`issue_salience` is the simplest mechanism in the organisational-silence pack:
once per step it writes the world-level scalar $\sigma(t) \in [0, 1]$ that every
voice-decision mechanism reads. It is intentionally not a stochastic process —
the modeller controls when, and by how much, the issue becomes more salient,
making the timing of any "triggering event" reproducible and ablatable.

By default, $\sigma(t)$ stays at `sigma_base` (0.3, a mildly visible concern)
until step `shock_t` (24, mid-run by convention for a 60-month scenario), and
then jumps by `shock_delta` (0.4) to about 0.7 — large enough to flip many
marginal voice-vs-silence agents through the `BETA_SALIENCE` term of the voice
logit. Setting `shock_delta = 0` disables the shock entirely.

## 2. Theory & source

There is no single empirical source: the step-function form is a deliberate
modelling choice from §4 of the design doc that turns "what changes after a
triggering event?" into a clean before/after comparison. The trajectory is

$$\sigma(t) = \begin{cases} \sigma_{\text{base}} & t < t_{\text{shock}} \\ \sigma_{\text{base}} + \delta_{\text{shock}} & t \ge t_{\text{shock}} \end{cases}, \qquad \sigma(t) \leftarrow \operatorname{clip}_{[0,1]}(\sigma(t))$$

- $\sigma_{\text{base}}$ (`sigma_base`, default 0.3) — the pre-shock baseline.
- $t_{\text{shock}}$ (`shock_t`, default 24) — the step at which the shock fires.
- $\delta_{\text{shock}}$ (`shock_delta`, default 0.4) — the additive jump.
- The result is clamped to $[0, 1]$ so the published value remains a valid
  fraction.

A different scenario can rewrite the trajectory (e.g. a slow ramp, a square
wave) by registering a custom mechanism in place of `issue_salience`. The
fixed-shock form here is the default because it is the simplest design that
makes the salience-vs-silence comparative statics interpretable.

## 3. Data flow

Reads the simulation clock and writes the world-level scalar
`SilenceWorld.issue_salience` to a value drawn from the step function above.
No per-agent state is touched and no events are recorded.

## 4. Position in the 6-phase loop

Runs in **Environment**, the second phase. Placing the salience update before
Decision guarantees that the freshly written $\sigma(t)$ is read by every
voice-decision mechanism in the same step — both `voice_decision_rule` and the
LLM variant include the current $\sigma$ in their logit / prompt.

It has no ordering constraint relative to the other Environment-phase
mechanism `retaliation_event`; the two write to disjoint world fields.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `ctx.clock.t()` | ✓ | | Current step index, used to gate the shock. |
| `SilenceWorld.issue_salience` | | ✓ | Set to $\sigma(t)$; clamped to [0, 1]. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** none. The mechanism depends only on the clock.
- **Downstream (same step):**
  - `voice_decision_rule` (Decision) reads `issue_salience` as the $\sigma$ term in its
    logit; place `issue_salience` before it in the scenario.
  - `voice_decision` (Decision, LLM variant) embeds the same $\sigma$ in its prompt.
  - `org_learning` (PostStep) compares $\sigma$ to `salience_floor` when deciding
    whether to grow or decay `knowledge_stock`.
- **Cross-step:** none.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `sigma_base` | `0.3` | calibration scale (tunable) | design §4 |
| `shock_t` | `24` | calibration scale (tunable) | design §4 — mid-run for a 60-month scenario |
| `shock_delta` | `0.4` | calibration scale (tunable) | design §4 — sized to flip marginal agents |

The defaults live as `SIGMA_BASE`, `SHOCK_T`, `SHOCK_DELTA` in
[`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs).

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "issue_salience"
phase = "environment"
[mechanism.params]
sigma_base  = 0.3
shock_t     = 24
shock_delta = 0.4
```

Set `shock_delta = 0.0` to disable the shock and study a stationary-$\sigma$
control. Lower `shock_t` to fire the trigger earlier.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("issue_salience", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. The trajectory is a pure function of the clock and the
three parameters, so two runs with identical parameters write identical
$\sigma(t)$ sequences regardless of seed.

## 10. Expected behaviour

In the baseline scenario `silence_rate` and `climate_of_silence` settle into a
near-stationary level over the first 20–24 steps under
$\sigma = \sigma_{\text{base}}$. After the shock fires at step 24, the lifted
$\sigma$ flows through `voice_decision_rule` and tilts many marginal agents
toward Voice; in the typical baseline run the cascade mechanism then amplifies
that tilt and silence rate drops visibly within a handful of steps. The system
either settles into a new lower-silence equilibrium (if the shock outweighs
the spiral) or relapses (if the spiral and fear feedback dominate). This
post-shock divergence is the comparative statics the design targets.

## 11. References

No external citation. The step-function $\sigma(t)$ trajectory is a modelling
choice; the `BETA_SALIENCE` coefficient that consumes it is grounded in
Morrison (2014) and documented on the
[`voice_decision_rule`](voice-decision-rule.md) page.
