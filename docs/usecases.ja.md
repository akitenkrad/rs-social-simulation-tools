[English](usecases.md) | **日本語**

# ユースケース＆レシピ

このページでは，代表的な研究タスクのコピー＆ペースト可能なワークフローを紹介します．

---

## 1. HRライフサイクルベースラインの実行

同梱のシナリオ `scenarios/hr_lifecycle_baseline.toml` は，5チーム×40エージェントのHRライフサイクルモデルをシード42で60ステップ（月次）実行します．モデルの全体像は[hr-lifecycle パック](packs/hr-lifecycle.ja.md)を参照してください．

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
```

CLIは10ステップごとのメトリクス系列（最後のステップを含む）を出力し，`runs/hr_lifecycle_baseline_42.jsonl` にJSONLログを書き出します．

後からJSONLをCSVサマリーとして読み込む（再実行不要）：

```sh
socsim summarize runs/hr_lifecycle_baseline_42.jsonl
```

### 別のパックを実行する — 意見ダイナミクス

CLIはWorld多態なので，同じコマンドで任意のパックを駆動できます．同梱の `scenarios/opinion_dynamics_baseline.toml` は，Watts–Strogatz ソーシャルネットワーク上で Hegselmann–Krause の有界信頼コンセンサスモデルを実行します（[opinion-dynamics パック](packs/opinion-dynamics.ja.md)を参照）：

```sh
socsim run scenarios/opinion_dynamics_baseline.toml
```

```
Running 'opinion_dynamics_baseline' (pack=opinion-dynamics, t_max=60, seeds=[42], parallel=false)

t               clusters         max_delta              mean            spread          variance
10               22.0000            0.1238            0.5092            0.9769            0.0360
20               18.0000            0.0331            0.5088            0.9769            0.0268
30               15.0000            0.0127            0.5094            0.9769            0.0243
40               12.0000            0.0049            0.5097            0.9769            0.0235
50               12.0000            0.0021            0.5098            0.9769            0.0233
60               12.0000            0.0010            0.5098            0.9769            0.0232
```

`clusters`/`variance`/`spread`/`mean` の系列は，エージェントが局所的なコンセンサスに達するにつれ，意見が時間とともにより少ないクラスタへ収束していく様子を示します．`epsilon`（信頼半径）パラメータを大きくすると，集団は完全な合意（単一クラスタ）へ向かいます．

---

## 2. マルチシードによる再現性チェック

シード0〜9で同じシナリオを実行し，決定論性を検証しつつ確率的分散を測定します：

```sh
socsim run scenarios/hr_lifecycle_baseline.toml --seeds 0..10
```

CLIは各メトリクスの平均・標準偏差・最小・最大を含むシード間サマリーテーブルを出力します．結果は決定論的です：同じコマンドを再実行しても常に同一の数値が得られます（各シードが独立したChaCha20 RNGを初期化するため）．

マルチコアマシンでより高速に実行する場合：

```sh
socsim run scenarios/hr_lifecycle_baseline.toml --seeds 0..10 --parallel
```

---

## 3. 仮説検証のためのパラメータスイープ

**研究上の問い：** `toxic_spread.p_spread`（有害行動の感染確率）を高めると，離職率増加を通じて組織パフォーマンスが低下するか？

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "toxic_spread.p_spread=0.2,0.46,0.7" \
    --seeds 0..10 \
    --out runs/toxic_cascade_sweep/
```

3×10 = 30トライアルが実行され，`runs/toxic_cascade_sweep/` に組み合わせごとのCSVが書き出されます．各CSVは `key,mean,std,min,max,n` の列を持ちます．

スイープ出力の例（3シードの場合）：

```
Sweeping 'hr_lifecycle_baseline' over 1 axes × 3 seeds
  toxic_spread.p_spread = [0.2, 0.46, 0.7]
  combo 0: toxic_spread.p_spread=0.2000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               35.3250      5.0624     29.1000     41.5000      3
knowledge_stock          91.9687      4.6135     85.9783     97.2030      3
org_performance          41.3058      2.5623     37.8100     43.8800      3
turnover_rate             0.0167      0.0118      0.0000      0.0250      3
  combo 2: toxic_spread.p_spread=0.7000
metric                      mean         std         min         max      n
------------------------------------------------------------------------
avg_tenure               37.7583      1.6872     35.3750     39.0500      3
knowledge_stock          95.8431      2.2273     92.7248     97.7876      3
org_performance          43.0002      1.4341     40.9778     44.1428      3
turnover_rate             0.0250      0.0204      0.0000      0.0500      3
```

