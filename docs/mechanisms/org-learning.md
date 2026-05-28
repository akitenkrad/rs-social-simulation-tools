**English** | [日本語](org-learning.ja.md)

# Organisational learning (`org_learning`)

> Argyris (1977) double-loop learning expressed as a binary switch each
> step: when at least one employee voiced *and* issue salience is above the
> `salience_floor`, every voicer's team gets a knowledge bump proportional
> to the team's voicer count; otherwise the entire knowledge stock decays
> by a small `decay_rate`. This is the mechanism that turns "voice is
> valuable" into a numeric performance signal over a silent climate.
> **Phase:** PostStep. **Source:** Argyris (1977). **Kind:** calibration (intervention model).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`org_learning` provides the only path by which voicing affects
`team.knowledge_stock` — and hence the recorded `org_performance` metric —
in the organisational-silence pack. Each PostStep it inspects two things:

1. Are there any voicers this step? It counts `Voice` agents per team
   (after the cascade has settled).
2. Is the issue salient enough to matter? It compares
   `SilenceWorld.issue_salience` to `salience_floor`.

If **both** conditions hold (`total_voicers > 0` AND `sigma >
salience_floor`), every team's `knowledge_stock` is incremented by
`learning_rate × team_voicers[i]` — a per-team accumulation proportional
to that team's contribution of voicers. Argyris (1977) calls this
*double-loop learning*: the organisation does not just adjust its
existing routines (single loop) but updates the routines themselves in
response to surfaced concerns.

If either condition fails — a fully silent climate, or a low-salience
period in which surfaced concerns are not consequential — the *entire*
knowledge stock decays by `decay_rate` per team. The decay models
unrenewed tacit knowledge: routines, judgements, and informal practices
that stop being refreshed in a silent organisation.

## 2. Theory & source

Argyris (1977) distinguishes single-loop learning (correcting actions
under fixed assumptions) from double-loop learning (revising the
assumptions themselves), and argues that the absence of voicing keeps
organisations locked in single-loop iteration on outdated norms. socsim
collapses the typology into a binary switch at the team level, with the
saliency gate ensuring that learning only fires when surfaced concerns
are weighty enough to matter:

$$\Delta K_k(t) = \begin{cases} \eta_{\text{learn}} \cdot V_k(t) & V_{\text{total}}(t) > 0 \wedge \sigma(t) > \sigma_{\text{floor}} \\ -\, \delta \cdot K_k(t) & \text{otherwise} \end{cases}$$

$$K_k(t+1) = \max(0, K_k(t) + \Delta K_k(t))$$

- $K_k(t)$ (`Team.knowledge_stock`) — the team's knowledge stock at time $t$.
- $V_k(t)$ — the count of agents in team $k$ with `Expression = Voice` at
  step $t$.
- $V_{\text{total}}(t)$ — the total voicer count across all teams.
- $\eta_{\text{learn}}$ (`learning_rate`, default 0.05) — per-voicer
  knowledge gain in the on-branch.
- $\delta$ (`decay_rate`, default 0.01) — per-step proportional decay in
  the off-branch (≈ 1 %/step).
- $\sigma_{\text{floor}}$ (`salience_floor`, default 0.3) — the salience
  threshold below which voicing doesn't trigger learning.

The on-branch is a *team-local* per-voicer accumulation; the off-branch is
a *global proportional* decay (every team decays by the same fraction).
This asymmetry means that even when only one team is voicing, the global
decay is suppressed for *every* team that step — a deliberate modelling
choice from §5.3 of the design that ties the macro effect of voice to a
whole-org signal.

## 3. Data flow

Reads `Employee.expression` and `Employee.team` for every agent (via
`BTreeMap` iteration), plus `SilenceWorld.issue_salience` and the
team count. Writes `Team.knowledge_stock` for every team — either
incremented per voicer (on-branch) or proportionally decayed (off-branch).

## 4. Position in the 6-phase loop

