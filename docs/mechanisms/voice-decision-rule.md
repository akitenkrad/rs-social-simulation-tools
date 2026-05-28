**English** | [日本語](voice-decision-rule.ja.md)

# Voice decision (rule and LLM) (`voice_decision_rule`, `voice_decision`)

> Each agent decides Voice vs Silence via a calibrated logistic that blends
> psychological safety, supervisor openness, issue salience, fear, implicit
> voice theory, and the neighbour silence ratio. On Silence the rule also
> assigns one of three motives — Acquiescent, Defensive, or Prosocial —
> following Van Dyne, Ang & Botero (2003). A feature-gated LLM sibling
> (`voice_decision`) replaces the logistic with a structured-JSON prompt and
> falls back to the rule defaults on any parse failure.
> **Phase:** Decision. **Sources:** Noelle-Neumann (1974); Van Dyne et al. (2003); Detert & Edmondson (2011); Kuran (1995); Edmondson (1999). **Kind:** mixed (empirical coefficients + theoretical structure).

[← Back to the mechanism catalog](../mechanisms.md)

## 1. Overview

`voice_decision_rule` is the centrepiece of the organisational-silence pack:
it consumes every per-agent and per-team scalar that the model treats as a
predictor of voicing, runs them through a single logistic, draws one
Bernoulli per agent, and writes the resulting `Expression::Voice` or
`Expression::Silence` to each employee. On a Silence draw it additionally
classifies the agent into one of three silence motives — the typology
introduced by Van Dyne, Ang & Botero (2003) — so the downstream `motive_mix`
event in `org_performance` can track the composition of silence over time.

The rule's structure mirrors the design's §4.3 decision equation, but the
implementation diverges from the design in two concrete ways that this page
documents authoritatively:

1. The **prosocial motive** is detected by a state predicate
   (`supervisor_openness > 0` AND `private_concern < 0`), not by a strength
   score. Prosocial silence is "I'm protecting others by withholding a
   critical view despite an open supervisor" — a characterised situation
   rather than the maximum of a continuum.
2. The `BETA_CLIMATE` term reads $\rho_i$ from the **per-agent snapshot**
   `Employee.neighbor_silence_ratio` left at the end of the previous step's
   Interaction phase by `silence_spiral`. This is the carrier that turns the
   intra-tick mechanism stack into a coherent multi-step spiral; the field
   is `pub(crate)` so external code cannot accidentally desynchronise it.

A second mechanism registered under the canonical name `voice_decision`
provides a drop-in LLM alternative; it shares the same write contract but
replaces the logistic with a structured prompt — see §2.1.

## 2. Theory & source

The rule blends six predictors into a single logit, draws Bernoulli$(p)$ with
$p = \operatorname{logistic}(\text{logit})$, and on Silence runs a tiny
classifier:

$$\text{logit}_i = \beta_0 + \beta_\psi \cdot \psi_i + \beta_u \cdot u_{k(i)} + \beta_\sigma \cdot \sigma - \beta_f \cdot f_i - \beta_\iota \cdot \iota_i - \beta_C \cdot \rho_i$$

$$p_i = \operatorname{logistic}(\text{logit}_i) = \frac{1}{1 + e^{-\text{logit}_i}}, \qquad X_i \sim \operatorname{Bernoulli}(p_i)$$

$$\text{Expression}_i = \begin{cases} \text{Voice} & X_i = 1 \\ \text{Silence} & X_i = 0 \end{cases}$$

The predictors and their citations:

- $\psi_i$ (`Employee.psych_safety`, Edmondson 1999) — perceived psychological
  safety. Positive sign: more safety raises the probability of voicing.
- $u_{k(i)}$ (`Team.supervisor_openness`, Detert & Burris 2007 / Morrison
  2014) — the supervisor's openness signal. Positive sign: an open supervisor
  raises voice probability.
- $\sigma$ (`SilenceWorld.issue_salience`, Morrison 2014) — current issue
  salience. Positive sign: a more salient issue is harder to keep quiet about.
- $f_i$ (`Employee.fear`, Kish-Gephart et al. 2009) — fear-of-speaking.
  **Subtracted**: more fear lowers voice probability.