**多次元スイープ** — 2軸を同時にスイープ：

```sh
socsim sweep scenarios/hr_lifecycle_baseline.toml \
    --param "peer_effect.alpha_peer=0.1,0.17,0.3" \
    --param "turnover.quit_cascade_bump=0.1,0.3,0.5" \
    --seeds 0..10 --parallel
```

これにより 3×3 = 9組み合わせ × 10シード = 90トライアルが生成されます．

---

## 4. 新しい研究モジュールの作成

カスタムシミュレーションドメインを追加するには，3つの要素を実装し，`SimulationBuilder` で繋ぎ合わせます．

### ステップ1 — `WorldState` を定義する

```rust,ignore
use socsim_core::{AgentId, SimClock, WorldState};

struct EconWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    pub gdp: f64,
}

impl WorldState for EconWorld {
    fn agent_ids(&self) -> Vec<AgentId> { self.agents.clone() }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
```

### ステップ2 — `Mechanism` を実装する

```rust,ignore
use socsim_core::{Mechanism, Phase, Result, StepContext};

struct GrowthMechanism { rate: f64 }

impl Mechanism<EconWorld> for GrowthMechanism {
    fn name(&self) -> &str { "growth" }
    fn phases(&self) -> &'static [Phase] { &[Phase::Environment] }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, EconWorld>) -> Result<()> {
        ctx.world.gdp *= 1.0 + self.rate;
        ctx.recorder.record_metric(ctx.clock.t(), "gdp", ctx.world.gdp);
        Ok(())
    }
}
```

### ステップ3 — `ModulePack` としてまとめる

```rust,ignore
use socsim_config::{ModulePack, Params, Registry};

struct EconPack;

impl ModulePack<EconWorld> for EconPack {
    fn pack_name(&self) -> &str { "econ" }
    fn register(&self, reg: &mut Registry<EconWorld>) {
        reg.register("growth", |params| {
            let rate = params.get_f64("rate", 0.02);
            Ok(Box::new(GrowthMechanism { rate }))
        });
    }
}
```

### ステップ4 — 組み立てて実行する

```rust,ignore
use socsim_engine::SimulationBuilder;

let mut reg = socsim_config::Registry::new();
EconPack.register(&mut reg);

let world = EconWorld { clock: socsim_core::SimClock::new(24), agents: vec![], gdp: 1000.0 };
let growth = reg.build("growth", &socsim_config::Params::empty()).unwrap();

let mut sim = SimulationBuilder::new(world)
    .add_mechanism(growth)
    .seed(42)
    .build();

sim.run().unwrap();
println!("Final GDP: {}", sim.world().gdp);
```

このパターンの完全な実行可能バージョンは `crates/socsim-engine/examples/custom_mechanism.rs` を参照してください．

---

## 5. 長時間実行の一時停止と再開（スナップショット）

実行をディスクにチェックポイントして後で再開できます — 長時間のスイープ，クラッシュ復旧，共通状態からの分岐 what-if 分析に有用です．World は `serde` を導出している必要があります．スナップショットは World・厳密な RNG ストリーム・クロックを捕捉しますが，mechanisms は**含みません**（再構築します）．

```rust,ignore
use socsim_engine::Snapshot;

// ... 途中まで実行 ...
for _ in 0..12 { sim.step()?; }
sim.snapshot().save("checkpoint.json")?;

// 後で：同じ mechanisms でシミュレーションを再構築してから復元．
let snap = Snapshot::load("checkpoint.json")?;
let mut resumed = build_my_sim(/* 任意のシード */);
resumed.restore(snap);
resumed.run()?;   // 12ヶ月目からビット単位で継続
```

