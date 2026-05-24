**English** | [日本語](learning-curve.ja.md)

# Learning curve (`learning_curve`)

> Each employee's productivity rises with tenure through learning-by-doing.
> **Phase:** Environment. **Source:** Bahk & Gort (1993). **Kind:** empirical ($\lambda$).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`learning_curve` is the simplest production mechanism in the HR lifecycle
module. Once per step it ages every employee by one month of tenure and
recomputes their individual productivity contribution as a concave function of
that tenure. New hires start near zero productivity and approach their ability
ceiling $\theta$ as they accumulate experience — the classic *learning-by-doing*
effect documented for new manufacturing plants by Bahk & Gort (1993).

It establishes each employee's baseline `productivity`, which downstream
mechanisms then modulate (`peer_effect`) and aggregate (`org_performance`).

## 2. Theory & source

Learning-by-doing predicts that output per worker grows with cumulative
experience but with diminishing returns, saturating toward a ceiling set by the
worker's underlying ability. socsim models this with a bounded-exponential
(modified-exponential) learning curve:

$$\text{tenure} \leftarrow \text{tenure} + 1, \qquad \pi = \theta \cdot \left(1 - e^{-\lambda \cdot \text{tenure}}\right)$$

- $\theta$ (`Employee.theta`) — the employee's true ability, the productivity ceiling.
- $\lambda$ (`lambda_learn`) — the learning rate; larger $\lambda$ reaches the ceiling faster.
- $\pi$ (`Employee.productivity`) — the effective productivity contribution this step.

At $\text{tenure} = 0$, $\pi = 0$; as $\text{tenure} \to \infty$, $\pi \to \theta$. The marginal gain per month
shrinks geometrically, reproducing the empirical concave learning curve. Bahk &
Gort (1993) decompose plant-level learning into capital, labour, and
organisational components; $\lambda = 0.15$ is set to the midpoint of their reported
range as an industry-average default.

## 3. Data flow

![learning_curve data flow](../assets/mech-learning-curve.svg)

The mechanism reads $\theta$ and the (incremented) `tenure` for each employee and
writes back the new `tenure` and `productivity`. No other state is touched.

## 4. Position in the 6-phase loop

Runs in **Environment**, the second phase, before any agent decisions or
interactions. This is deliberate: productivity is part of the "environment" each
employee acts within during that step, so it must be refreshed first.

- It sets `productivity` to the *individual* baseline $\pi$.
- Later in the same step, `peer_effect` (Interaction) multiplies that baseline
  by a team factor, and `org_performance` (Reward) sums the result.

Because it only depends on per-employee $\theta$ and `tenure`, it has no ordering
constraints with other Environment-phase mechanisms.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `Employee.theta` | ✓ | | Productivity ceiling. |
| `Employee.tenure` | ✓ | ✓ | Incremented by 1 (saturating) each step. |
| `Employee.productivity` | | ✓ | Overwritten with the individual baseline $\pi$. |

## 6. Dependencies & ordering constraints

- **Upstream:** none. Reads only fields owned by each `Employee`.
- **Downstream:** `peer_effect` expects `productivity` to hold the individual
  baseline before it applies the team multiplier; `org_performance` sums the
  final `productivity`. Keeping `learning_curve` in Environment (before
  Interaction and Reward) guarantees that ordering.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `lambda_learn` | `0.15` | empirical (growth rate) | Bahk & Gort (1993), midpoint of reported range |

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "learning_curve"
phase = "environment"
[mechanism.params]
lambda_learn = 0.15
```

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let learning = reg.build("learning_curve", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(learning)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. It iterates `employees.values_mut()` and updates each
employee independently, so the result is order-independent and fully
deterministic for a given world state.

## 10. Expected behaviour

`avg_tenure` rises by roughly one month per step (minus churn from `turnover`),
and each surviving employee's `productivity` climbs along a concave curve toward
$\theta$. In the baseline scenario this is the dominant driver of the early-run
increase in `org_performance` before turnover and hiring reach steady state.

## 11. References

- Bahk, B.-H., & Gort, M. (1993). Decomposing learning by doing in new plants.
  *Journal of Political Economy*, 101(4), 561–583.
