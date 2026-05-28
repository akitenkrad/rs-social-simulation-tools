**English** | [日本語](organizational-silence.ja.md)

# `organizational-silence` pack

> **Organisational silence on a hierarchical network**: employees with heterogeneous silence motives decide whether to voice concerns or stay silent under fear, supervisor openness, and peer-silence pressure — emergence of a climate of silence (Morrison & Milliken 2000).
> **World:** `SilenceWorld`. **Mechanisms:** ten (plus an optional LLM-driven `voice_decision`). **Cargo feature:** `pack-organizational-silence` (on by default); `pack-organizational-silence-llm` adds the LLM variant. **Time unit:** one step ≈ one month.

[← Back to the pack catalog](../packs.md)

## 1. Overview

The `organizational-silence` pack models how a workforce slips into — or escapes
from — a **climate of silence**: the state in which a large fraction of
employees privately disagree with the status quo yet publicly keep quiet
(Morrison & Milliken 2000). It is the social-organisational counterpart of the
[`opinion-dynamics`](opinion-dynamics.md) pack: both watch emergent collective
behaviour on a network, but where opinion dynamics aggregates a single scalar
opinion, this pack tracks a richer per-agent state — fear (Kish-Gephart et al.
2009), perceived psychological safety (Edmondson 1999), implicit voice theory
(Detert & Edmondson 2011), and one of three silence motives (acquiescent,
defensive, prosocial — Van Dyne, Ang & Botero 2003) — on top of a hierarchical
organisation with supervisor signals and stochastic retaliation events.

Two features differentiate this model from a plain opinion-dynamics network:

- **Hierarchy and supervisor signals.** Employees live on teams led by a
  supervisor whose openness `u_k ∈ [-1, 1]` is the dominant proximal cue
  agents read before voicing. The pack also offers a `supervisor_homogeneity`
  knob that interpolates between identical supervisors across the
  organisation and a uniformly spread distribution — directly testing whether
  uniform leadership signals make global silence emergence more likely (Sohn 2022).
- **Threshold cascades and spiral perception.** A Granovetter / Kuran (1995)
  preference-falsification cascade can flip many silent dissenters to voice in
  a single round once neighbour-voice ratios exceed personal thresholds, while
  the Noelle-Neumann (1974) spiral of silence runs in the opposite direction
  by eroding psychological safety where local silence is dense.

The pack also exposes the **LLM / rule toggle** as a built-in ablation: the
Decision phase can run either the calibrated logistic rule or an LLM-driven
voice decision (`voice_decision`) by changing one mechanism name in the
scenario TOML. The world preserves the four macro variables the model is
calibrated against — issue salience σ(t), climate of silence C(t), voice
volume V(t), and organisational performance Π(t) — so reproducing Milliken
2003's "~85% ever silent" or Detert & Edmondson 2011's "~50% silencing among
high implicit-voice-theory employees" only requires reading off the standard
metric series.

## 2. The world: `SilenceWorld`

`SilenceWorld` owns the shared state every mechanism reads and writes:

| Field | Type | Models |
|---|---|---|
| `clock` | `SimClock` | the simulation clock |
| `employees` | `BTreeMap<AgentId, Employee>` | the live roster (sorted by id for determinism) |
| `teams` | `Vec<Team>` | per-team supervisor openness and `knowledge_stock` |
| `network` | `SocialNetwork` | a [Watts–Strogatz](../architecture.md) small-world tie graph |
| `issue_salience` | `f64` | σ(t) ∈ [0, 1]: how visible/serious the focal issue is right now |
| `climate_of_silence` | `f64` | C(t): fraction of agents in `Silence` ∧ `private_concern < 0` |
| `voice_volume` | `f64` | V(t): fraction of agents currently in `Voice` |
| `org_performance` | `f64` | Π(t) = `knowledge_stock` · (1 − C(t)) |
| `retaliation_this_step` | `Vec<AgentId>` | transient: this step's retaliated agents → consumed by `fear_appraisal` and `psafety_update` |

