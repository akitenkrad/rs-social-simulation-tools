**English** | [日本語](retaliation-event.ja.md)

# Retaliation event (`retaliation_event`)

> A low-probability per-step shock: with probability $p_{\text{retaliate}}$ one
> recent voicer is targeted and every neighbour of that voicer (plus the voicer
> themselves) is marked as retaliated against this step. The marks feed the
> fear update and the psychological-safety update later in the tick.
> **Phase:** Environment. **Source:** Kish-Gephart et al. (2009). **Kind:** stochastic.

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`retaliation_event` is the punctuated-shock channel that injects negative
consequences of voicing into the model. Every step it clears the previous
step's retaliation list, then with probability $p_{\text{retaliate}}$ fires a
single event: it picks a target from among the current voicers (or any agent
as a fallback), marks every network neighbour of that target — plus the target
themselves — as retaliated against this step, and records a `retaliation`
event with the target id and the number of affected agents.

The marks live in the transient buffer `SilenceWorld.retaliation_this_step`,
which `fear_appraisal` reads in the same Decision phase to bump fear, and
`psafety_update` reads in PostStep to nudge perceived psychological safety
downward. The buffer is the canonical hand-off between the punctuated shock
and the agent-level state updates.

## 2. Theory & source

Kish-Gephart, Detert, Treviño & Edmondson (2009) review the empirical evidence
that retaliation — formal or informal sanctions against speaking up — is the
central driver of silence in organisations. Retaliation is *rare per step but
salient when it occurs*: a one-off observation of a colleague being punished
for voicing can leave a multi-month imprint on a workgroup's fear of speaking.
socsim mirrors this with a per-step Bernoulli draw at a low rate
($p_{\text{retaliate}} = 0.05$ in `calibration.rs`) and a network-local impact
radius (the target's direct neighbours):

$$\Pr[\text{event fires at } t] = p_{\text{retaliate}}, \qquad \text{affected}(t) = \{ \text{target} \} \cup N(\text{target})$$

The target is drawn uniformly from the *current voicers* — the cohort whose
public dissent is observable to the organisation. When no agent is voicing
this step (e.g. very early in the run or deep in a silent equilibrium) the
mechanism falls back to drawing uniformly from the full agent population,
modelling the more diffuse case where retaliation targets perceived dissent
that has not yet been publicly voiced.

The affected list is then deduplicated and sorted — `affected.sort();
affected.dedup();` — so the downstream `HashSet` build in `fear_appraisal` is
order-independent regardless of how the network's adjacency list happened to
be allocated.

## 3. Data flow

Reads `SilenceWorld.employees` (filtered to the current voicers) and
`SilenceWorld.network` (for the target's neighbours). Writes
`SilenceWorld.retaliation_this_step` with the deduplicated, sorted list of
affected agents, and records a `retaliation` event when an event fires. The
list is read by `fear_appraisal` in the same step's Decision phase and by
`psafety_update` in PostStep.

## 4. Position in the 6-phase loop

Runs in **Environment**, the second phase. Placing it before Decision
guarantees that `fear_appraisal` reads the freshly-written
`retaliation_this_step` in the very same step. It also runs before the cascade
and the voice-decision, so the heightened fear feeds into agents' choices
immediately rather than next tick.

It has no ordering constraint relative to `issue_salience` within Environment;
the two write to disjoint world fields.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `ctx.clock.t()` | ✓ | | Stamps the recorded event. |
| `ctx.rng` | ✓ | | One Bernoulli draw; on event, one uniform-index draw. |
| `SilenceWorld.employees` | ✓ | | Filtered to current `Voice` expression for the candidate set. |
| `SilenceWorld.agent_ids()` | ✓ | | Fallback candidate set when there are no voicers. |
| `SilenceWorld.network` | ✓ | | Adjacency list for the target's neighbours. |
| `SilenceWorld.retaliation_this_step` | | ✓ | Cleared every step; rewritten on a fire. |
| `ctx.recorder` | | ✓ | Records a `retaliation` event `{target, n_affected}`. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** none. Runs first within Environment after the
  buffer clear.
- **Downstream (same step):**
  - `fear_appraisal` (Decision) reads the buffer to bump fear for every
    affected agent.
  - `psafety_update` (PostStep) reads the buffer to nudge $\psi$ downward for
    every affected agent.
- **Cross-step:** the buffer is overwritten — never accumulated — every step,
  so retaliation does not leak between steps. The persistent imprint of
  retaliation lives instead in the updated `Employee.fear` and `psych_safety`
  fields.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `p_retaliate` | `0.05` | empirical (per-step retaliation probability) | Kish-Gephart et al. (2009) |

The default is `P_RETALIATE` in
[`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs);
the docstring there cites Kish-Gephart et al. (2009).

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "retaliation_event"
phase = "environment"
[mechanism.params]
p_retaliate = 0.05            # kish-gephart:2009
```

Set `p_retaliate = 0.0` to disable the shock channel entirely (a useful
ablation for isolating the spiral and IVT mechanisms).

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("retaliation_event", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **two** RNG values on a fire: one `f64` for the Bernoulli gate, and one
`gen_range(0..candidates.len())` for the target selection. To keep the draw
sequence reproducible across runs, the candidate set is built by iterating
`SilenceWorld.employees` (a `BTreeMap`, so iteration is already sorted by
`AgentId`) before any RNG call; the fallback `agent_ids()` is likewise sorted
inside the world helper. After picking the target, the affected list is
sorted and deduplicated before being written to the buffer, so even mechanisms
downstream that build a `HashSet` are order-independent.

## 10. Expected behaviour

In the bundled `org_silence_baseline.toml` (60 steps, seed 0) the event fires
roughly two to four times across the run — a frequency consistent with the
`p_retaliate = 0.05` Bernoulli rate. Each fire affects on the order of 6–8
agents (the target plus their roughly six Watts–Strogatz neighbours). Fear
ticks upward immediately on the affected cohort, briefly lifting the silence
rate; psychological safety drifts down over the following several steps. Over
many seeds, raising `p_retaliate` shifts the long-run `climate_of_silence`
upward and pushes the steady-state motive mix toward more `Defensive`
silences.

## 11. References

- Kish-Gephart, J. J., Detert, J. R., Treviño, L. K., & Edmondson, A. C.
  (2009). Silenced by fear: The nature, sources, and consequences of fear at
  work. *Research in Organizational Behavior*, 29, 163–193.
