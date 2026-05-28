**English** | [日本語](psafety-update.ja.md)

# Psychological-safety update (`psafety_update`)

> An end-of-step Edmondson update for perceived psychological safety:
> agents who voiced this step nudge $\psi$ upward by `psafety_learn`; agents
> who were retaliated against nudge $\psi$ downward by the same amount.
> **Phase:** PostStep. **Source:** Edmondson (1999). **Kind:** empirical ($\psi$ learning rate).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`psafety_update` closes the within-step loop for perceived psychological
safety. After the Decision, Interaction, and Reward phases have settled the
step's actions, this PostStep mechanism walks every employee and adjusts
their $\psi$ by a fixed learning rate based on two binary signals:

- Did the agent voice this step? Voicing without an observed sanction is
  evidence to the agent that "speaking up is safer than I thought".
- Was the agent retaliated against this step? Being marked in
  `retaliation_this_step` is direct evidence that "speaking up *is*
  punished", regardless of whether the agent voiced.

Both effects use the same learning rate `psafety_learn`. The two signals
are independent — an agent can both voice *and* be retaliated against in
the same step, in which case the net $\Delta \psi$ is zero.

The updated $\psi$ then feeds into next step's `voice_decision_rule` through
the $+\beta_\psi \cdot \psi_i$ term of the voice logit, completing the
empirical learning loop Edmondson (1999) calls "experience-based update of
psychological safety".

## 2. Theory & source

Edmondson's (1999) team-learning study treats psychological safety as an
empirically updatable belief: each observable instance of speaking up
without negative consequence raises team members' perceived safety, while
each observed sanction lowers it. socsim operationalises this as a
two-signal additive update with a shared learning rate:

$$\Delta \psi_i = \underbrace{\eta_\psi \cdot \mathbf{1}[\text{Expression}_i = \text{Voice}]}_{\text{voiced this step}} - \underbrace{\eta_\psi \cdot \mathbf{1}[i \in \text{retaliation\_this\_step}]}_{\text{retaliated against}}$$

$$\psi_i \leftarrow \operatorname{clip}_{[0,1]}(\psi_i + \Delta \psi_i)$$

- $\eta_\psi$ (`psafety_learn`, default 0.1, the constant `PSAFETY_LEARN` in
  `calibration.rs`) — the learning rate. At 0.1, ten consecutive voicings
  without retaliation are enough to drag $\psi$ to its upper bound.
- $\mathbf{1}[\cdot]$ — indicator of the binary signal.
- The result is clamped to $[0, 1]$.

This is a distinct update from the spiral-driven erosion in
`silence_spiral`. The spiral acts on every step regardless of action,
proportionally to $\rho$; this mechanism acts only on the explicit
action–consequence pair of this step.

## 3. Data flow

Reads `Employee.expression` for every agent and
`SilenceWorld.retaliation_this_step` (collected into a `HashSet<AgentId>`
for O(1) lookup). Writes back the updated `Employee.psych_safety` for every
agent. The retaliation buffer is **not** cleared here — that is
`retaliation_event`'s responsibility at the start of next step.

## 4. Position in the 6-phase loop

Runs in **PostStep**, the sixth and final phase. Two reasons:

1. The update is *based on this step's outcomes* — voice decision (set in
   Decision) plus retaliation buffer (set in Environment). Both must have
   run for the signals to be correct.
2. The updated $\psi$ is intended to be visible to *next step's* voice
   decision, not this step's. Running in PostStep matches that intent
   exactly: the new $\psi$ value is the one `voice_decision_rule` reads at
   the next tick's Decision phase.

Within PostStep there is no strict ordering requirement against
`climate_silence` or `org_learning`, but the bundled scenario declares
`psafety_update` first by convention, so the per-agent state is settled
before the world-aggregate `climate_silence` recomputes.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `SilenceWorld.retaliation_this_step` | ✓ | | Built into a `HashSet<AgentId>` for O(1) lookup. |
| `Employee.expression` | ✓ | | Determines the "voiced" signal. |
| `Employee.psych_safety` | ✓ | ✓ | Updated in place; clamped to [0, 1]. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):**
  - `voice_decision_rule` (Decision) or `voice_decision` (Decision) must
    have set `Expression`.
  - `retaliation_event` (Environment) must have populated
    `retaliation_this_step` for this step.
  - `prefalse_cascade` (Interaction) may also have rewritten `Expression`
    to `Voice` for some cascaded agents; the cascade's flips count as
    voicing for the $\psi$ update, which is the intended modelling choice
    (cascading is observable speaking up).
- **Downstream (next step):** `voice_decision_rule` reads
  `Employee.psych_safety` as the $+\beta_\psi$ term of its logit.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `psafety_learn` | `0.1` | empirical (Edmondson 1999 learning rate) | Edmondson (1999) — `PSAFETY_LEARN` |

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "psafety_update"
phase = "post_step"
[mechanism.params]
psafety_learn = 0.1           # edmondson:1999
```

Setting `psafety_learn = 0.0` freezes $\psi$ at its initial draws; the
spiral can still erode it via `silence_spiral`, but actions can no longer
update beliefs.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("psafety_update", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. The two signals are pure functions of state set
earlier in the step. Iteration uses `BTreeMap`-sorted order, and each
update is a per-agent additive operation, so two runs with the same world
state produce identical $\psi$ vectors.

## 10. Expected behaviour

In a steady-voice scenario with rare retaliation, $\psi$ drifts toward 1.0
across the run — every successful voicing pushes the belief upward by
$\eta_\psi$. The drift is bounded by the upper clamp and by the
spiral's per-step erosion in `silence_spiral`, which can subtract a
percentage point on every step.

In a high-retaliation scenario, the asymmetry flips: most steps add zero
or subtract $\eta_\psi$, and $\psi$ trends toward 0. Once at the floor, the
$+\beta_\psi$ term contributes nothing to the voice logit and the system
relies on the supervisor / salience terms to keep any voicing alive at all.

The mechanism is therefore the slow learning loop that complements the
fast within-step action of `voice_decision_rule`: actions write
expressions, expressions update beliefs, beliefs steer the next step's
actions.

## 11. References

- Edmondson, A. C. (1999). Psychological safety and learning behavior in
  work teams. *Administrative Science Quarterly*, 44(2), 350–383.
