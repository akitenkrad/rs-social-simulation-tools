**English** | [日本語](06-normal-form-game.ja.md)

# T6 — A normal-form game

**What you'll build:** an iterated prisoner's dilemma, first from a generated scenario and then from Rust, followed by a plot of action shares across seeds.
**Estimated time:** 30 minutes.

## Prerequisites

- [T0 — Getting started](00-getting-started.md) (`socsim init` and `socsim run`).
- A working Rust toolchain. Basic Rust is enough for the library-mode part.
- [`uv`](https://docs.astral.sh/uv/), only for the plotting step.

The CLI commands below write into a temporary directory. Run them from a shell where the `socsim` binary is on `PATH`:

```sh
work=$(mktemp -d)
cd "$work"
```

## Steps

### 1. Scaffold and run an iterated prisoner's dilemma

Ask the bundled `normal-form` pack for its starter scenario:

```sh
socsim init --module-pack normal-form --out ipd.toml
```

```
Wrote starter scenario to 'ipd.toml'
```

The generated TOML has five pieces: `[simulation]` sets the horizon and seed; `[world.game]` contains the payoff matrix; `[world.population]` assigns Tit-for-Tat and fictitious play to the two agents; `game_match`, `game_decide`, and `game_play` run the game; and `game_metrics` observes it.

Run it:

```sh
socsim run ipd.toml
```

```
Running 'ipd_tft_vs_fp' (pack=normal-form, t_max=200, seeds=[42], parallel=false)

Seed 42 — 0 events recorded

t       game.action_entropy    game.coop_rate  game.eq_distance  game.mean_payoff    game.payoff_sd  game.played_share.a0  game.played_share.a1       game.regret
10                0.0000            0.0000            0.0000            1.0000            0.0000            0.0000            1.0000            0.0000
...
200               0.0000            0.0000            0.0000            1.0000            0.0000            0.0000            1.0000            0.0000
```

Both agents cooperate at step 1. Fictitious play defects at step 2, Tit-for-Tat answers at step 3, and thereafter every played action is `D`: `game.played_share.a1 = 1`. Mutual defection gives each player `1`, so `game.mean_payoff = 1`; it is also the stage game's Nash equilibrium, so regret and equilibrium distance are both zero.

### 2. Change one rule and compare the result

Copy the scenario and edit two values:

```sh
cp ipd.toml ipd-all-c.toml
```

In `ipd-all-c.toml`, change the simulation name to `ipd_tft_vs_all_c`, and change the second population group from

```toml
rule  = "fictitious_play"
```

to

```toml
rule  = "all_c"
```

Then run the changed scenario:

```sh
socsim run ipd-all-c.toml
```

```
Running 'ipd_tft_vs_all_c' (pack=normal-form, t_max=200, seeds=[42], parallel=false)

Seed 42 — 0 events recorded

t       game.action_entropy    game.coop_rate  game.eq_distance  game.mean_payoff    game.payoff_sd  game.played_share.a0  game.played_share.a1       game.regret
10                0.0000            1.0000            4.0000            3.0000            0.0000            1.0000            0.0000            2.0000
...
200               0.0000            1.0000            4.0000            3.0000            0.0000            1.0000            0.0000            2.0000
```

Now both agents always cooperate, so the mean payoff rises from `1` to `3`. The positive regret (`2`) and equilibrium distance (`4`) are a useful warning: this cooperative path pays both agents more than mutual defection, but it is not a Nash equilibrium of the one-shot stage game.

### 3. Assemble the same game in Rust

Scenario mode covers built-in games and rules. Library mode lets you provide custom games, rules, matchings, and experiment state. Start a binary crate and add `socsim-games` with features `core,engine`:

```toml
[dependencies]
socsim-games = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["core", "engine"] }
```

Then put this in `src/main.rs` (the same minimal construction as the [`socsim-games` guide](../games.md#minimal-example-iterated-prisoners-dilemma)):

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};

const ROUNDS: usize = 100;

fn main() {
    // Row and column payoffs; action 0 = C, action 1 = D.
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .expect("the prisoner's dilemma matrix is valid");

    let mut rules = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 1;
    let world = build_world(
        game,
        2,
        (),
        InitialActions::Same(0),
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".into()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".into()), count: 1 },
        ]),
        ROUNDS,
        ROUNDS as u64,
        seed,
    )
    .expect("the world configuration is valid");

    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .expect("the simulation configuration is valid")
    .build();

    let mut scores = [0.0, 0.0];
    for _ in 0..ROUNDS {
        simulation.step().unwrap();
        for (score, payoff) in scores.iter_mut().zip(simulation.world().payoffs()) {
            *score += payoff;
        }
    }
    println!("TFT score: {}, Fixed(D) score: {}", scores[0], scores[1]);
}
```

```sh
cargo run -q
```

```
TFT score: 99, Fixed(D) score: 104
```

`GameWorld::payoffs()` contains only the current step, so the loop accumulates the scores. `assemble` installs the matching, decision, learning, and payoff mechanisms; add `GameMetrics` separately when a library-mode run needs recorded columns.

### 4. Read the recorded output

The scenario writes `runs/ipd_tft_vs_fp_42.jsonl` beneath the temporary directory. The CLI table samples the display every 10 steps, while the JSONL has one metric record per key and step:

```json
{"key":"game.played_share.a1","t":1,"type":"metric","value":0.0}
{"key":"game.played_share.a1","t":2,"type":"metric","value":0.5}
{"key":"game.played_share.a1","t":3,"type":"metric","value":1.0}
```

For this scenario, the columns mean:

- `game.played_share.a0` / `.a1`: shares of played seats using action 0 (`C`) / action 1 (`D`);
- `game.coop_rate`: the share using the action labelled by `cooperate_label`;
- `game.mean_payoff` / `game.payoff_sd`: mean and population standard deviation over agents who played;
- `game.regret`: the largest gain available from a unilateral deviation;
- `game.eq_distance`: L1 distance from the closest solved Nash equilibrium;
- `game.action_entropy`: Shannon entropy of the played-action shares.

Undefined metrics are omitted rather than written as NaN. See [`game_metrics`](../mechanisms/game-metrics.md) for the exact definitions and optional event records.

### 5. Plot action shares with `socsim-tools`

The repository includes a five-seed IPD fixture with a game specification and solved equilibria. From the repository root, render its mean action shares and 95% t interval into another temporary directory:

```sh
figures=$(mktemp -d)
cd tools/socsim_tools
uv run --extra games socsim-tools plot-shares \
  --source ../../crates/socsim-games/tests/fixtures/viz/ipd/seed_0.jsonl \
  --source ../../crates/socsim-games/tests/fixtures/viz/ipd/seed_1.jsonl \
  --source ../../crates/socsim-games/tests/fixtures/viz/ipd/seed_2.jsonl \
  --source ../../crates/socsim-games/tests/fixtures/viz/ipd/seed_3.jsonl \
  --source ../../crates/socsim-games/tests/fixtures/viz/ipd/seed_4.jsonl \
  --game-spec ../../crates/socsim-games/tests/fixtures/viz/ipd/game_spec.json \
  --solutions ../../crates/socsim-games/tests/fixtures/viz/ipd/solutions.json \
  --output-dir "$figures" --format png
