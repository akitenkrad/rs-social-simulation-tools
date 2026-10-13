**English** | [日本語](cli.ja.md)

# CLI Reference

The `socsim` binary exposes six scenario subcommands plus a `datasets` group for survey dataset acquisition. Run `socsim --help` or `socsim <COMMAND> --help` for flags.

The binary is **world-polymorphic**: every scenario names a *module pack* and is dispatched to that pack's world type. Four packs ship today (`hr-lifecycle`, `opinion-dynamics`, `organizational-silence`, `normal-form`); new packs register behind a `pack-*` Cargo feature.

```
socsim <COMMAND>

Commands:
  init       Generate a starter scenario TOML for a module pack
  run        Run a scenario (single seed or multi-seed)
  validate   Validate a scenario TOML, then apply the selected pack's own checks
  list       List available module packs or mechanisms
  sweep      Run a grid parameter sweep
  summarize  Re-aggregate existing JSONL run logs into a summary
  datasets   List / show / fetch survey datasets from the registry
  help       Print this message or the help of the given subcommand(s)
```

![CLI workflow](assets/cli-workflow.svg)

---

## init

Generate a starter scenario TOML for a module pack.

```
socsim init --module-pack <MODULE_PACK> --out <OUT>
```

| Flag | Description |
|---|---|
| `--module-pack` | Pack name, e.g. `hr-lifecycle` |
| `-o, --out` | Output file path |

**Example**

```sh
socsim init --module-pack hr-lifecycle --out scenarios/my_scenario.toml
```

Output:

```
Wrote starter scenario to 'scenarios/my_scenario.toml'
```