- $\iota_i$ (`Employee.ivt_strength`, Detert & Edmondson 2011) — implicit voice
  theory strength. **Subtracted**: an agent who has internalised "speaking up
  is risky" silences themselves independently of situational fear.
- $\rho_i$ (`Employee.neighbor_silence_ratio`, Noelle-Neumann 1974) — snapshot
  of the agent's local silence ratio from the *previous* step's Interaction
  phase. **Subtracted**: a silent local majority erodes the agent's
  willingness to voice — the spiral of silence.

The default coefficients live in
[`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs)
and are summarised in the pack page's
[Calibration anchors](../packs/organizational-silence.md#5-calibration-anchors)
table.

### Motive classification on Silence

When $X_i = 0$ the mechanism assigns `silence_motive` by ranking the three
suppressors in a fixed order. The classifier is a pure function
(`classify_motive`) for unit testability:

1. **Prosocial silence first.** If `supervisor_openness > 0` *and*
   `private_concern < 0`, motive = `Prosocial`. This is the "protective
   withholding" case from Van Dyne et al. (2003): the agent has a critical
   view to share *and* the supervisor is open to hearing it, yet they stay
   silent — interpreted as withholding to protect others rather than because
   of personal threat.
2. **Otherwise, fear vs IVT, with ties going to fear.** If $f_i \ge \iota_i$,
   motive = `Defensive` (fear-driven). Otherwise motive = `Acquiescent`
   ("nothing will change anyway" — disengagement-driven).

The order matters: a high-fear, high-IVT agent whose supervisor is open and
who holds a critical concern is classified `Prosocial`, not `Defensive` —
the situational predicate trumps the strength comparison.

### 2.1 LLM variant (`voice_decision`)

The pack ships a feature-gated LLM variant registered under the canonical
name `voice_decision`. Both mechanisms can coexist in a single registry; the
scenario TOML picks between them by name. Source:
[`crates/socsim-packs/src/organizational_silence/mechanisms_llm.rs`](../../crates/socsim-packs/src/organizational_silence/mechanisms_llm.rs).

The LLM variant is gated behind:

- the `organizational-silence-llm` feature on the `socsim-packs` crate, and
- the `pack-organizational-silence-llm` feature on the `socsim-cli` crate.

When either feature is enabled, the pack's `register` method additionally
inserts a `voice_decision` mechanism alongside `voice_decision_rule`.

For each agent in `ctx.agent_order` the mechanism:

1. **Builds the prompt** from the agent's level, tenure, fear,
   psych_safety, ivt_strength, neighbour silence ratio, supervisor openness,
   `retaliated_this_step` flag, and the current $\sigma$. The prompt asks the
   model to return one JSON object on a single line.
2. **Calls `LiveClient::complete`** with a deterministic `LlmConfig`
   (temperature 0, scenario seed) — the live client is assembled from the
   pack's `LlmSettings` and routes to a local Ollama → OpenAI fallback,
   serving cache hits from a JSON-file-backed `PromptCache`.
3. **Parses the response** with `serde_json` into a
   `{decision, motive, rationale}` shape. The parser tolerates upper/lower
   case and falls back to `(Silence, Defensive)` on any parse failure, so a
   single misbehaving response cannot abort the run.

The response shape:

```json
{"decision": "VOICE"|"SILENCE", "motive": "acquiescent"|"defensive"|"prosocial"|null, "rationale": "..."}
```

A warm `PromptCache` makes the LLM variant a deterministic oracle: replays
hit the cache and return without a network round-trip. Tests can inject a
`socsim_llm::mock::ScriptedClient` through the `from_client` constructor.
The same write contract as the rule variant applies: on Voice the motive is
cleared (`None`); on Silence the parsed motive is written.

## 3. Data flow

The rule reads $\psi$, $f$, $\iota$, `team`, and the previous-step
$\rho$ snapshot from each employee, plus `Team.supervisor_openness` and
`SilenceWorld.issue_salience`. It snapshots team openness into a `Vec<f64>`
before mutating employees (so the borrow checker permits the mutation), then
iterates `ctx.agent_order` to compute the logit, draw a Bernoulli, write
the new `Expression`, and — on Silence — write `silence_motive`. The LLM
variant has the same I/O shape but calls `LiveClient::complete` per agent
instead of evaluating the logit.

## 4. Position in the 6-phase loop

Runs in **Decision**, the third phase. Within Decision the bundled scenario
declares `fear_appraisal` first and `voice_decision_rule` (or
`voice_decision`) second, so the agent's fear has been updated from this
step's retaliation buffer before the logit reads $f_i$. The supervisor
openness term and $\sigma$ term read from world fields, so any other Decision
mechanism that wrote to those fields would also need to run earlier — but in
the bundled scenario neither is touched after Environment.

The mechanism is the principal hand-off between the silence appraisal stack
(salience, retaliation, fear, IVT, $\rho$) and the within-step Interaction
phase (`silence_spiral`, `prefalse_cascade`), both of which read the
`Expression` and `silence_motive` this mechanism writes.

## 5. State read/write contract

| Field | Read | Write | Notes |
|---|:--:|:--:|---|
| `ctx.agent_order` | ✓ | | Deterministic iteration order; one RNG draw per id. |
| `ctx.rng` | ✓ | | One `gen::<f64>()` Bernoulli draw per agent. |
| `SilenceWorld.issue_salience` | ✓ | | Read as $\sigma$ in the logit. |
| `Team.supervisor_openness` | ✓ | | Snapshotted into `Vec<f64>` before mutating employees. |
| `Employee.psych_safety` | ✓ | | $\psi_i$ in the logit. |
| `Employee.fear` | ✓ | | $f_i$ in the logit (subtracted). |
| `Employee.ivt_strength` | ✓ | | $\iota_i$ in the logit (subtracted). |
| `Employee.team` | ✓ | | Index into the openness snapshot. |
| `Employee.neighbor_silence_ratio` | ✓ | | $\rho_i$ in the logit (subtracted); set by previous step's `silence_spiral`. |
| `Employee.private_concern` | ✓ | | Used by `classify_motive` for the Prosocial branch. |
| `Employee.expression` | | ✓ | Set to `Voice` or `Silence`. |
| `Employee.silence_motive` | | ✓ | Cleared on Voice; set on Silence by `classify_motive`. |

## 6. Dependencies & ordering constraints

- **Upstream (same step):**
  - `fear_appraisal` (Decision) must precede this mechanism so $f_i$ is fresh
    from this step's retaliation buffer.
  - `issue_salience` (Environment) writes $\sigma$ before Decision begins.
- **Upstream (previous step):**
  - `silence_spiral` (Interaction) wrote `Employee.neighbor_silence_ratio` at
    the end of the previous step — the carrier of the spiral effect into this
    step's logit.
- **Downstream (same step):**
  - `silence_spiral` (Interaction) reads each agent's freshly-written
    `Expression` to compute the next step's $\rho$ snapshot.
  - `prefalse_cascade` (Interaction) only flips agents whose `Expression ==
    Silence` and whose `private_concern < 0`, both of which this mechanism
    writes.
  - `org_performance` (Reward) tallies `silence_motive` for the `motive_mix`
    event.

## 7. Parameters

| Param key | Default | Kind | Source |
|---|---|---|---|
| `beta_0` | `-0.5` | calibration scale (tunable) | calibration — mildly negative intercept |
| `beta_psafety` | `1.2` | empirical | Edmondson (1999) |
| `beta_fear` | `1.5` | empirical | Kish-Gephart et al. (2009) |
| `beta_ivt` | `0.8` | empirical | Detert & Edmondson (2011) |
| `beta_sup` | `1.0` | empirical | Detert & Burris (2007) / Morrison (2014) |
| `beta_salience` | `1.0` | empirical | Morrison (2014) |
| `beta_climate` | `1.5` | empirical | Noelle-Neumann (1974) / Sohn (2022) |

Defaults live in
[`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs)
as `BETA_0`, `BETA_PSAFETY`, `BETA_FEAR`, `BETA_IVT`, `BETA_SUP`,
`BETA_SALIENCE`, `BETA_CLIMATE`.