Each **`Employee`** carries the behavioural state the ten mechanisms act on:
`level` (1 = frontline … L = executive), `tenure` (months), `team` index,
`private_concern` ∈ [-1, 1] (negative ⇒ critical of the status quo), the
current public `expression` ∈ {`Voice`, `Silence`, `Neutral`}, the
empirical-trait scalars `fear` (Kish-Gephart 2009),
`psych_safety` (Edmondson 1999), `ivt_strength` (Detert & Edmondson 2011), a
personal `voice_threshold` (Kuran 1995) that gates the cascade, the assigned
`silence_motive` ∈ {`Acquiescent`, `Defensive`, `Prosocial`} (Van Dyne 2003),
and a per-step snapshot `neighbor_silence_ratio` ρ_i that carries the spiral
effect between phases. Each **`Team`** holds the supervisor's
`supervisor_openness` and a `knowledge_stock` updated by `org_learning`.

The world is built by
`SilenceWorld::new(n_teams, team_size, n_levels, ws_k, ws_beta, supervisor_homogeneity, &mut rng)`
from a seeded [`SimRng`](../architecture.md), so a given seed always produces
the same starting organisation. Determinism inside each mechanism follows the
same `sort-by-AgentId` pattern as the [`hr-lifecycle`](hr-lifecycle.md) pack:
every per-agent iteration that draws from the RNG or accumulates `f64` sorts
its candidate set first, and `BTreeMap` iteration is sorted by key.

### Hierarchy & supervisor homogeneity

The `supervisor_homogeneity` parameter η ∈ [0, 1] interpolates between two
extremes of leadership uniformity:

- **η = 1** — every team is assigned the same baseline supervisor openness
  (≈ 0, a common mean). All supervisors send the same signal, so the silence
  spiral is unobstructed by any maverick team.
- **η = 0** — supervisor openness is spread uniformly in `[-1, 1]`, giving
  the organisation a mix of hostile, neutral, and open supervisors.

Intermediate η linearly blends the two. This parameter is the design's
direct knob for the Sohn (2022) finding that homogeneous supervisor signals
make global silence emergence more likely: at high η the few outliers that
could break a spiral are eliminated, while at low η a sub-population of open
supervisors keeps voice alive.

## 3. The ten mechanisms

The pack registers ten rule-based mechanisms across the six socsim phases. An
optional eleventh, `voice_decision`, replaces `voice_decision_rule` when the
`pack-organizational-silence-llm` feature is enabled (§3.1).

| Mechanism | Phase | Kind | Role |
|---|---|---|---|
| `issue_salience` | Environment | scenario-driven | Updates σ(t); supports a step-function shock (`shock_t`, `shock_delta`) for a mid-run triggering event. |
| `retaliation_event` | Environment | stochastic | With probability `p_retaliate` per step, picks a recent voicer (or any agent as fallback), marks them and their neighbours as retaliated against, and records a `retaliation` event (Kish-Gephart 2009). |
| `fear_appraisal` | Decision | empirical | Updates each employee's fear from this step's retaliation set and supervisor openness; small per-step decay returns fear to baseline in calm climates (Kish-Gephart 2009). |
| `voice_decision_rule` | Decision | mixed | Rule-based logistic voice/silence decision; on silence, assigns a motive ∈ {`acquiescent`, `defensive`, `prosocial`} by dominant suppressor (Van Dyne 2003). |
| `silence_spiral` | Interaction | empirical | Snapshots each employee's neighbour silence ratio ρ_i and nudges psych_safety down by `epsilon · ρ · 0.05`; the ρ_i snapshot is the carrier of the spiral effect into the next tick (Noelle-Neumann 1974). |
| `prefalse_cascade` | Interaction | mixed | Iterative voice-flip cascade: any silent agent with `private_concern < 0` flips to Voice when their neighbour voice ratio exceeds their personal `voice_threshold`, repeated to fixpoint. Records a `cascade` event when the total flipped mass exceeds `cascade_threshold` (default 5%) of the population (Kuran 1995 / Granovetter 1978). |
| `org_performance` | Reward | aggregation | Refreshes the macro aggregates and records `silence_rate`, `climate_of_silence`, `voice_volume`, `knowledge_stock`, `org_performance`, `opinion_clusters`, `n_employees`; fires a `motive_mix` event with the (acquiescent, defensive, prosocial) breakdown over currently-silent agents. |
| `psafety_update` | PostStep | empirical | Nudges each employee's ψ up by `psafety_learn` if they voiced, down by `psafety_learn` if they were retaliated against (Edmondson 1999). |
| `climate_silence` | PostStep | aggregation | Recomputes C(t) so the published value reflects the end-of-step world after the cascade and any other Reward-phase / PostStep changes. |
| `org_learning` | PostStep | optional intervention | Argyris (1977) double-loop learning: when at least one employee voiced *and* σ(t) > `salience_floor`, each voicer's team gets a `learning_rate` increment to its `knowledge_stock`; otherwise the entire stock decays by `decay_rate` (≈ 1%/step) reflecting unrenewed tacit knowledge in a silent climate. |

