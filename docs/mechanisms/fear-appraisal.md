**English** | [日本語](fear-appraisal.ja.md)

# Fear appraisal (`fear_appraisal`)

> Each employee's fear-of-speaking $f_i$ is nudged up if they (or a neighbour)
> were retaliated against this step, nudged down by a small per-step decay,
> and further dampened by a positive supervisor signal — a per-step appraisal
> step that reads the retaliation buffer set by `retaliation_event`.
> **Phase:** Decision. **Source:** Kish-Gephart et al. (2009). **Kind:** empirical ($\beta_{\text{fear}}$ scale).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`fear_appraisal` consumes the transient `retaliation_this_step` buffer that
`retaliation_event` set in the same Environment phase, and uses it to update
every employee's `fear` field. Three forces are blended every step:

1. **Retaliation shock** — affected agents (target + neighbours, as written
   into the buffer) get a fear bump of size `fear_sensitivity`.
2. **Baseline decay** — a small `DECAY = 0.02` pulls every agent's fear back
   toward zero, so a calm run with no retaliations slowly extinguishes
   accumulated fear.
3. **Supervisor openness bonus** — when an employee's supervisor signals
   positive openness ($u_k > 0$), an `OPEN_BONUS = 0.05 \cdot u_k$ extra
   reduction in fear is applied — modelling the empirical finding that a
   visibly open leader buffers fear of speaking (Detert & Burris 2007).

The updated `fear` then flows into the voice-decision logit through the
$-\beta_f \cdot f_i$ term and (for the LLM variant) into the prompt template.

## 2. Theory & source

Kish-Gephart, Detert, Treviño & Edmondson (2009) treat fear at work as a
dynamic appraisal: it climbs after observable sanctions, drops in calm
climates, and is moderated by leadership cues. socsim implements this as a
clamped additive update with three terms:

$$\text{retaliation\_term}_i = \begin{cases} k_{\text{fear}} & i \in \text{retaliation\_this\_step} \\ 0 & \text{otherwise} \end{cases}$$

$$\text{openness\_term}_i = b_{\text{open}} \cdot \max(0, u_{k(i)})$$

$$f_i \leftarrow \operatorname{clip}_{[0,1]}\!\left( f_i + \text{retaliation\_term}_i - d - \text{openness\_term}_i \right)$$

- $f_i$ (`Employee.fear`) — the agent's fear-of-speaking $\in [0, 1]$.
- $k_{\text{fear}}$ (`fear_sensitivity`, default 0.4) — the additive bump
  applied when the agent was retaliated against this step (the constant
  `FEAR_SENSITIVITY` in `calibration.rs`).
- $d$ (`DECAY = 0.02`) — per-step pull toward baseline; a compile-time
  constant inside the mechanism.
- $b_{\text{open}}$ (`OPEN_BONUS = 0.05`) — per-step extra reduction in fear
  when the supervisor signals positive openness; also a compile-time
  constant.
- $u_{k(i)}$ (`Team.supervisor_openness`) — the supervisor openness of the
  team agent $i$ belongs to; only the positive part is used (a hostile
  supervisor does *not* raise fear in this mechanism — the spiral does that).
- The result is clamped to $[0, 1]$.

The dominant emotional signal in the voice logit is fear, hence
$\beta_{\text{fear}} = 1.5$ in `calibration.rs` — the largest negative
coefficient among the voice-decision predictors.

## 3. Data flow

Reads `SilenceWorld.retaliation_this_step` (set by `retaliation_event`
earlier in the same step), the `Team.supervisor_openness` for every team
(snapshotted into a `Vec<f64>` before the per-agent mutation), and every
agent's current `Employee.fear`. Writes back the updated `Employee.fear` for
every agent. Nothing else is touched.

## 4. Position in the 6-phase loop

Runs in **Decision**, the third phase. Placing it before the voice-decision
mechanism ensures the freshly-updated fear feeds into the voice logit (and
the LLM prompt) on the very same step that retaliation was observed.

Within Decision, the mechanism order in the bundled scenario is
`fear_appraisal` → `voice_decision_rule` (or `voice_decision`). Reordering
these would delay the fear update by one tick and break the
within-step appraisal–decision coupling that the design depends on.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `SilenceWorld.retaliation_this_step` | ✓ | | Built into a `HashSet<AgentId>` for O(1) lookup. |
| `Team.supervisor_openness` | ✓ | | Snapshotted into a `Vec<f64>` before the per-agent mutation loop. |
| `Employee.team` | ✓ | | Index into the supervisor openness snapshot. |
| `Employee.fear` | ✓ | ✓ | Updated in place; clamped to [0, 1]. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** `retaliation_event` (Environment) must have set
  `retaliation_this_step` to the canonical sorted-deduplicated affected list.
  If the retaliation mechanism is disabled, the buffer is empty every step
  and `fear_appraisal` reduces to pure decay + openness bonus.
- **Downstream (same step):**
  - `voice_decision_rule` (or `voice_decision`) reads `Employee.fear` as the
    dominant negative term in the voice logit.
  - `psafety_update` (PostStep) also reads the retaliation buffer; the two
    are independent and write to different fields.
- **Cross-step:** fear persists across steps, so the per-step decay is the
  only path back to baseline. Repeated retaliation events drive fear toward
  the upper bound; sustained positive supervisor openness drains it.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `fear_sensitivity` | `0.4` | calibration scale (tunable) | Kish-Gephart et al. (2009) |

The `DECAY = 0.02` and `OPEN_BONUS = 0.05` constants are compile-time
constants embedded in the mechanism source; they are not exposed via
scenario parameters. The `BETA_FEAR = 1.5` coefficient that consumes the
updated fear in the voice logit lives in `calibration.rs` and is documented
on the [`voice_decision_rule`](voice-decision-rule.md) page.

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "fear_appraisal"
phase = "decision"
[mechanism.params]
fear_sensitivity = 0.4
```

Must appear before `voice_decision_rule` (or `voice_decision`) in the Decision
phase so the fear update is visible to the voice logit on the same tick.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("fear_appraisal", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. The mechanism iterates `ctx.world.employees`
(`BTreeMap`, sorted by `AgentId`) and updates each agent independently from
the snapshotted retaliation set and team-openness vector. Two runs over the
same world state produce bit-identical fear vectors.

## 10. Expected behaviour

With no retaliation events ($p_{\text{retaliate}} = 0$) and slightly positive
average supervisor openness, fear drifts toward zero over a few dozen steps —
the decay and openness bonus dominate. When `retaliation_event` does fire,
the affected cohort sees a step-jump in fear that decays back over the
subsequent ~20 steps. In a sustained-retaliation scenario (e.g.
$p_{\text{retaliate}} = 0.2$) fear stays elevated across most of the run and
the voice logit's negative $-\beta_f \cdot f$ term dominates, driving silence
upward and shifting the motive mix toward `Defensive`.

## 11. References

- Kish-Gephart, J. J., Detert, J. R., Treviño, L. K., & Edmondson, A. C.
  (2009). Silenced by fear: The nature, sources, and consequences of fear at
  work. *Research in Organizational Behavior*, 29, 163–193.
- Detert, J. R., & Burris, E. R. (2007). Leadership behavior and employee
  voice: Is the door really open? *Academy of Management Journal*, 50(4),
  869–884.
