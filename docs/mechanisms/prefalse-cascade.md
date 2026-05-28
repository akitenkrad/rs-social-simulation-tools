**English** | [日本語](prefalse-cascade.ja.md)

# Preference-falsification cascade (`prefalse_cascade`)

> Iterate to fixpoint: any silent agent with a critical private concern flips
> to Voice when their neighbour voice ratio exceeds their personal threshold,
> repeated in synchronous rounds until no further flips happen. If the total
> mass that flipped this tick exceeds `cascade_threshold` (default 5 %) of
> the active population, record a `cascade` event with the size and fraction.
> **Phase:** Interaction. **Source:** Kuran (1995); Granovetter (1978). **Kind:** mixed (calibration scale + threshold).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`prefalse_cascade` is the disruptive counter-force to the silence spiral. It
implements the Kuran (1995) preference-falsification cascade on the public
expression channel: agents who privately disagree with the status quo
(`private_concern < 0`) and are publicly silent get an opportunity to flip
to `Voice` once enough of their network neighbours are already voicing.
Because the flip lowers the silence ratio of *that* agent's neighbours, the
mechanism may chain several flips into a single Interaction phase — a
cascade that can flip a large fraction of marginal silent dissenters in one
tick.

Concretely, the apply method repeats a synchronous pass until no candidate
flips on a pass. On each pass it:

1. Builds the candidate set — agents in `Silence` with
   `private_concern < 0` — sorted by `AgentId` for determinism.
2. For each candidate, computes their neighbour voice ratio. If that ratio
   strictly exceeds the agent's `voice_threshold`, the agent is queued for
   flipping at the end of the pass.
3. Applies all queued flips simultaneously (a synchronous round). Their
   `silence_motive` is cleared on the flip.
4. Repeats until a pass produces zero flips.

After the fixpoint, if the total flipped mass exceeded
`cascade_threshold` × `n_employees`, a `cascade` event is recorded with
the absolute size and the fraction.

## 2. Theory & source

Kuran (1995) introduced preference falsification: people misrepresent their
private views in public when the perceived cost of dissent is high, and
collectively this hides the true distribution of preferences from everyone.
A small change in the perceived public consensus — a few new voicers —
can cross many private thresholds at once and unleash a cascade.

Granovetter's (1978) threshold model gives the per-agent formulation: agent
$i$ flips from silent to vocal when the fraction of their neighbours
already voicing exceeds their personal $\theta_i$. socsim adopts this
directly, restricting the candidate set to *silent dissenters* (so agents
who genuinely agree with the status quo don't get swept along), and
iterating until quiescent:

$$\text{candidates}(t) = \{ i : \text{Expression}_i = \text{Silence} \wedge b_i < 0 \}$$

$$\rho_i^{V}(t) = \frac{|\{ j \in N(i) : \text{Expression}_j = \text{Voice}\}|}{|N(i)|}$$

For each candidate $i$, flip if $\rho_i^{V}(t) > \theta_i$ where $\theta_i$
(`Employee.voice_threshold`) is drawn at world construction from
$\mathcal{N}(\text{THETA\_VOICE\_MEAN}, \text{THETA\_VOICE\_SD}^2)$ and
clamped to $[0, 1]$. Apply all flips simultaneously, repeat until no agent
flips. Record a `cascade` event when

$$\frac{\text{total flipped}}{n_{\text{employees}}} > \text{cascade\_threshold}.$$

The cascade threshold (`cascade_threshold`, default `0.05`) sets the *event
detection* sensitivity, not the *cascade trigger*. A small but non-zero
flip mass on a given step does not record an event; only mass cascades —
those that touch at least 5 % of the population — do.

## 3. Data flow

Reads `Employee.expression`, `Employee.private_concern`, and
`Employee.voice_threshold` for every agent, plus `SilenceWorld.network`
adjacencies for the per-agent neighbour voice ratio. Writes
`Employee.expression = Voice` and `Employee.silence_motive = None` for every
flipped agent. Records a `cascade` event with the total flipped count and
the population fraction when the threshold is crossed.

## 4. Position in the 6-phase loop

Runs in **Interaction**, the fourth phase. Within Interaction the bundled
scenario declares `silence_spiral` first and `prefalse_cascade` second. This
ordering is conventional rather than strictly required: the spiral writes
$\rho_i$ (the silence ratio snapshot) and erodes $\psi$, while the cascade
reads $\rho_i^{V}$ (the *voice* ratio, computed fresh from the current
expressions) — the two write to disjoint fields. Declaring the spiral first
matches the design's intent: each step the spiral first quantifies the
silence pressure, then the cascade gets one shot to break it.

By running before `org_performance` (Reward) the cascade's flips are
visible to the macro aggregates of this tick: a successful cascade lowers
`silence_rate` and `climate_of_silence` immediately, rather than next tick.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `Employee.expression` | ✓ | ✓ | Read every pass; flipped to `Voice` at the end of a pass. |
| `Employee.private_concern` | ✓ | | Restricts the candidate set to dissenters ($b < 0$). |
| `Employee.voice_threshold` | ✓ | | Personal threshold $\theta_i$. |
| `Employee.silence_motive` | | ✓ | Set to `None` on a flip. |
| `SilenceWorld.network` | ✓ | | Adjacency list for the neighbour-voice-ratio query. |
| `ctx.recorder` | | ✓ | Records a `cascade` event when flipped mass > `cascade_threshold` of the population. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):** `voice_decision_rule` (or `voice_decision`) must
  have written this step's `Expression`s before the cascade runs; the cascade
  is a *correction* on top of the per-agent decision, not a replacement.