The full equations, parameter defaults, and citations live in
[`crates/socsim-packs/src/organizational_silence/mechanisms.rs`](../../crates/socsim-packs/src/organizational_silence/mechanisms.rs)
and [`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs).

### 3.1 LLM variant `voice_decision`

The pack ships a second voice-decision mechanism registered under the
canonical name `voice_decision` (the rule-based version keeps
`voice_decision_rule`, so both can coexist in the registry). It is feature-gated
behind `pack-organizational-silence-llm` at the CLI level
(`organizational-silence-llm` in the `socsim-packs` crate); enabling either
feature surfaces the extra mechanism in `socsim list mechanisms`.

In production the LLM mechanism is wired to
[`socsim-llm`](../architecture.md#llm-layer-socsim-llm)'s `LiveClient`,
assembled Ollama-first → OpenAI-fallback → caching from environment variables
the same way as every other socsim LLM mechanism. The per-call `LlmConfig`
uses `temperature = 0` and the scenario seed, and a JSON-file-backed
`PromptCache` turns a warm replay into a deterministic oracle. Tests inject a
`socsim_llm::mock::ScriptedClient` via the `from_client` constructor instead.

The mechanism builds a structured persona-and-context prompt — level,
tenure, fear, psych_safety, ivt_strength, neighbour silence ratio, supervisor
openness, retaliated_this_step, issue_salience — and asks the model to
respond with a single-line JSON object:

```json
{"decision": "VOICE"|"SILENCE", "motive": "acquiescent"|"defensive"|"prosocial"|null, "rationale": "..."}
```

Parsing is case-tolerant and falls back to (`Silence`, `Defensive`) on any
parse failure, so one misbehaving call never aborts the run.

Switching between the rule and the LLM is the pack's primary ablation
pivot: it is a single mechanism-name change in the scenario TOML
(`voice_decision_rule` ↔ `voice_decision`). Caveats from the design carry
over: the model is sensitive to the wording of the ψ prompt, every tick adds
N LLM calls (one per agent, modulo cache hits), and the cache hit rate
depends on how finely the prompt context is discretised. We recommend Ollama
local for production sweeps and a frontier-model run for the verification
subset.

## 4. The pipeline & metrics

One tick of the model walks the standard
[6-phase loop](../architecture.md#the-6-phase-tick-loop). The starter
scenario assigns the ten mechanisms as follows:

- **Environment** — `issue_salience` updates σ(t); `retaliation_event` may
  fire and prepare the `retaliation_this_step` buffer.
- **Decision** — `fear_appraisal` reads the retaliation buffer and updates
  fear, then `voice_decision_rule` (or `voice_decision` under the LLM
  feature) draws Bernoulli(p) for every agent in `ctx.agent_order` and
  assigns motives on silence.
- **Interaction** — `silence_spiral` snapshots ρ_i and nudges ψ; then
  `prefalse_cascade` runs to fixpoint on the silent-with-negative-concern
  agents.
- **Reward** — `org_performance` refreshes aggregates and records every
  metric and the `motive_mix` event for the step.
- **PostStep** — `psafety_update` adjusts ψ from voice / retaliation
  experience, `climate_silence` re-publishes C(t), and `org_learning`
  applies the knowledge gain or decay.

`org_performance` records seven metrics every step and fires three named
events:

| Metric / event | Meaning |
|---|---|
| `silence_rate` | Fraction of employees currently in `Silence`. |
| `climate_of_silence` | C(t): fraction in `Silence` ∧ `private_concern < 0`. |
| `voice_volume` | V(t): fraction currently in `Voice`. |
| `ever_silent_fraction` | Cumulative fraction of agents who have been in `Silence` at least once during the run (computed from the per-step `silence_rate` series). |
| `knowledge_stock` | Sum of `team.knowledge_stock` across all teams. |
| `org_performance` | Π(t) = `knowledge_stock` · (1 − C(t)). |
| `opinion_clusters` | Number of distinct `private_concern` clusters within tolerance `cluster_tol` (default 0.05). |
| `n_employees` | Current active headcount. |
| `retaliation` (event) | Fired by `retaliation_event`; payload `{target, n_affected}`. |
| `cascade` (event) | Fired by `prefalse_cascade` when the flipped mass exceeds `cascade_threshold`; payload `{size, fraction}`. |
| `motive_mix` (event) | Fired each step by `org_performance`; payload `{acquiescent, defensive, prosocial, no_motive}` over currently-silent agents. |

Since PR #52 these events also surface in the JSONL run log (`type:"event"`
records) alongside the per-step metrics, so a single `*.jsonl` file is the
full reproducible record of a run.

## 5. Calibration anchors

The headline empirical anchors live as `pub const` in
[`crates/socsim-packs/src/organizational_silence/calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs)
with inline citations. They split into logistic coefficients of the voice
decision, prior-distribution scales for per-agent initial state, update
rates, and two *calibration targets* a baseline run is expected to reproduce.