```

This writes `shares_played_share.png`. Use `plot-actions` when the run also contains `game.roster` and `game.step_actions` events. The [recorded-run visualization guide](../games.md#recorded-runs-share-series-and-action-rasters) explains both commands and the event-volume trade-off.

## Run it

The shortest path through the tutorial is:

```sh
work=$(mktemp -d) && cd "$work"
socsim init --module-pack normal-form --out ipd.toml
socsim run ipd.toml
# In a temporary Rust crate containing the source above:
cargo run -q
```

## What you learned

- The `normal-form` pack turns a scenario into a stage game, population, matching, decision rules, payoff evaluation, and per-step metrics.
- Changing one population rule can move the trajectory from mutual defection to persistent cooperation without changing the payoff matrix.
- In Rust, `build_world` owns game and rule state; `assemble` adds the executable game mechanisms.
- `game_metrics` records shares, payoffs, regret, equilibrium distance, and entropy as JSONL metric rows.
- `socsim-tools[games]` aggregates multiple seeds and draws share series; action rasters need action events.

## Next

- [Normal-form games](../games.md#scenario-files) — the full scenario schema, library builders, population dynamics, solvers, and visualization contract.
- [`normal-form` pack](../packs/normal-form.md) — CLI-specific validation, starter scenarios, and checked sweeps.
- Game mechanisms: [`game_match`](../mechanisms/game-match.md), [`game_decide`](../mechanisms/game-decide.md), [`game_play`](../mechanisms/game-play.md), and [`game_metrics`](../mechanisms/game-metrics.md).
- [Use cases: normal-form games](../usecases.md#8-normal-form-games-from-a-scenario) — spatial games, parameter sweeps, and continuous-action oligopoly.
