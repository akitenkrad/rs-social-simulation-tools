**English** | [日本語](climate-silence.ja.md)

# Climate of silence (`climate_silence`)

> An end-of-step idempotent recomputation of the world-level
> `climate_of_silence` aggregate $C(t)$ — fraction of agents in `Silence`
> with a critical private concern. Acts as the canonical "published value"
> step that reflects every Reward-phase and PostStep change.
> **Phase:** PostStep. **Source:** Morrison & Milliken (2000). **Kind:** aggregation.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`climate_silence` is the bookkeeping mechanism that ensures the world-level
$C(t)$ field is consistent with the agent roster at end of step. It is
strictly an aggregator — it draws no randomness, takes no parameters, and
holds no internal state — but it is a necessary member of the pack: by the
time PostStep runs, the cascade may have flipped agents to `Voice` after
`org_performance` (Reward) already computed $C(t)$, and this mechanism
re-runs the aggregate so the published value matches the end-of-step world.

Concretely, the implementation calls `SilenceWorld::recompute_macro_aggregates`,
which recomputes both `climate_of_silence` and `voice_volume` from the
current `employees` roster.

## 2. Theory & source

Morrison & Milliken (2000) frame the *climate of silence* as an
organisational state — not a per-agent attribute. It is the fraction of
the workforce that holds a critical private view yet remains publicly
silent:

$$C(t) = \frac{|\{ i : \text{Expression}_i = \text{Silence} \wedge b_i < 0 \}|}{|E(t)|}$$

where $|E(t)|$ is the current active employee count. The numerator is the
"concealed dissent" cohort — agents who would speak up under a different
environment. socsim publishes $C(t)$ once per step from this formula; the
mechanism is the canonical recompute point.

The value of $C(t)$ also enters `org_performance`'s Π(t) formula
($\Pi(t) = K(t) \cdot (1 - C(t))$, see
[`org_performance`](org-performance.md) for the hr-lifecycle variant and
the silence-pack §3 footnote), so a stale $C(t)$ would propagate into the
recorded $\Pi(t)$. In the bundled scenario this risk is mitigated because
`org_performance` (Reward) calls `recompute_macro_aggregates` itself before
recording, and `climate_silence` re-publishes after the PostStep mechanisms
have settled — the two points bracket the cascade and the $\psi$ update.

## 3. Data flow

Reads `Employee.expression` and `Employee.private_concern` for every agent
(via `recompute_macro_aggregates`). Writes
`SilenceWorld.climate_of_silence` and `SilenceWorld.voice_volume`. No
events are recorded.

## 4. Position in the 6-phase loop

Runs in **PostStep**, the sixth phase. The placement guarantees the
published $C(t)$ reflects every Reward-phase and PostStep change to
`Expression` (the cascade in Interaction has already settled by then,
and the only PostStep mechanism that could affect the count is the cascade
itself — which has already run). Within PostStep the bundled scenario
declares `climate_silence` after `psafety_update` so the per-agent state
is settled first.

There is no strict ordering requirement between `climate_silence` and
`org_learning`; the latter reads `Team.knowledge_stock`, not the climate
aggregate.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `Employee.expression` | ✓ | | Counted in `BTreeMap` (sorted) order. |
| `Employee.private_concern` | ✓ | | Restricts the numerator to dissenters. |
| `SilenceWorld.climate_of_silence` | | ✓ | $C(t)$ — fraction of `Silence` ∧ `private_concern < 0`. |
| `SilenceWorld.voice_volume` | | ✓ | Recomputed as a side-effect of `recompute_macro_aggregates`. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** every mechanism that may flip `Expression` —
  `voice_decision_rule` (Decision) and `prefalse_cascade` (Interaction) —
  must have run. In PostStep this is automatic.
- **Downstream (same step):** none. `climate_silence` is one of the final
  bookkeeping steps of the tick.
- **Downstream (next step):** `voice_decision_rule` does *not* read
  `SilenceWorld.climate_of_silence` (it reads $\rho_i$ from the per-agent
  snapshot instead), so the climate aggregate is purely a measurement
  channel, not a feedback channel. Researchers consuming the JSONL log do
  read it via the `climate_of_silence` metric series.

## 7. Parameters

None. `climate_silence` is a pure aggregation mechanism with no tunable
parameters; `from_params` ignores all input.

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "climate_silence"
phase = "post_step"
```

No `[mechanism.params]` block is needed.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("climate_silence", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. The recompute walks `employees.values()` in
`BTreeMap` (sorted-by-AgentId) order, counts two scalars, and writes them.
Two runs over the same world state produce identical world aggregates.

## 10. Expected behaviour

`climate_silence` produces no visible trajectory of its own — it only
re-publishes the aggregate already used in `org_performance`. The
PostStep recompute matters when the cascade has flipped agents *after*
the Reward-phase recompute: in that case the JSONL log's
`climate_of_silence` series records the *Reward-phase* value (snapshot
before the PostStep aggregate is recomputed), but any external consumer
reading the final world state at end of run sees the PostStep value. The
mechanism is therefore a *consistency* guarantee for snapshot/resume and
for any library-mode caller inspecting `sim.world()` between ticks.

## 11. References

- Morrison, E. W., & Milliken, F. J. (2000). Organizational silence: A
  barrier to change and development in a pluralistic world. *Academy of
  Management Review*, 25(4), 706–725.