Every pack has a starter, e.g. `socsim init --module-pack opinion-dynamics --out scenarios/op.toml` or `socsim init --module-pack normal-form --out scenarios/ipd.toml` (a repeated prisoner's dilemma, identical to `scenarios/normal_form_ipd.toml`).

---

## run

Run a scenario TOML for one seed or a range of seeds.

```
socsim run [OPTIONS] <SCENARIO>
```

| Flag | Default | Description |
|---|---|---|
| `--seeds <A..B>` | scenario seed | Seed range (exclusive upper bound) |
| `--parallel` | false | Run seeds in parallel using Rayon |

**Single-seed run** (uses the seed from the TOML):

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
```

Output:

```
Running 'hr_lifecycle_baseline' (pack=hr-lifecycle, t_max=60, seeds=[42], parallel=false)

Seed 42 — 82 events recorded

t             avg_tenure   knowledge_stock   org_performance     turnover_rate
10                9.1000           53.9517           32.1462            0.0000
20               14.6000           62.4468           35.7133            0.0000
30               21.5500           72.5042           40.4270            0.0250
40               25.9000           78.4727           40.2186            0.0000
50               30.0750           85.3493           40.8007            0.0000
60               35.6250           92.3841           41.8100            0.0000
```

**Multi-seed run** prints a cross-seed summary table instead of per-step series:

```sh
socsim run scenarios/hr_lifecycle_baseline.toml --seeds 0..3
```

Output:

```
Running 'hr_lifecycle_baseline' (pack=hr-lifecycle, t_max=60, seeds=[0, 1, 2], parallel=false)

Cross-seed summary (3 seeds):

metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               35.8000      0.5319     35.3750     36.5500      3
knowledge_stock          92.6772      1.2340     91.1426     94.1641      3
org_performance          42.8467      1.4574     40.7856     43.8800      3
turnover_rate             0.0083      0.0118      0.0000      0.0250      3
```

Before printing `Running …`, `run` checks the scenario exactly as [`validate`](#validate) does; an invalid scenario stops with exit code 1 and nothing is run or written.

After each run, JSONL log files are written as specified by `output.log_path` in the scenario TOML (see [JSONL output format](#jsonl-output-format) below).

---

## validate

Check a scenario TOML without running it.

```
socsim validate <SCENARIO>
```

The check has two parts, run in this order:

1. **The generic scenario check**, the same for every pack: every
   `[[mechanism]]` name is registered in the named pack, the scheduler is
   known (`"sequential"` or `"random_activation"`), and `t_max > 0`.
2. **The pack's own check** (`CliPack::validate`). The `hr-lifecycle`,
   `opinion-dynamics` and `organizational-silence` packs add nothing here. The
   `normal-form` pack checks the whole scenario: every key and its type,
   unknown keys, the combination of game, rules, matching and population
   dynamic, and one dry build with the scenario's `seed` (see the
   [normal-form pack § What `validate` rejects](packs/normal-form.md#what-validate-rejects)).

If the first part fails, the second is not run.

**Example**

```sh
socsim validate scenarios/hr_lifecycle_baseline.toml
# OK — scenario 'scenarios/hr_lifecycle_baseline.toml' is valid.
```

A failure names the file and, below it, what to fix — for the `normal-form`
pack, the key path:

```
Error: validation failed for 'nowak_may.toml'

Caused by:
    config error: world.game.temptaton: unknown key (known keys: kind, punishment, reward, sucker, temptation)
```

```
Error: validation failed for 'ipd.toml'

Caused by:
    config error: mechanism 'game_decid' is not registered in the 'normal-form' pack; available: [fermi_update, game_decide, game_match, game_metrics, game_play, imitate_best, moran_update, replicator_update]
```

**`run` and `sweep` validate first.** Both commands perform the same two-part
check before they print `Running …` / `Sweeping …`, run any trial or write any
file; a failure exits with code 1 and leaves no JSONL log, CSV or scenario file
behind. For the `hr-lifecycle`, `opinion-dynamics` and `organizational-silence`
packs, valid scenarios run exactly as before; an invalid one is now rejected
instead of being run — for example a scenario with `t_max = 0`, which used to
run zero steps:

```
Error: validation failed for 'hr0.toml'

Caused by:
    config error: simulation.t_max must be > 0
```

---

## list

List available module packs or the mechanisms inside each pack. For what each pack contains, see the [module pack catalog](packs.md).

```
socsim list <WHAT>
```

`<WHAT>` is one of `packs` or `mechanisms`.

**Examples**

```sh
socsim list packs
```

```
Available module packs:
  hr-lifecycle
  opinion-dynamics
  organizational-silence
  normal-form
```

```sh
socsim list mechanisms
```

```
Mechanisms by pack:
  [hr-lifecycle]
    fit
    hiring
    knowledge_loss
    learning_curve
    ocb
    org_performance
    peer_effect
    socialization
    toxic_spread
    turnover
  [opinion-dynamics]
    convergence
    deffuant
    hegselmann_krause
    lorenz
    opinion_metrics
    social_judgement
  [organizational-silence]
    climate_silence
    fear_appraisal
    issue_salience
    org_learning
    org_performance
    prefalse_cascade
    psafety_update
    retaliation_event
    silence_spiral
    voice_decision_rule
  [normal-form]
    fermi_update
    game_decide
    game_match
    game_metrics
    game_play
    imitate_best
    moran_update
    replicator_update
```

When the CLI is built with `--features pack-organizational-silence-llm`, the
`[organizational-silence]` block also lists a `voice_decision` entry (the LLM
variant of `voice_decision_rule`).

---

## sweep

Run a grid parameter sweep over the Cartesian product of one or more parameter axes.

```
socsim sweep [OPTIONS] <SCENARIO>
```

| Flag | Default | Description |
|---|---|---|
| `--param <MECH.PARAM\|world.PATH=V1,V2,...>` | — | Sweep axis (repeatable for multi-dimensional sweep); with the `normal-form` pack also `world.<PATH>=V1,V2,...` |
| `--seeds <A..B>` | `0..5` | Seed range for each combo |
| `-o, --out <DIR>` | `runs/sweep` | Output directory for per-combo CSV files |
| `--parallel` | false | Parallel seeds within each combo |

**Example** — probe how `toxic_spread.p_spread` affects outcomes:

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "toxic_spread.p_spread=0.2,0.46,0.7" \
    --seeds 0..3
```

Output (abridged):

```
Sweeping 'hr_lifecycle_baseline' over 1 axes × 3 seeds
  toxic_spread.p_spread = [0.2, 0.46, 0.7]
  combo 0: toxic_spread.p_spread=0.2000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               35.3250      5.0624     29.1000     41.5000      3
...
  combo 2: toxic_spread.p_spread=0.7000
...
Wrote 3 CSV files to 'runs/sweep'
```

Each CSV file is named `combo_<N>_<param>=<value>.csv` and contains columns `key,mean,std,min,max,n`.

**Multi-dimensional sweep** — add `--param` once per axis:

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "peer_effect.alpha_peer=0.1,0.17,0.3" \
    --param "turnover.quit_cascade_bump=0.1,0.3,0.5" \
    --seeds 0..10 --parallel
```

`sweep` checks the base scenario first, as [`validate`](#validate) does, so
the base scenario itself must be valid. With the `hr-lifecycle`,
`opinion-dynamics` and `organizational-silence` packs, the combinations are
not checked: an axis whose key does not exist is silently ignored and every
combination runs the base parameters —

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml --param "toxic_spread.p_sprea=0.2,0.5" --seeds 0..1
# exit 0; writes combo_0000_toxic_spread.p_sprea=0.2000.csv and combo_0001_…
```

— so check the spelling against the mechanism page.

### Checked sweeps (`normal-form`)

The `normal-form` pack runs a **checked sweep**: unknown paths are errors, every
combination is validated before any trial runs, and the effective scenario of
each combination is written next to its CSV.

```sh
socsim sweep scenarios/normal_form_nowak_may.toml \
    --param "world.game.temptation=1.6,1.9" --seeds 0..2 -o runs/nm_sweep
```

```
Sweeping 'nowak_may_b185' over 1 axes × 2 seeds
  world.game.temptation = [1.6, 1.9]
  combo 0: world.game.temptation=1.6000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
game.action_entropy       0.6787      0.0054      0.6733      0.6841      2
game.coop_rate            0.5832      0.0162      0.5670      0.5994      2
game.mean_payoff          5.9735      0.1399      5.8337      6.1134      2
...
  combo 1: world.game.temptation=1.9000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
game.action_entropy       0.6114      0.0139      0.5975      0.6253      2
game.coop_rate            0.3014      0.0165      0.2849      0.3179      2
game.mean_payoff          3.4352      0.1473      3.2879      3.5825      2
...

Wrote 2 CSV files to 'runs/nm_sweep'
```

```
runs/nm_sweep/
  combo_0000_world.game.temptation=1.6000.csv
  combo_0000_world.game.temptation=1.6000.scenario.toml
  combo_0001_world.game.temptation=1.9000.csv
  combo_0001_world.game.temptation=1.9000.scenario.toml
```

**Axes.** An axis is either a `world.` path — an existing key under `[world]`,
through nested tables (`world.game.temptation`,
`world.population.initial_shares.C`, `world.history_len`) — or
`<mechanism>.<param>` (`game_decide.logit_lambda`, `fermi_update.k`, …),
which patches every `[[mechanism]]` entry with that name. Only integers and
floats can be swept. The pack page lists the
[axes worth sweeping](packs/normal-form.md#sweeps).

**Effective scenarios.** Each `combo_<N>_<label>.scenario.toml` is the base
scenario with that combination's values patched in, written with
`toml::to_string` (comments are dropped, keys sorted). It is a complete
scenario: `socsim run` on it reruns that combination. Labels show four
decimals, so read the exact value from this file (`stop_tol=1e-6` and `1e-9`
both label as `0.0000`; the combination index keeps the names apart). Only the
`normal-form` pack writes these files.

**Errors.** A wrong axis stops the sweep before any trial runs and before the
output directory is created (exit code 1):

```
Error: sweep failed

Caused by:
    config error: sweep axis 'world.game.temptaton': no key 'temptaton' in [world.game]
```

| Cause | Message (after `config error: `) |
|---|---|
| typo in a `world.` path | `sweep axis 'world.game.temptaton': no key 'temptaton' in [world.game]` |
| unknown mechanism name | `sweep axis 'game_decid.logit_lambda': mechanism 'game_decid' is not in the scenario` |
| the target is not a number | `sweep axis 'world.game.kind': existing value is a string; only integer and float values can be swept` |
| a fractional value for an integer key | `sweep axis 'world.game.temptation': existing value is an integer; 1.6 is not an integral value representable as i64` |
| a share label that the base scenario leaves out | `sweep axis 'world.population.initial_shares.D': no key 'D' in [world.population.initial_shares]` |
| a combination that is not a valid scenario | `sweep combination [world.game.temptation=0.9]: config error: world.game: catalog condition failed: T > R > P >= S` |
| a misspelt mechanism parameter | `sweep combination [game_decide.logit_lamda=1]: config error: mechanism[game_decide].logit_lamda: unknown key (known keys: …)` |

**Three things to know.**

1. **An integer stays an integer.** A key written as `temptation = 2` can only
   be swept over integral values; write a value you intend to sweep as a
   float (`temptation = 2.0`).
2. **`world.population.initial_shares.<label>` can sweep only labels written
   in the base scenario.** A label left out to receive the rest has no key to
   patch.
3. **A missing mechanism parameter is added as a float, not rejected.** Sweeping
   a parameter no assigned rule reads — `game_decide.logit_lambda` in
   `scenarios/normal_form_ipd.toml`, where no agent follows `logit` — passes
   the check and gives identical results for every value.

---

## summarize

Re-aggregate one or more existing JSONL run logs into summary statistics without re-running the simulation.

```
socsim summarize [OPTIONS] <PATH>
```

| Flag | Default | Description |
|---|---|---|
| `--format <csv\|json>` | `csv` | Output format |

`<PATH>` may be a single `.jsonl` file or a directory; directories are scanned non-recursively for `*.jsonl` files.

**Examples**

```sh
socsim summarize runs/hr_lifecycle_baseline_42.jsonl
```

```
key,mean,std,min,max,n
avg_tenure,35.625000,0.000000,35.625000,35.625000,1
knowledge_stock,92.384145,0.000000,92.384145,92.384145,1
org_performance,41.810021,0.000000,41.810021,41.810021,1
turnover_rate,0.000000,0.000000,0.000000,0.000000,1
```

```sh
socsim summarize runs/ --format json
```

---

## datasets

Inspect and acquire the survey datasets backing survey replications, from the machine-readable registry in `socsim-datasets`. The subcommand group is **separate** from the world-polymorphic scenario spine and needs no module pack.

```
socsim datasets <SUBCOMMAND>
```

| Subcommand | Description |
|---|---|
| `datasets list` | List every dataset in the registry: key, name, file count, and acquisition kind (`auto` / `manual`) |
| `datasets show <KEY>` | Show one dataset's overview (name, key, DOI, source URL, citation, license) and, per file, its acquisition method — for **manual** files the instructions URL + steps; for **auto-downloadable** files the resolved download URL, the exact `socsim datasets fetch …` command, plus any `sha256` / `expect_rows` |
| `datasets fetch <KEY> [--dest <DIR>] [--force]` | Actually download the dataset's files into `--dest` (default a gitignored `data/` dir), atomic-writing and verifying `sha256` + row counts, skipping cache hits |

`fetch` requires building the CLI with the opt-in **`--features datasets-acquire`** flag. The default CLI build keeps `list` / `show` working, but `fetch` simply tells you to rebuild with that feature. Data is **never vendored** into the repo; the license-gated raw ANES Time Series microdata is declared `Source::Manual` and reports manual instructions instead of being auto-downloaded, whereas the CC0 1.0 (public domain) CES 2022 Common Content is a `Source::Dataverse` and is auto-downloadable (`fetch ces-2022` pulls it directly from the Harvard Dataverse, no account / terms).

**Examples**

```sh
socsim datasets list
socsim datasets show anes-2020
cargo run --features datasets-acquire -- datasets fetch anes-2020 --dest data/
```

---

## JSONL output format

Each `socsim run` writes one JSONL file per seed. The path is controlled by `output.log_path` in the scenario TOML, which supports two substitution tokens:

| Token | Replaced with |
|---|---|
| `{name}` | `simulation.name` |
| `{seed}` | The integer seed for that trial |

Example template: `"runs/{name}_{seed}.jsonl"` → `runs/hr_lifecycle_baseline_42.jsonl`

Each line of the JSONL file is a self-contained JSON object. Two record types are emitted:

**Metric record**

```json
{"type":"metric","t":1,"key":"turnover_rate","value":0.0}
```

**Event record**

```json
{"type":"event","t":3,"kind":"turnover","payload":{"agent":7,"team":2}}
```

Fields are consistent across all records of each type; `payload` is mechanism-specific.