The LLM variant `voice_decision` recognises a different parameter set:

| Param key | Default | Kind | Source |
|---|---|---|---|
| `cache_path` | `"runs/silence_cache.json"` | path | LLM prompt cache file |
| `seed` | `42` | u64 | passed to `LlmConfig` |
| `temperature` | `0.0` | f64 | passed to `LlmConfig` |

## 8. How to apply

### Scenario TOML — rule

```toml
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
```

### Scenario TOML — LLM variant

Build the CLI with the LLM feature, then swap the mechanism name to
`voice_decision`:

```sh
cargo build --release -p socsim-cli --features pack-organizational-silence-llm
```

```toml
[llm]
decision_mode = "llm"
temperature   = 0.0
seed          = 42
cache_path    = "runs/silence_cache.json"

[[mechanism]]
name  = "voice_decision"
phase = "decision"
[mechanism.params]
cache_path = "runs/silence_cache.json"
seed       = 42
```

### Library mode

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("voice_decision_rule", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. Determinism & RNG

The rule iterates `ctx.agent_order` (the scheduler-provided ordering, which
is itself seeded) and draws exactly one Bernoulli per agent via
`ctx.rng.gen::<f64>()`. Because the iteration order is fixed and the
read-snapshot of team openness happens before any mutation, two runs over
the same world state and seed produce bit-identical
`Expression`/`silence_motive` vectors.

The LLM variant is deterministic under (a) `temperature = 0` and a warm
`PromptCache` (every call hits the cache and returns the same response), or
(b) a `ScriptedClient` in tests. Live runs without a warm cache depend on
the backend respecting `temperature = 0`; the cache hit rate is reported via
`MetadataCollector::cache_hit_rate()` so a researcher can verify the run was
served from cache before treating its output as reproducible.

## 10. Expected behaviour

In the baseline scenario (seed 0, 60 steps), the rule produces:

- Silence rates around 0.20–0.55 across the run; voice volume the
  complement.
- A pre-shock climate of silence in the 5–10 % range that drops further
  after the $\sigma$ shock at $t = 24$ tilts marginal agents toward Voice.
- A motive mix dominated by `Acquiescent` (the most common silence is "it
  won't change anything" when neither fear nor IVT is high) with a smaller
  Defensive cohort under retaliation and the occasional Prosocial silence
  when an open supervisor coexists with a critical private concern.
- Cascade events firing on most steps when many marginal silent dissenters
  are present (the rule produces enough silence-with-critical-concern
  agents to make the cascade's > 5 % flip-mass threshold easy to hit).

Switching to `voice_decision` (LLM) with a frontier model tends to produce
qualitatively similar trajectories but with motive shares biased toward
whichever motive the model verbalises most readily — usually `Defensive`
under high `fear` and `Acquiescent` under high `ivt_strength`. The rule
remains the calibrated baseline; the LLM variant is the comparative
artefact.

## 11. References

- Detert, J. R., & Burris, E. R. (2007). Leadership behavior and employee
  voice: Is the door really open? *Academy of Management Journal*, 50(4),
  869–884.
- Detert, J. R., & Edmondson, A. C. (2011). Implicit voice theories:
  Taken-for-granted rules of self-censorship at work. *Academy of Management
  Journal*, 54(3), 461–488.
- Edmondson, A. C. (1999). Psychological safety and learning behavior in
  work teams. *Administrative Science Quarterly*, 44(2), 350–383.
- Kish-Gephart, J. J., Detert, J. R., Treviño, L. K., & Edmondson, A. C.
  (2009). Silenced by fear: The nature, sources, and consequences of fear at
  work. *Research in Organizational Behavior*, 29, 163–193.
- Kuran, T. (1995). *Private Truths, Public Lies: The Social Consequences
  of Preference Falsification*. Harvard University Press.
- Morrison, E. W. (2014). Employee voice and silence. *Annual Review of
  Organizational Psychology and Organizational Behavior*, 1(1), 173–197.
- Noelle-Neumann, E. (1974). The spiral of silence: A theory of public
  opinion. *Journal of Communication*, 24(2), 43–51.
- Van Dyne, L., Ang, S., & Botero, I. C. (2003). Conceptualizing employee
  silence and employee voice as multidimensional constructs. *Journal of
  Management Studies*, 40(6), 1359–1392.
