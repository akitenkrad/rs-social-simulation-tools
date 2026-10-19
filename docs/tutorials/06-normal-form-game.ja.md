[English](06-normal-form-game.md) | **日本語**

# T6 — 戦略形ゲーム

**作るもの：** 繰り返し囚人のジレンマを，生成したシナリオとRustのそれぞれから組み立て，最後に複数シードの行動割合を図にします．
**所要時間：** 30分．

## 前提

- [T0 — はじめに](00-getting-started.ja.md)（`socsim init` と `socsim run`）．
- 動作するRustツールチェーン．ライブラリモードの部分には基本的なRustで十分です．
- 描画ステップだけ [`uv`](https://docs.astral.sh/uv/) が必要です．

以下のCLIコマンドは一時ディレクトリへ書き出します．`socsim` バイナリが `PATH` 上にあるシェルで実行してください：

```sh
work=$(mktemp -d)
cd "$work"
```

## ステップ

### 1. 繰り返し囚人のジレンマを生成して実行する

同梱の `normal-form` パックからスターターシナリオを生成します：

```sh
socsim init --module-pack normal-form --out ipd.toml
```

```
Wrote starter scenario to 'ipd.toml'
```

生成したTOMLには5つの部品があります．`[simulation]` は期間とシード，`[world.game]` は利得行列，`[world.population]` は2エージェントへのしっぺ返しと仮想プレイの割り当てです．`game_match`，`game_decide`，`game_play` がゲームを動かし，`game_metrics` が観測します．

実行します：

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

ステップ1では両者が協力します．仮想プレイはステップ2で裏切り，しっぺ返しはステップ3で応じます．その後に打たれる行動はすべて `D` なので，`game.played_share.a1 = 1` です．相互裏切りでは各プレイヤーの利得が `1` となり，`game.mean_payoff = 1` です．これは段階ゲームのNash均衡でもあるため，後悔と均衡までの距離はいずれもゼロです．

### 2. 規則を1つ変えて結果を比べる

シナリオを複製し，2つの値を編集します：

```sh
cp ipd.toml ipd-all-c.toml
```

`ipd-all-c.toml` のシミュレーション名を `ipd_tft_vs_all_c` に変え，第2の集団の

```toml
rule  = "fictitious_play"
```

を次のように変えます：

```toml
rule  = "all_c"
```

変更したシナリオを実行します：

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

今度は両者が常に協力するため，平均利得は `1` から `3` に上がります．正の後悔（`2`）と均衡までの距離（`4`）には注意が必要です．この協力経路では相互裏切りより両者の利得が高くなりますが，1回限りの段階ゲームのNash均衡ではありません．

### 3. 同じゲームをRustで組み立てる

シナリオモードは組込みのゲームと規則を扱います．ライブラリモードでは，独自のゲーム，規則，Matching，実験状態を指定できます．バイナリcrateを作り，`socsim-games` に feature `core,engine` を付けます：

```toml
[dependencies]
socsim-games = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["core", "engine"] }
```

次に `src/main.rs` に以下を書きます（[`socsim-games` ガイド](../games.ja.md#最小の例繰り返し囚人のジレンマ)と同じ最小構成です）：

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

`GameWorld::payoffs()` が持つのは現在のステップの値だけなので，ループで得点を累積します．`assemble` はMatching，決定，学習，利得の各メカニズムを登録します．ライブラリモードの実行で記録列が必要なら，`GameMetrics` を別に足します．

### 4. 記録した出力を読む

シナリオは一時ディレクトリの下に `runs/ipd_tft_vs_fp_42.jsonl` を書きます．CLIの表は10ステップごとに表示しますが，JSONLにはキーとステップごとに1つのメトリクスレコードがあります：

```json
{"key":"game.played_share.a1","t":1,"type":"metric","value":0.0}
{"key":"game.played_share.a1","t":2,"type":"metric","value":0.5}
{"key":"game.played_share.a1","t":3,"type":"metric","value":1.0}
```

このシナリオの列は次を表します：

- `game.played_share.a0` / `.a1`：行動0（`C`）/ 行動1（`D`）を打った席の割合；
- `game.coop_rate`：`cooperate_label` で指定した行動を使った割合；
- `game.mean_payoff` / `game.payoff_sd`：対戦したエージェントの利得の平均と母標準偏差；
- `game.regret`：一方的な逸脱で得られる利得の増分の最大値；
- `game.eq_distance`：解いたNash均衡のうち最も近いものまでのL1距離；
- `game.action_entropy`：打たれた行動割合のShannonエントロピー．

定義できないメトリクスはNaNとして書かず，省略します．厳密な定義と任意のイベント記録は [`game_metrics`](../mechanisms/game-metrics.ja.md) を参照してください．

### 5. `socsim-tools` で行動割合を図にする

リポジトリには，ゲーム定義と求めた均衡を伴う5シードのIPD fixtureがあります．リポジトリのルートから，行動割合の平均と95 % t区間を別の一時ディレクトリへ描きます：

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

`shares_played_share.png` ができます．runに `game.roster` と `game.step_actions` のイベントもあれば，`plot-actions` で行動ラスターを描けます．[記録済みrunの可視化ガイド](../games.ja.md#記録した-run-の割合の系列と行動のラスター)では，両コマンドとイベント量のトレードオフを説明しています．

## 実行する

このチュートリアルの最短経路は次のとおりです：

```sh
work=$(mktemp -d) && cd "$work"
socsim init --module-pack normal-form --out ipd.toml
socsim run ipd.toml
# 上のソースを置いた一時的なRust crateで：
cargo run -q
```

## 学んだこと

- `normal-form` パックは，シナリオから段階ゲーム，集団，Matching，決定規則，利得評価，ステップごとの指標を組み立てます．
- 利得行列を変えなくても，集団の規則を1つ変えると，相互裏切りから持続的な協力へ軌跡が変わります．
- Rustでは `build_world` がゲームと規則の状態を持ち，`assemble` が実行可能なゲームのメカニズムを足します．
- `game_metrics` は，割合，利得，後悔，均衡までの距離，エントロピーをJSONLのメトリクス行として記録します．
- `socsim-tools[games]` は複数シードを集約して割合の系列を描きます．行動ラスターには行動イベントが必要です．

## 次へ

- [戦略形ゲーム](../games.ja.md#scenario-ファイル) — シナリオの全スキーマ，ライブラリのビルダー，集団動学，ソルバー，可視化の契約．
- [`normal-form` パック](../packs/normal-form.ja.md) — CLI固有の検査，スターターシナリオ，検証付きスイープ．
- ゲームのメカニズム：[`game_match`](../mechanisms/game-match.ja.md)，[`game_decide`](../mechanisms/game-decide.ja.md)，[`game_play`](../mechanisms/game-play.ja.md)，[`game_metrics`](../mechanisms/game-metrics.ja.md)．
- [ユースケース：戦略形ゲーム](../usecases.ja.md#8-シナリオから戦略形ゲームを走らせる) — 空間ゲーム，パラメータスイープ，連続行動の寡占．