- **Downstream (same step):** `org_performance` (Reward) reads the
  post-cascade `Expression`s when computing `silence_rate`, `voice_volume`,
  and the `motive_mix` event. `psafety_update` (PostStep) reads the same
  expressions when deciding whether a given agent voiced this step.
- **Cross-step:** none. The cascade is fixpoint-iterated within a single
  step.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `cascade_threshold` | `0.05` | tunable (event-detection sensitivity) | Kuran (1995) — "mass cascade" definition |

The per-agent thresholds $\theta_i$ are not parameters of *this* mechanism;
they live in `Employee.voice_threshold`, populated at world construction
from `THETA_VOICE_MEAN` (0.4) and `THETA_VOICE_SD` (0.15). See
[`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs).

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "prefalse_cascade"
phase = "interaction"
[mechanism.params]
cascade_threshold = 0.05      # kuran:1995
```

Raise `cascade_threshold` to make `cascade` events fire less often (a
stricter "mass cascade" definition); lower it to log even small bursts of
flips.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("prefalse_cascade", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. Each inner pass collects candidates into a `Vec`,
sorts that vector by `AgentId`, evaluates every candidate against its own
neighbour voice ratio, queues all flips, and applies them at the end of the
pass — a synchronous round. Because flips within a pass do not see each
other's updates, two runs with the same starting `Expression` distribution
produce bit-identical fixpoint outcomes regardless of `BTreeMap` traversal
implementation.

## 10. Expected behaviour

In the baseline scenario the cascade fires often: with the default
population (40 agents) and the default thresholds, even a handful of
voicers can push a marginal silent dissenter past their threshold, and the
synchronous round amplifies that. A typical seed-0 run records a `cascade`
event on most steps, with flipped fractions in the 0.10–0.20 range. The
cascade therefore acts as a steady erosion of the silence pool that the
spiral and fear updates rebuild between steps.

When `cascade_threshold` is raised to 0.5, the recorded events become rare;
the underlying flips still happen but only the largest cascades are flagged
in the event log. When `voice_threshold` priors are tuned upward (raising
`THETA_VOICE_MEAN` at the source), cascades become harder to trigger and
the long-run silence rate stays higher.

## 11. References

- Granovetter, M. (1978). Threshold models of collective behavior.
  *American Journal of Sociology*, 83(6), 1420–1443.
- Kuran, T. (1995). *Private Truths, Public Lies: The Social Consequences
  of Preference Falsification*. Harvard University Press.
