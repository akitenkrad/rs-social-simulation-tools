**English** | [日本語](peer-effect.ja.md)

# Peer effect (`peer_effect`)

> Each employee's productivity is scaled up by the average ability of their team,
> capturing positive spillovers from high-performing colleagues.
> **Phase:** Interaction. **Source:** Mas & Moretti (2009). **Kind:** empirical (α_peer).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`peer_effect` models the well-documented phenomenon that workers produce more
when surrounded by more-able colleagues. After `learning_curve` has established
each employee's individual baseline productivity `π`, this mechanism applies a
multiplicative team-quality factor: an employee on a team whose average ability
exceeds the organisation-wide mean receives a productivity boost; one on a
below-average team receives a mild penalty.

It is the primary mechanism by which team composition affects output,
complementing `ocb`'s knowledge-stock channel.

## 2. Theory & source

Mas & Moretti (2009) document that cashier scan rates rise significantly when a
high-productivity peer joins the same shift, with the effect strongest for
workers who can observe their peer directly. socsim abstracts this to a team-
level multiplicative factor proportional to the gap between the team's mean
ability and the organisation-wide baseline:

```text
π_eff = π · (1 + α_peer · (team_mean_θ / base_mean_θ))
```

- `π` (`productivity`) — the individual baseline set by `learning_curve`.
- `team_mean_θ` (`Team.mean_theta`) — the mean ability of the employee's team,
  recomputed by `org_performance` at the end of the prior step.
- `base_mean_θ` (`HrWorld.base_mean_theta`) — the organisation-wide ability
  baseline set at initialisation; held constant as a reference denominator.
- `α_peer` (`alpha_peer`) — the empirical peer-effect magnitude (0.17).

When `base_mean_θ ≈ 0` the ratio is treated as 1.0 to avoid division by zero.

The ratio `team_mean_θ / base_mean_θ` is near 1.0 for an average team, above
1.0 for a high-ability team, and below 1.0 for a low-ability one, so the factor
amplifies or attenuates `π` proportionally.

## 3. Data flow

![peer_effect data flow](../assets/mech-peer-effect.svg)

The mechanism reads each employee's current `productivity` and `team` index,
looks up `Team.mean_theta` for that team, and overwrites `productivity` with the
peer-adjusted value. `HrWorld.base_mean_theta` is a read-only constant
throughout the simulation.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the fourth phase. This placement is deliberate:

- `learning_curve` (Environment, phase 2) must have already set each employee's
  individual `π` before `peer_effect` can scale it.
- `org_performance` (Reward, phase 5) then sums the peer-adjusted `productivity`
  values to produce the organisation-level output metric.

Among Interaction-phase mechanisms, `peer_effect` should be declared after any
mechanism that alters `Team.mean_theta` within the same step, though in
practice `mean_theta` is recomputed by `org_performance` at the end of each
step and read here at the start of the next, so the hand-off is cross-step.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `Employee.productivity` | ✓ | ✓ | Overwritten with peer-adjusted value. |
| `Employee.team` | ✓ | | Index into `HrWorld.teams`. |
| `Team.mean_theta` | ✓ | | Set by `org_performance` at prior step's Reward. |
| `HrWorld.base_mean_theta` | ✓ | | Constant reference denominator; set at init. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** `learning_curve` must run first (Environment) to
  establish the individual baseline `π` that `peer_effect` multiplies.
- **Upstream (prior step):** `org_performance` must have recomputed
  `Team.mean_theta` for every team at the end of the previous step. If
  `org_performance` is absent, `mean_theta` values may be stale.
- **Downstream:** `org_performance` (Reward) reads the peer-adjusted
  `productivity` when aggregating `org_performance`.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `alpha_peer` | `0.17` | empirical (peer-effect magnitude) | Mas & Moretti (2009) |

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "peer_effect"
phase = "interaction"
[mechanism.params]
alpha_peer = 0.17
```

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let peer = reg.build("peer_effect", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(peer)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. The update is a pure arithmetic formula applied
independently to each employee. It uses a pre-built `team_means` snapshot
(indexed by team id) so the result is order-independent and fully deterministic
for a given world state.

## 10. Expected behaviour

In a simulation that includes `learning_curve`, `peer_effect`, and
`org_performance`, teams with above-average mean ability should show
systematically higher aggregate productivity than equal-size low-ability teams.
The effect scales with the dispersion of `theta` across teams: if `theta` is
nearly uniform across all employees, the ratio `team_mean_θ / base_mean_θ`
stays near 1.0 and the mechanism is effectively inert. Positive assortative
sorting (via selective hiring or turnover) can amplify the peer effect over
time.

## 11. References

- Mas, A., & Moretti, E. (2009). Peers at work. *American Economic Review*,
  99(1), 112–145.
