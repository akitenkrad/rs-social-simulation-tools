**English** | [日本語](turnover.ja.md)

# Turnover (`turnover`)

> Each employee decides whether to quit based on embeddedness, satisfaction, person–organisation fit, and network contagion.
> **Phase:** Decision. **Source:** Kristof-Brown et al. (2005) + Krackhardt & Porter (1986). **Kind:** mixed (empirical ρ + tunable logit weights).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`turnover` models voluntary employee attrition. Once per step it walks every
active employee in a fixed random order (`ctx.agent_order`) and draws a
Bernoulli coin whose probability comes from a logistic regression of four
organisational-behaviour predictors: how embedded the employee is in the
organisation, how satisfied they are, their person–organisation fit, and how
many of their network neighbours quit last month (the Krackhardt cascade).

When an employee quits they are removed from both the employee roster and the
social network, and their record is pushed to `departed_this_step`. The cascade
then adjusts every remaining neighbour: each neighbour's
`recent_quit_neighbors` counter is bumped by one and their `embeddedness`
drops by 0.02 (clamped to `[0, 1]`). That updated `recent_quit_neighbors`
value is what `turnover` will read on the *next* step — closing a feedback
loop that can produce clustered quit waves.

`turnover` also captures `headcount_at_step_start` at the very start of its
own run, before any removals, so that `org_performance` (Reward) can compute a
well-defined denominator for the turnover rate.

## 2. Theory & source

The quit decision follows a standard logistic model:

```text
logit = BASE_QUIT_LOGIT
      + QUIT_EMBED_SENS · (1 − embeddedness)
      + QUIT_SAT_SENS   · (1 − satisfaction)
      + ρ_po_turn       · po_fit
      + QUIT_CASCADE_BUMP · recent_quit_neighbors

p_quit = logistic(logit) = 1 / (1 + e^(−logit))

if ctx.rng.gen::<f64>() < p_quit  →  employee quits
```

- `BASE_QUIT_LOGIT` (−4.82) sets the baseline monthly quit rate of roughly
  0.8 % in isolation — calibrated to industry averages.
- `QUIT_EMBED_SENS` (1.0) and `QUIT_SAT_SENS` (0.8) are tunable sensitivity
  weights on the two "push" factors. Higher embeddedness and higher satisfaction
  both lower quit probability.
- `ρ_po_turn` (−0.35) is the empirical correlation between PO fit and turnover
  intention, drawn from the meta-analysis by Kristof-Brown et al. (2005); its
  negative sign reflects that better fit reduces quitting.
- `QUIT_CASCADE_BUMP` (0.30) is a tunable contagion weight. Each neighbour who
  quit last month adds 0.30 to the logit, reproducing the "snowball" pattern
  documented by Krackhardt & Porter (1986).

**Cascade mechanics (post-quit):**  
After determining the full set of quitters for this step, the mechanism first
resets all `recent_quit_neighbors` to 0, then iterates over each quitter's
former neighbour list and increments their `recent_quit_neighbors` counter and
decrements `embeddedness` by 0.02. This two-pass approach (reset-all, then
bump) ensures clean accounting even when multiple quitters share neighbours.

## 3. Data flow

![turnover data flow](../assets/mech-turnover.svg)

`headcount_at_step_start` is captured first; then every employee is evaluated
in `ctx.agent_order`; quitters are removed; finally the Krackhardt cascade
updates their former neighbours. The `departed_this_step` list is consumed
downstream by `knowledge_loss` (PostStep) and by `org_performance` (Reward).

## 4. Position in the 6-phase loop

Runs in **Decision**, the third phase, after `Environment` (where
`learning_curve` sets individual productivity) and before `Interaction` (where
`peer_effect`, `ocb`, and `toxic_spread` operate on the surviving roster).

Within Decision, `turnover` and `hiring` both run. If both are active,
`turnover` should be declared before `hiring` in the scenario TOML so that
(a) `headcount_at_step_start` reflects the pre-attrition count and (b) `hiring`
refills vacancies created by that same step's attrition. Swapping the order
would cause `hiring` to overshoot the team target by the number who later quit
in the same step.