Runs in **PostStep**, the sixth phase. Two reasons:

1. The expressions used to count voicers must be *final* for the step —
   after the cascade has settled. Running in PostStep guarantees this.
2. The knowledge stock written here is read by *next step's*
   `org_performance` (Reward) when computing $\Pi(t) = K(t) \cdot (1 -
   C(t))$. Running in PostStep places the write between two
   `org_performance` records, so the metric series shows step-by-step
   accumulation rather than a delayed effect.

Within PostStep there is no strict ordering against `psafety_update` or
`climate_silence`; the bundled scenario declares it last by convention so
all per-agent updates settle before the world aggregate moves.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `SilenceWorld.issue_salience` | ✓ | | Compared to `salience_floor`. |
| `Employee.expression` | ✓ | | Counted per team in `BTreeMap` iteration order. |
| `Employee.team` | ✓ | | Index into the per-team voicer counter. |
| `Team.knowledge_stock` | ✓ | ✓ | On-branch: incremented per voicer. Off-branch: proportionally decayed; floored at 0. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):**
  - `voice_decision_rule` (Decision) or `voice_decision` (Decision) writes
    the per-agent `Expression`s.
  - `prefalse_cascade` (Interaction) may rewrite some of those
    `Expression`s to `Voice` — cascade flips count toward the on-branch
    voicer total, which is the intended modelling.
  - `issue_salience` (Environment) writes the $\sigma$ that is read here.
- **Downstream (next step):**
  - `org_performance` (Reward) reads the updated `Team.knowledge_stock` via
    `total_knowledge_stock()` when computing `org_performance` and
    `knowledge_stock` metrics.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `learning_rate` | `0.05` | calibration scale (tunable) | Argyris (1977) |
| `decay_rate` | `0.01` | calibration scale (tunable) | Argyris (1977) — slow tacit-knowledge drift |
| `salience_floor` | `0.3` | calibration scale (tunable) | matches `SIGMA_BASE` so the on-branch only fires post-shock |

All three parameters live as local constants in the mechanism's
`from_params` defaults; they are not exposed as `pub const` in
`calibration.rs` because they encode an *intervention model* rather than
an empirical regularity. The defaults are documented in the
[`OrganizationalSilencePack`](../packs/organizational-silence.md#3-the-ten-mechanisms)
page.

## 8. How to apply

### Scenario TOML

```toml
[[mechanism]]
name  = "org_learning"
phase = "post_step"
[mechanism.params]
learning_rate  = 0.05
decay_rate     = 0.01
salience_floor = 0.3
```

Setting `learning_rate = 0.0` disables the on-branch (knowledge stock
monotonically decays); setting `decay_rate = 0.0` disables the off-branch
(stock never decreases). Lowering `salience_floor` to 0 makes any voice
trigger the on-branch regardless of salience.

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("org_learning", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

Draws **no** randomness. The voicer-counting walk uses `BTreeMap`
iteration (sorted by `AgentId`) and the team-update walk iterates the
`Vec<Team>` in insertion order, so two runs over the same world state
produce identical knowledge stocks.

## 10. Expected behaviour

In the baseline scenario, the on-branch is active for most steps once
the salience shock fires at $t = 24$: the lifted $\sigma$ exceeds
`salience_floor = 0.3`, and the cascade keeps at least a handful of
voicers around. `knowledge_stock` rises roughly linearly from the
post-shock point, and the `org_performance` metric rises with it (since
$C$ is small). Before the shock, the off-branch fires intermittently
whenever a step happens to produce zero voicers — but in the bundled
scenario the cascade usually creates enough voicers to keep the
on-branch active.

Disabling the cascade (or raising the cascade threshold so cascade flips
no longer happen) often produces long off-branch streaks during the
pre-shock period: the knowledge stock decays steadily until the shock
revives voicing.

## 11. References

- Argyris, C. (1977). Double loop learning in organizations. *Harvard
  Business Review*, 55(5), 115–125.
