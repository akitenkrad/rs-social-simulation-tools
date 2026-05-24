**English** | [日本語](knowledge-loss.ja.md)

# Knowledge loss (`knowledge_loss`)

> Departing employees carry away tacit knowledge, permanently reducing their team's knowledge stock.
> **Phase:** PostStep. **Source:** Nonaka (1994). **Kind:** mixed (φ_tacit empirical; κ, β tunable).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`knowledge_loss` models the organisational cost of voluntary turnover that does
not appear in productivity figures until tenure rebuilds: when an employee quits,
a fraction of the cumulative know-how embedded in their team disappears with
them. The tacit component of this knowledge — routines, judgements, and
relationships that resist codification — is disproportionately hard to replace.

The mechanism fires in the PostStep phase, after `turnover` has already removed
employees and added them to `departed_this_step`. For each departure it
calculates the tacit-knowledge loss proportional to the leaver's ability and
tenure, then subtracts it from the team's `knowledge_stock`. It also clears
`departed_this_step`, acting as the canonical end-of-step cleanup for that
buffer.

## 2. Theory & source

Nonaka (1994) argues that organisational knowledge has two inseparable
components: explicit (codifiable, transferable) and tacit (embodied, sticky).
When a knowledge worker leaves, the tacit fraction is largely irretrievable.
socsim operationalises this with a tenure-scaled loss formula:

```text
years = tenure_months / 12
ΔK    = −κ · φ_tacit · |θ| · years^β
team.knowledge_stock = max(0,  team.knowledge_stock − |ΔK|)
```

- `θ` (`Employee.theta`) — the departed employee's ability (absolute value used
  to handle the positive-scale draw).
- `tenure_months` — months of service at departure; converted to years so the
  scale of loss matches the OCB inflow measured in knowledge-units per step.
- `φ_tacit` (`phi_tacit = 0.85`) — empirical tacit-knowledge fraction (Nonaka
  1994); 85 % of a worker's knowledge is tacit and lost on departure.
- `κ` (`kappa_loss = 0.40`) — tunable scale that sizes the typical leaver's
  drain to a few months of team OCB inflow, preventing `knowledge_stock` collapse.
- `β` (`beta_loss = 1.0`) — tunable exponent; at the default of 1.0 loss is
  linear in tenure-years. Values above 1.0 make long-tenured leavers
  disproportionately costly; values below 1.0 compress the tenure effect.

## 3. Data flow

![knowledge_loss data flow](../assets/mech-knowledge-loss.svg)

The mechanism iterates `departed_this_step` (populated by `turnover` earlier in
the same step) and decrements `Team.knowledge_stock` for each leaver. It then
clears `departed_this_step`.

## 4. Position in the 6-phase loop

Runs in **PostStep**, the sixth and final phase. This placement is mandatory:
`departed_this_step` is populated by `turnover` (Decision, phase 3) and read
here; if `knowledge_loss` ran earlier the buffer would be empty. Running last
also allows it to perform the canonical clear of `departed_this_step` without
risk of interfering with other mechanisms that still need the list (such as
`org_performance` in Reward which reads `departed_this_step.len()`).

Among PostStep mechanisms, `knowledge_loss` should be declared **after**
`socialization`, which drains `new_hires_this_step`; the two PostStep mechanisms
share no state but declaring `knowledge_loss` last respects the "cleanup last"
convention.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `HrWorld.departed_this_step` | ✓ | ✓ | Reads `(id, θ, tenure, team)` tuples; cleared at end. |
| `Team.knowledge_stock` | ✓ | ✓ | Decremented per leaver; floored at 0. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** `turnover` (Decision) must have run and populated
  `departed_this_step` with `(id, θ, tenure_months, team_idx)` tuples.
- **Upstream (same step):** `org_performance` (Reward) reads
  `departed_this_step.len()` to compute `turnover_rate` and must therefore run
  **before** `knowledge_loss` clears the buffer. The phase ordering
  (Reward before PostStep) guarantees this automatically.
- **Downstream:** nothing reads the cleared `departed_this_step` after PostStep.
  `ocb` (Interaction) is the counterpart that adds to `knowledge_stock`; the
  balance between OCB inflow and departure outflow governs long-run stock level.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `phi_tacit` | `0.85` | empirical (tacit-knowledge fraction) | Nonaka (1994) |
| `kappa_loss` | `0.40` | tunable (loss scale) | calibration |
| `beta_loss` | `1.0` | tunable (tenure exponent) | calibration |

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "knowledge_loss"
phase = "post_step"
[mechanism.params]
phi_tacit  = 0.85
kappa_loss = 0.40
beta_loss  = 1.0
```

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let kl = reg.build("knowledge_loss", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(kl)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. The computation iterates a pre-collected `departed`
list derived from `departed_this_step`, which is order-independent (each leaver's
loss is computed independently and applied to a separate team slot). The result
is fully deterministic for a given world state.

## 10. Expected behaviour

In a simulation with `turnover`, `ocb`, and `knowledge_loss`, `knowledge_stock`
should reach a roughly stable level once hiring and turnover approach steady
state. A burst of turnover (from the Krackhardt cascade, for example) causes a
visible dip in `knowledge_stock` that recovers over subsequent months as OCB
replenishes the stock and new hires accumulate tenure. Long-tenured leavers
(high `years`) cause disproportionately large drops, especially if `beta_loss`
is tuned above 1.0.

## 11. References

- Nonaka, I. (1994). A dynamic theory of organizational knowledge creation.
  *Organization Science*, 5(1), 14–37.
