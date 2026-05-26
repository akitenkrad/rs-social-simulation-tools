**English** | [日本語](socialization.ja.md)

# Socialization (`socialization`)

> New hires receive a random support draw that, combined with their person–organisation fit, determines their initial socialization score and gives them an early boost in embeddedness.
> **Phase:** PostStep. **Source:** onboarding model (calibration). **Kind:** calibration.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`socialization` is the onboarding mechanism for employees hired in the same
step. It runs in **PostStep**, after all Decision- and Interaction-phase
mechanisms have finished, and processes every agent in `new_hires_this_step`
— the list that `hiring` populated earlier in the same step.

For each new hire it draws a random organisational support level, blends it
with the hire's person–organisation fit to produce a `socialization` score
in `[0, 1]`, then uses that score to give a small upward nudge to
`embeddedness`. Once all new hires have been processed the mechanism drains
`new_hires_this_step`, leaving it empty for the next step.

The mechanism is intentionally parameter-free. Its fixed coefficients (0.5/0.5
blend, 0.1 embeddedness increment, `U[0.4, 1.0)` support range) encode the
assumption that early-career support varies across organisations and roles but
is always at least moderately positive, and that even a single month of
onboarding moves embeddedness by a modest, bounded amount.

## 2. Theory & source

The socialization formula blends two components — intrinsic fit and received
support — into an integration index:

$$\text{support} \sim \mathcal{U}[0.4, 1.0)$$

$$\text{socialization} = \operatorname{clip}_{[0,1]}\!\left(0.5\,\text{po\_fit} + 0.5\,\text{support}\right)$$

$$\text{embeddedness} \leftarrow \operatorname{clip}_{[0,1]}\!\left(\text{embeddedness} + 0.1\,\text{socialization}\right)$$

- `po_fit` — the new hire's person–organisation fit, assigned at construction
  and fixed. Employees with higher $\text{po\_fit}$ integrate faster.
- $\text{support}$ — uniform draw in $\mathcal{U}[0.4, 1.0)$. The lower bound of 0.4 reflects
  the assumption that organisations always provide at least some baseline
  onboarding; the upper bound below 1.0 means support is never perfect.
- The equal-weight blend (0.5/0.5) gives the two components symmetric
  influence on socialization.
- The 0.1 embeddedness increment is a small per-step nudge that accumulates
  over subsequent steps through the natural dynamics of the simulation
  (network growth, tenure, etc.) rather than a single large initialisation.
- All values are clamped to $[0, 1]$ to stay within valid ranges.

There is no published citation for this specific functional form; it is a
calibration choice designed to produce realistic onboarding dynamics when
combined with `turnover`'s logistic quit model and the Watts–Strogatz network.

## 3. Data flow

![socialization data flow](../assets/mech-socialization.svg)

`socialization` reads `new_hires_this_step` (populated by `hiring`) and each
new hire's `po_fit` and `embeddedness`. It writes back `socialization` and the
incremented `embeddedness`, then clears `new_hires_this_step`.

## 4. Position in the 6-phase loop

Runs in **PostStep**, the sixth and final phase. This guarantees that:

1. `hiring` (Decision) has already inserted new employees and populated
   `new_hires_this_step` before `socialization` reads it.
2. Interaction-phase mechanisms (`peer_effect`, `ocb`, `toxic_spread`) have
   already run on the existing roster. New hires do not participate in
   Interaction on their first step — they receive socialization first and join
   full interactions from the next step onwards.
3. `knowledge_loss` also runs in PostStep. If both are active, ordering between
   them does not matter because they operate on disjoint state
   (`new_hires_this_step` vs. `departed_this_step`).

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `HrWorld.new_hires_this_step` | ✓ | ✓ | Read to iterate new hires; cleared at end. |
| `Employee.po_fit` | ✓ | | Person–organisation fit, fixed at hire. |
| `Employee.embeddedness` | ✓ | ✓ | Incremented by `0.1 · socialization`, clamped to `[0, 1]`. |
| `Employee.socialization` | | ✓ | Set to `clamp01(0.5·po_fit + 0.5·support)`. |

## 6. Dependencies & ordering constraints

**Must run after:**
- `hiring` (Decision) — `hiring` populates `new_hires_this_step`; without it
  the list is empty and `socialization` is a no-op. In practice, if `hiring`
  is not registered then `new_hires_this_step` will never be populated and
  `socialization` can be omitted safely.

**No downstream dependents within the same step.** The updated `socialization`
and `embeddedness` values are first read by `turnover` and `fit` in the
*following* step.

**Shared state hand-offs:**

| Producer | Field | Consumer |
|---|---|---|
| `hiring` | `new_hires_this_step` | `socialization` |
| `socialization` | clears `new_hires_this_step` | (clean for next step) |
| `socialization` | `Employee.embeddedness` | `turnover` (next step) |
| `socialization` | `Employee.socialization` | `fit` (next step, indirectly) |

## 7. Parameters

`socialization` has **no configurable parameters**. All coefficients are
compiled constants:

| Constant | Value | Role |
|---|---|---|
| support lower bound | `0.4` | Minimum organisational support |
| support upper bound | `1.0` (exclusive) | Maximum organisational support |
| fit weight | `0.5` | Equal blend with support |
| support weight | `0.5` | Equal blend with fit |
| embeddedness increment | `0.1` | Per-step nudge scaled by socialization |

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "hiring"
phase = "decision"
[mechanism.params]
rho_si  = 0.51
p_toxic = 0.04

[[mechanism]]
name  = "socialization"
phase = "post_step"
```

`socialization` takes no `[mechanism.params]` block. It must appear in the
TOML after `hiring`, though because they run in different phases the ordering
constraint is automatically respected by the engine.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let socialization = reg.build("socialization", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(socialization)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

`socialization` draws from `ctx.rng` — one `gen_range(0.4..1.0_f64)` call per
new hire. Because the number of new hires per step is determined by `hiring`
(which is itself deterministic for a given seed), and because `new_hires_this_step`
is an ordered list, the support draws are reproducible across runs with the
same seed in the same sequence.

## 10. Expected behaviour

In a baseline scenario (with `hiring` and `turnover` active):

- Each new hire starts `socialization` with a `po_fit`-dependent floor and a
  random support boost. A hire with $\text{po\_fit} = 0.8$ and $\text{support} = 0.7$ would
  receive $\text{socialization} = \operatorname{clip}_{[0,1]}(0.5 \times 0.8 + 0.5 \times 0.7) = 0.75$.
- The corresponding `embeddedness` bump of $0.1 \times 0.75 = 0.075$ is small but
  meaningful: it lowers the new hire's quit probability in `turnover` by
  roughly $0.075 \times \text{quit\_embed\_sens}$ logit units in the *next* step, reducing
  the chance of immediate re-attrition.
- Without `socialization`, new hires begin with `embeddedness = 0` and have
  markedly higher quit probability in month 1, creating unrealistic "day-one
  regret" spikes in the turnover rate.
- Varying the support range (e.g., `[0.1, 0.5)` for a low-support
  organisation) requires a code-level change to the constants; this is a known
  limitation of the parameter-free design.

## 11. References

No external citation. The functional form is a calibration choice internal to
the socsim-packs hr-lifecycle model.
