**English** | [日本語](hiring.ja.md)

# Hiring (`hiring`)

> Each team is refilled to its target size by sampling new employees whose ability is drawn from a Normal distribution and filtered through a selection signal.
> **Phase:** Decision. **Source:** Schmidt & Hunter (1998). **Kind:** empirical ($\rho_{\text{SI}}$).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`hiring` runs once per step, after `turnover` has removed quitters, and
refills every team that is below its `target_team_size`. For each vacancy it
draws a candidate's true ability $\theta$ from a calibrated Normal distribution,
constructs a selection signal that blends the standardised ability score with
measurement noise (modelling an imperfect assessment instrument), and then
hires unconditionally. The new employee is connected to up to two existing team
members in the social network and added to `new_hires_this_step`, which the
`socialization` mechanism (PostStep) will drain later in the same step to
initialise their social integration.

`hiring` therefore plays two structural roles: it maintains headcount and seeds
the `new_hires_this_step` buffer that drives onboarding.

## 2. Theory & source

The selection model follows Schmidt & Hunter's (1998) meta-analytic framework
for the validity of personnel selection. The core idea is that a selection
instrument captures only a noisy signal of true ability:

$$\theta \sim \mathcal{N}(\theta_{\text{mean}}, \theta_{\text{sd}}^2), \qquad \theta \leftarrow \max(\theta, \theta_{\text{floor}})$$

$$\text{signal} = \rho_{\text{SI}}\, z_\theta + \sqrt{1 - \rho_{\text{SI}}^2}\;\varepsilon, \qquad z_\theta = \frac{\theta - \theta_{\text{mean}}}{\theta_{\text{sd}}}, \quad \varepsilon \sim \mathcal{N}(0,1)$$

- $\rho_{\text{SI}}$ (0.51) is the empirical selection validity from Schmidt & Hunter
  (1998) — the correlation between the selection signal and true job
  performance. A perfect instrument would give $\rho_{\text{SI}} = 1$; a random one
  would give $\rho_{\text{SI}} = 0$.
- The construction ensures that $\operatorname{Var}(\text{signal}) = 1$ regardless of $\rho_{\text{SI}}$, so
  the signal is properly standardised.
- In the current implementation, hiring is **unconditional**: the signal is
  computed and recorded in the event log but does not yet gate the hire
  decision. This is an intentional modelling choice that leaves room for a
  future threshold or top-k selection policy.

Each new hire is also assigned a `is_toxic` flag, drawn as a Bernoulli with
probability `p_toxic` (0.04), replicating the baseline prevalence reported by
Housman & Minor (2015).

After insertion the hire is connected to up to two randomly selected existing
team members in the Watts–Strogatz social network.

## 3. Data flow

![hiring data flow](../assets/mech-hiring.svg)

For each team deficit, `hiring` samples $\theta$ and $\varepsilon$ from `ctx.rng`, inserts a
new `Employee` record, adds a network node with up to two edges, and appends
the new agent's ID to `new_hires_this_step`. The `socialization` mechanism
then consumes that list later in the same step.

## 4. Position in the 6-phase loop

Runs in **Decision**, the third phase, after `Environment`. Within Decision,
`hiring` must run **after** `turnover` so that:

1. `headcount_at_step_start` (captured by `turnover`) reflects the
   pre-attrition count used by `org_performance`.
2. `hiring` can see the post-attrition team sizes and fill the right number of
   vacancies.

`hiring` must run **before** `socialization` (PostStep), because `socialization`
reads `new_hires_this_step`, which `hiring` populates.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `HrWorld.teams` | ✓ | | Iterated to find under-strength teams. |
| `HrWorld.target_team_size` | ✓ | | Target headcount per team. |
| `HrWorld.employees` | ✓ | ✓ | Read for team counts; new Employee inserted. |
| `HrWorld.network` | | ✓ | New node added; up to 2 edges to existing team members. |
| `HrWorld.new_hires_this_step` | | ✓ | Appended; consumed by `socialization`. |
| `HrWorld.next_id` | ✓ | ✓ | Incremented to generate a fresh `AgentId`. |
| `Employee.theta` | | ✓ | Sampled from N(1.0, 0.2), floored at 0.1. |
| `Employee.is_toxic` | | ✓ | Bernoulli(p_toxic). |

All other `Employee` fields (tenure, socialization, embeddedness, po_fit,
pj_fit, satisfaction, productivity, cum_reward, recent_quit_neighbors) are
initialised to their default values at construction.

## 6. Dependencies & ordering constraints

**Must run after:**
- `turnover` (Decision) — so that vacancies from this step's attrition are
  visible and `headcount_at_step_start` has been captured.

**Must run before:**
- `socialization` (PostStep) — `hiring` populates `new_hires_this_step`;
  `socialization` drains it. Running `socialization` without `hiring` in the
  same step would process an empty list.

**Shared state hand-offs:**

| Producer | Field | Consumer |
|---|---|---|
| `turnover` | vacancies in `employees` / `teams` | `hiring` |
| `hiring` | `new_hires_this_step` | `socialization` |

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `rho_si` | `0.51` | empirical (selection validity) | Schmidt & Hunter (1998) |
| `p_toxic` | `0.04` | empirical (toxic prevalence) | Housman & Minor (2015) |

`THETA_MEAN` (1.0), `THETA_SD` (0.2), and `THETA_FLOOR` (0.1) are compiled
constants and are not currently exposed as scenario parameters.

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "turnover"
phase = "decision"
[mechanism.params]
rho_po_turn       = -0.35
base_quit_logit   = -4.82
quit_embed_sens   =  1.0
quit_sat_sens     =  0.8
quit_cascade_bump =  0.30

[[mechanism]]
name  = "hiring"
phase = "decision"
[mechanism.params]
rho_si  = 0.51
p_toxic = 0.04
```

`hiring` must appear after `turnover` in the TOML to ensure correct within-phase
ordering.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let mut params = Params::empty();
params.set("rho_si",  0.51_f64);
params.set("p_toxic", 0.04_f64);

let hiring = reg.build("hiring", &params)?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(hiring)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

`hiring` draws from `ctx.rng` for every new hire: one `Normal` sample for $\theta$,
one `Normal` sample for $\varepsilon$ (the noise term in the selection signal), and one
`Bernoulli` draw for `is_toxic`. Network edge targets (up to 2 team members)
are also sampled from `ctx.rng`. Because the number of hires per step is
determined by the team-deficit counts — which are themselves deterministic for a
given seed and history — the full draw sequence is reproducible across runs with
the same seed.

## 10. Expected behaviour

In a baseline scenario:

- When `turnover` is active, `hiring` fires most steps to refill teams that
  have lost members, keeping headcount close to `target_team_size ×
  num_teams`.
- New hires enter with `tenure = 0` and near-zero `productivity` (see
  `learning_curve`). The team's mean productivity dips briefly after a wave
  of attrition and refilling, then recovers over the following months as new
  hires climb the learning curve.
- Raising `rho_si` toward 1.0 concentrates incoming $\theta$ values toward
  higher-ability candidates (the signal tracks ability more accurately),
  which gradually lifts `org_performance` in long runs.
- `p_toxic = 0.04` means that roughly 1 in 25 new hires is toxic at entry,
  providing the seed pool for `toxic_spread` (Interaction).

## 11. References

- Schmidt, F. L., & Hunter, J. E. (1998). The validity and utility of selection
  methods in personnel psychology: Practical and theoretical implications of
  85 years of research findings. *Psychological Bulletin*, 124(2), 262–274.
- Housman, M., & Minor, D. (2015). Toxic workers. *Harvard Business School
  Working Paper* 16-057.