| Anchor | Value | Source / role |
|---|---|---|
| `BETA_PSAFETY` | `1.2` | Edmondson (1999) — ψ → voice coefficient |
| `BETA_FEAR` | `1.5` | Kish-Gephart et al. (2009) — fear → silence (subtracted) |
| `BETA_IVT` | `0.8` | Detert & Edmondson (2011) — implicit voice theory → silence |
| `BETA_SUP` | `1.0` | Detert & Burris (2007) / Morrison (2014) — supervisor openness → voice |
| `BETA_SALIENCE` | `1.0` | Morrison (2014) — salience → voice |
| `BETA_CLIMATE` | `1.5` | Noelle-Neumann (1974) / Sohn (2022) — spiral of silence (subtracted) |
| `BETA_0` | `-0.5` | calibration scale — intercept tuned so an average agent mildly skews silent |
| `F_MEAN`, `F_SD` | `0.4`, `0.2` | Kish-Gephart et al. (2009) — initial fear prior `N(0.4, 0.2)` |
| `PSAFETY_MEAN`, `PSAFETY_SD` | `0.5`, `0.2` | Edmondson (1999) — initial ψ prior |
| `THETA_VOICE_MEAN`, `THETA_VOICE_SD` | `0.4`, `0.15` | Kuran (1995) — voice-threshold prior |
| `P_RETALIATE` | `0.05` | Kish-Gephart et al. (2009) — per-step retaliation probability |
| `FEAR_SENSITIVITY` | `0.4` | calibration scale — fear bump per retaliation |
| `EPSILON_SPIRAL` | `0.25` | Noelle-Neumann (1974) — spiral perception magnitude |
| `PSAFETY_LEARN` | `0.1` | Edmondson (1999) — ψ learning rate |
| `EVER_SILENT_TARGET` | `0.85` | Milliken, Morrison & Hewlin (2003) — **target**: ≥1 issue silenced over a 6-month window |
| `HICO_SILENCE_TARGET` | `0.50` | Detert & Edmondson (2011) — **target**: silence rate among HiCo (high IVT) employees |