実行可能デモ：`cargo run -p socsim-packs --example snapshot_resume`．詳細は[ライブラリガイド](library.ja.md#スナップショット保存と再開)を参照してください．

---

## 6. 学習する離職ポリシーの訓練（MARL）

固定の意思決定ヒューリスティックを REINFORCE で学習したポリシーに置き換えます．参照モジュールは `marl` feature の背後に学習可能な離職ポリシーを同梱しています：

```sh
cargo run -p socsim-packs --features marl --example marl_turnover
```

これは `burn` のポリシーネットワークを訓練し，従業員が個人合理性報酬によって stay/quit を学習，合理的離職を創発的なポリシーとして再現します．MARL を独自の World に組み込むには `ObsEncoder` / `ActionApplier` / `RewardFn` を実装し `MarlTrainer` を回します — [ライブラリガイド](library.ja.md#学習ポリシーmarl)を参照してください．

---

## 7. イベント駆動 / セルオートマトンの格子モデル

すべてのモデルが1ティックあたりエージェント1アクションというわけではありません．**イベント駆動**（Gillespie型）モデル — 投票者モデル，接触過程の感染，サブティック反応ダイナミクス — は，観測可能な時点の間に*多数*の微小イベントを発火させます．socsim は離散的なティックループ上で動作するので，慣用句は**一定数の微小イベントを1つの `Mechanism::apply` 内でバッチ処理し，それらのイベントを1ティックにマッピングする**ことです．動作する例は `crates/socsim-engine/examples/cellular_automata.rs` — トーラス格子上の投票者モデルです．

```sh
cargo run -p socsim-engine --example cellular_automata
```

### `CellGrid` + 事前計算済み `Adjacency` による格子状態

セルごとの状態を `CellGrid<T>` に保持し，近傍テーブルを `Adjacency` として**一度だけ**事前計算することで，ホットループがアロケーションを行わないようにします：

```rust,ignore
use socsim_grid::{Adjacency, Boundary, CellGrid, Grid, Neighborhood};

struct VoterWorld {
    clock: socsim_core::SimClock,
    cells: CellGrid<u8>,   // セルごとに1つの意見，行優先
    adjacency: Adjacency,  // CSR近傍テーブル，フラットインデックス
}

let grid = Grid::new(16, 16, Boundary::Toroidal);
let adjacency = grid.adjacency(Neighborhood::Moore);            // 一度だけ構築
let cells = CellGrid::from_fn(grid, |_r, _c| rng.gen_range(0..4));
```

### 多数のイベントを1ティックにバッチ処理する

```rust,ignore
fn apply(&mut self, _p: Phase, ctx: &mut StepContext<'_, VoterWorld>) -> Result<()> {
    let n = ctx.world.cells.len();
    // 1エンジンティック == `events_per_step` 個の投票者イベント．
    for _ in 0..self.events_per_step {
        let idx = ctx.rng.gen_range(0..n);                       // ランダムなセル
        let nbrs = ctx.world.adjacency.neighbors(idx);           // O(1)スライス
        if nbrs.is_empty() { continue; }
        let nbr = nbrs[ctx.rng.gen_range(0..nbrs.len())];
        let opinion = *ctx.world.cells.get_idx(nbr).unwrap();
        *ctx.world.cells.get_idx_mut(idx).unwrap() = opinion;    // 近傍をコピー
    }
    // 吸収状態：格子が一様になったら停止する．
    if ctx.world.distinct_opinions() <= 1 { ctx.request_stop(); }
    Ok(())
}
```

### `run_observed` でステップごとのメトリクスを収集する

```rust,ignore
sim.run_observed(|report| {
    // 各ティック後の異なる意見の数；合意時に report.stopped == true
    println!("t={} distinct={}", report.t, report.world.distinct_opinions());
})?;
```

このモデルはデフォルトの `NullRecorder` を使います — `socsim-log` 依存は不要です．近傍APIの詳細は[ライブラリガイド](library.ja.md#アロケーションを伴わない近傍クエリ)を，`run_observed` / `StepReport` については[ステップごとの観測](library.ja.md#ステップごとの観測run_observed--stepreport)の節を参照してください．