`fit` (also Decision) updates satisfaction before turnover evaluates it, so
`fit` should precede `turnover` in declaration order.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `HrWorld.employees` | ✓ | ✓ | Quitters are removed. |
| `HrWorld.network` | ✓ | ✓ | Nodes and edges for quitters removed. |
| `HrWorld.departed_this_step` | | ✓ | Populated with `(id, θ, tenure, team)` per quitter; consumed by `knowledge_loss` and `org_performance`. |
| `HrWorld.headcount_at_step_start` | | ✓ | Set once at the top of `apply`, before any removal. |
| `Employee.embeddedness` | ✓ | ✓ | Read for quit logit; decremented 0.02 for cascade neighbours. |
| `Employee.satisfaction` | ✓ | | Read for quit logit. |
| `Employee.po_fit` | ✓ | | Read for quit logit. |
| `Employee.recent_quit_neighbors` | ✓ | ✓ | Read for cascade logit term; reset to 0 then incremented for neighbours. |
| `network.neighbors(id)` | ✓ | | Neighbour list used for cascade. |

## 6. Dependencies & ordering constraints

**Must run after:**
- `fit` (Decision) — so that `satisfaction` reflects the current step's
  person–fit update before the quit decision is evaluated.

**Must run before:**
- `hiring` (Decision) — so that `headcount_at_step_start` is the pre-attrition
  figure and hiring can refill vacancies from this step.
- `knowledge_loss` (PostStep) — `turnover` populates `departed_this_step`;
  `knowledge_loss` reads it to compute tacit knowledge drain. Do not run
  `knowledge_loss` in the same step without `turnover` having run first.
- `org_performance` (Reward) — uses `departed_this_step` and
  `headcount_at_step_start` to compute the turnover rate.

**Shared state hand-offs:**

| Producer | Field | Consumer(s) |
|---|---|---|
| `turnover` | `departed_this_step` | `knowledge_loss`, `org_performance` |
| `turnover` | `headcount_at_step_start` | `org_performance` |
| `turnover` | updated `recent_quit_neighbors` | `turnover` (next step) |

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `rho_po_turn` | `−0.35` | empirical | Kristof-Brown et al. (2005), meta-analytic correlation |
| `base_quit_logit` | `−4.82` | tunable | calibrated to ~0.8 % monthly baseline quit rate |
| `quit_embed_sens` | `1.0` | tunable | sensitivity of quit logit to embeddedness deficit |
| `quit_sat_sens` | `0.8` | tunable | sensitivity of quit logit to satisfaction deficit |
| `quit_cascade_bump` | `0.30` | tunable | contagion weight per recently-departed neighbour |

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "fit"
phase = "decision"
[mechanism.params]
rho_pj = 0.20
rho_po = 0.07

[[mechanism]]
name  = "turnover"
phase = "decision"
[mechanism.params]
rho_po_turn       = -0.35
base_quit_logit   = -4.82
quit_embed_sens   =  1.0
quit_sat_sens     =  0.8
quit_cascade_bump =  0.30
```

`turnover` must appear after `fit` and before `hiring` in the TOML to preserve
correct ordering within the Decision phase.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let mut params = Params::empty();
params.set("rho_po_turn",       -0.35_f64);
params.set("base_quit_logit",   -4.82_f64);
params.set("quit_embed_sens",    1.0_f64);
params.set("quit_sat_sens",      0.8_f64);
params.set("quit_cascade_bump",  0.30_f64);

let turnover = reg.build("turnover", &params)?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(turnover)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

`turnover` draws from `ctx.rng` — one `gen::<f64>()` call per employee per
step, in the fixed iteration order defined by `ctx.agent_order`. Because
`agent_order` is a deterministic permutation derived from the simulation seed
at the start of each step, two runs with the same seed produce identical quit
sequences even when `f64` accumulation is involved in the logit.

The Krackhardt cascade is applied in a sorted neighbour order, so its
`embeddedness` decrements are also order-independent.

## 10. Expected behaviour

In a baseline scenario (default parameters, 60-month run):

- Monthly turnover rate fluctuates around 0.8–2 % when satisfaction and
  embeddedness are near their equilibrium values.
- A single cluster quit triggers elevated `recent_quit_neighbors` for the
  neighbours, raising their quit probability the following month. This can
  produce short-lived multi-period cascade bursts visible as spikes in the
  `turnover_rate` time series recorded by `org_performance`.
- Removing `quit_cascade_bump` (set to 0) suppresses the spikes and produces a
  smooth, lower-variance attrition curve.
- `knowledge_loss` converts the `departed_this_step` list into team-level
  tacit-knowledge drain, so high-tenure quitters have an outsized negative
  effect on `knowledge_stock`.

## 11. References

- Kristof-Brown, A. L., Zimmerman, R. D., & Johnson, E. C. (2005). Consequences
  of individuals' fit at work: A meta-analysis of person–job, person–organization,
  person–group, and person–supervisor fit. *Personnel Psychology*, 58(2), 281–342.
- Krackhardt, D., & Porter, L. W. (1986). The snowball effect: Turnover embedded
  in communication networks. *Journal of Applied Psychology*, 71(1), 50–55.