Constants marked "calibration scale" are tunable knobs chosen so the run
reproduces the two empirical targets; the
[architecture page](../architecture.md#calibration-philosophy) explains the
empirical-vs-tunable split for socsim packs in general.

## 6. How to apply

### Scenario / CLI

Generate the starter scenario and run it:

```sh
socsim init --module-pack organizational-silence --out scenarios/os.toml
socsim run scenarios/os.toml
```

The bundled `scenarios/org_silence_baseline.toml` runs the rule-based
voice-decision on a 5-team × 8-employee × 3-level organisation over 60
monthly steps, with the salience shock at month 24:

```toml
[simulation]
name        = "org_silence_baseline"
module_pack = "organizational-silence"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[world]
n_teams                = 5
team_size_initial      = 8
n_levels               = 3
network_model          = "watts_strogatz"
network_k              = 6
network_beta           = 0.1
supervisor_homogeneity = 0.5

[[mechanism]]
name  = "issue_salience"
phase = "environment"
[mechanism.params]
sigma_base  = 0.3
shock_t     = 24
shock_delta = 0.4

[[mechanism]]
name  = "retaliation_event"
phase = "environment"
[mechanism.params]
p_retaliate = 0.05            # kish-gephart:2009

[[mechanism]]
name  = "fear_appraisal"
phase = "decision"
[mechanism.params]
fear_sensitivity = 0.4

[[mechanism]]
name  = "voice_decision_rule"
phase = "decision"
[mechanism.params]
beta_0        = -0.5
beta_psafety  = 1.2           # edmondson:1999
beta_fear     = 1.5           # kish-gephart:2009
beta_ivt      = 0.8           # detert:2011
beta_sup      = 1.0
beta_salience = 1.0
beta_climate  = 1.5           # noelle-neumann:1974

[[mechanism]]
name  = "silence_spiral"
phase = "interaction"
[mechanism.params]
epsilon = 0.25                # noelle-neumann:1974

[[mechanism]]
name  = "prefalse_cascade"
phase = "interaction"
[mechanism.params]
cascade_threshold = 0.05      # kuran:1995

[[mechanism]]
name  = "org_performance"
phase = "reward"
[mechanism.params]
cluster_tol = 0.05

[[mechanism]]
name  = "psafety_update"
phase = "post_step"
[mechanism.params]
psafety_learn = 0.1           # edmondson:1999

[[mechanism]]
name  = "climate_silence"
phase = "post_step"

[[mechanism]]
name  = "org_learning"
phase = "post_step"
[mechanism.params]
learning_rate  = 0.05
decay_rate     = 0.01
salience_floor = 0.3

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["silence_rate", "climate_of_silence", "voice_volume", "knowledge_stock", "org_performance", "opinion_clusters"]
```

Switch to the LLM variant by running the shipped LLM scenario instead, and
rebuilding the CLI with the LLM feature on:

```sh
cargo build --release -p socsim-cli --features pack-organizational-silence-llm
socsim run scenarios/org_silence_llm.toml
```

The LLM scenario uses the same world and mechanism stack, but replaces
`voice_decision_rule` with `voice_decision` and adds an `[llm]` block with
`temperature = 0` and a `cache_path` so the run is reproducible from a warm
cache. Cross-paradigm comparison is then a same-seed, two-scenario diff.

### Library

```rust
use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{SimClock, SimRng};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};
use socsim_packs::organizational_silence::{
    OrganizationalSilencePack, SilenceWorld,
};

let mut rng = SimRng::from_seed(42);
let mut world = SilenceWorld::new(5, 8, 3, 6, 0.1, 0.5, &mut rng);
world.clock = SimClock::new(60);

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let mut builder = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42);
for name in [
    "issue_salience", "retaliation_event",
    "fear_appraisal", "voice_decision_rule",
    "silence_spiral", "prefalse_cascade",
    "org_performance",
    "psafety_update", "climate_silence", "org_learning",
] {
    builder = builder.add_mechanism(reg.build(name, &Params::empty())?);
}
let mut sim = builder.build();
sim.run()?;
```

When building from source with the LLM variant, enable the feature
explicitly:

```sh
cargo run -p socsim-cli --features pack-organizational-silence-llm \
    -- run scenarios/org_silence_llm.toml
```

## 7. See also

- [Mechanism catalog](../mechanisms.md) — the wider catalog the spiral / cascade / fear-appraisal mechanisms sit beside.
- [hr-lifecycle pack](hr-lifecycle.md) — the other organisational pack (workforce evolution; ten calibrated mechanisms).
- [opinion-dynamics pack](opinion-dynamics.md) — the sibling "emergence on a network" pack.
- [T5 — A scenario pack](../tutorials/05-scenario-pack.md) — build a pack from scratch.
- [Use cases & recipes](../usecases.md) · [CLI reference](../cli.md) · [Architecture](../architecture.md)
