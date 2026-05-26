**English** | [日本語](ocb.ja.md)

# Organisational citizenship behaviour (`ocb`)

> Satisfied, well-fitting employees contribute voluntary knowledge to their team,
> incrementing the team's knowledge stock each step.
> **Phase:** Interaction. **Source:** calibration ($\alpha_k$ tunable). **Kind:** tunable.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`ocb` models *organisational citizenship behaviour* — discretionary, extra-role
effort that employees contribute beyond their formal job requirements. In the HR
lifecycle module this behaviour is operationalised as voluntary knowledge
sharing: each step, every employee makes a contribution to their team's
`knowledge_stock` proportional to how satisfied they are and how well they fit
the organisation. Employees who are dissatisfied or misfit contribute little or
nothing; highly satisfied, well-fitting employees can meaningfully accelerate
the team's collective knowledge growth.

The mechanism is intentionally calibration-based rather than tied to a single
empirical study: the scaling factor `$\alpha_k$` is a free parameter used to balance
knowledge accumulation against the knowledge loss imposed by `knowledge_loss`.

## 2. Theory & source

OCB is associated with a broad literature showing that attitudinal outcomes
(satisfaction, organisational commitment, fit) predict extra-role behaviour
(Organ, 1988; Kristof-Brown et al., 2005). socsim's implementation treats this
contribution as a linear product:

$$\Delta K_{\text{team}} = \alpha_k \cdot \text{satisfaction} \cdot \text{po\_fit}$$

- $\text{satisfaction}$ (`Employee.satisfaction`) — the employee's current job satisfaction $\in [0, 1]$, updated
  each step by the `fit` mechanism.
- $\text{po\_fit}$ (`Employee.po_fit`) — person–organisation fit $\in [0, 1]$; employees who identify with the
  organisation's values contribute more.
- $\alpha_k$ (`alpha_k`) — a tunable scaling coefficient (default 0.30); set to
  balance the long-run trajectory of `knowledge_stock` against `knowledge_loss`
  given the modeller's target churn rate.

Employees are processed in ascending `AgentId` order so that floating-point
accumulation into `ΔK_team` is fully deterministic across runs.

## 3. Data flow

![ocb data flow](../assets/mech-ocb.svg)

The mechanism iterates over employees sorted by id, accumulates the knowledge
contribution per team into a temporary buffer, then adds each team's total
contribution to `Team.knowledge_stock`.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the fourth phase, alongside `peer_effect`. The two
mechanisms act on distinct outputs (`productivity` vs `knowledge_stock`) so
their relative ordering within the Interaction phase does not matter for
correctness.

The `fit` mechanism (Decision, phase 3) updates `satisfaction` before this
phase, so `ocb` always reads the freshest satisfaction values from the current
step.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `Employee.satisfaction` | ✓ | | Updated earlier in the same step by `fit`. |
| `Employee.po_fit` | ✓ | | Person–organisation fit; set at hire / by scenario init. |
| `Employee.team` | ✓ | | Index into `HrWorld.teams`. |
| `Team.knowledge_stock` | | ✓ | Incremented by the team's total OCB contribution. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** `fit` (Decision) should run before `ocb` so that
  `satisfaction` reflects the current period's fit evaluation. If `fit` is
  absent, `ocb` uses whatever satisfaction value was last written.
- **Downstream:** `knowledge_loss` (PostStep) reads `Team.knowledge_stock` and
  applies tacit-knowledge loss from departures; the net effect of `ocb` minus
  `knowledge_loss` determines whether the stock grows or shrinks.
- **No dependency** on `peer_effect`; the two share the Interaction phase but
  write to different fields.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `alpha_k` | `0.30` | tunable (balances knowledge gain vs. loss) | calibration |

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "ocb"
phase = "interaction"
[mechanism.params]
alpha_k = 0.30
```

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let ocb = reg.build("ocb", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(ocb)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. Employees are sorted by `AgentId` (ascending) before
accumulation, which guarantees that the floating-point summation into
`knowledge_stock` follows a fixed order and produces bit-identical results
across runs with the same world state.

## 10. Expected behaviour

In a stable workforce with consistently high satisfaction and good
person–organisation fit, `knowledge_stock` should grow steadily each step.
When turnover is low, `knowledge_loss` is small and the stock trends upward.
Under high-churn scenarios (e.g., toxic-spread episodes), satisfaction falls,
OCB contributions shrink, and `knowledge_loss` can exceed OCB gains, causing
the stock to decline. Tuning `alpha_k` upward raises the baseline growth rate;
tuning it downward makes the stock more sensitive to turnover shocks.

## 11. References

- Organ, D. W. (1988). *Organizational Citizenship Behavior: The Good Soldier
  Syndrome*. Lexington Books.
- Kristof-Brown, A. L., Zimmerman, R. D., & Johnson, E. C. (2005). Consequences
  of individuals' fit at work: A meta-analysis of person–job, person–
  organization, person–group, and person–supervisor fit. *Personnel Psychology*,
  58(2), 281–342.
