[English](library.md) | **日本語**

# ライブラリAPI

`socsim` はRustライブラリとして利用できます：関連するクレートを `Cargo.toml` の依存関係に追加し，プログラムからシミュレーションを構築します．このページでは，カスタム `Mechanism` の実装からシミュレーションの実行までの完全なワークフローを説明します．

---

## コア抽象化

すべてのsocsimのロジックは `socsim-core` で定義された4つのトレイトに基づいています：

| トレイト | 役割 |
|---|---|
| `WorldState` | 全共有シミュレーション状態（エージェント，クロック，ドメインデータ）を所有する |
| `Mechanism<W>` | 1つのコンポーザブルな研究ロジック単位；1つ以上の `Phase` で実行される |
| `Scheduler<W>` | 各ステップのエージェント活性化順序を決定する |
| `Recorder` | メトリクスと構造化イベントのシンク |

`StepContext<'_, W>` はすべての `Mechanism::apply` 呼び出しに渡され，ワールドへの可変アクセス，クロックのコピー，RNG，レコーダー，およびそのステップの活性化順序を提供します．

---

## ステップ1 — `WorldState` を実装する

`WorldState` はエージェントのロスターとクロックを提供する必要があります．それ以外のドメイン状態は自由に定義できます．

```rust,ignore
use socsim_core::{AgentId, SimClock, WorldState};

struct CounterWorld {
    clock: SimClock,
    agents: Vec<AgentId>,
    pub value: f64,
}

impl CounterWorld {
    fn new(t_max: u64) -> Self {
        Self {
            clock: SimClock::new(t_max),
            agents: vec![AgentId(0)],
            value: 0.0,
        }
    }
}

impl WorldState for CounterWorld {
    fn agent_ids(&self) -> Vec<AgentId> { self.agents.clone() }
    fn clock(&self) -> &SimClock { &self.clock }
    fn clock_mut(&mut self) -> &mut SimClock { &mut self.clock }
}
```

---

## ステップ2 — `Mechanism` を実装する

メカニズムは参加する `Phase` を宣言し，それらのフェーズ中に `StepContext` を受け取ります．

```rust,ignore
use socsim_core::{Mechanism, Phase, Result, StepContext};

struct GrowthMechanism {
    rate: f64,
}

impl Mechanism<CounterWorld> for GrowthMechanism {
    fn name(&self) -> &str { "growth" }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Environment]
    }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, CounterWorld>) -> Result<()> {
        ctx.world.value += self.rate;
        ctx.recorder.record_metric(ctx.clock.t(), "value", ctx.world.value);
        Ok(())
    }
}
```

6つのフェーズは実行順に以下の通りです：

| フェーズ | 典型的な用途 |
|---|---|
| `PreStep` | ブックキーピング，ステップ毎のカウンターリセット |
| `Environment` | 外生的ショック，リソース補充，学習曲線 |
| `Decision` | エージェントの意思決定（離職意図，採用） |
| `Interaction` | ピア効果，ネットワーク拡散，感染 |
| `Reward` | 報酬の計算と適用；集計メトリクスの記録 |
| `PostStep` | クリーンアップ，社会化，離職/採用イベントの発行 |

メカニズムは `phases()` から長いスライスを返すことで複数のフェーズに登録できます．登録された各フェーズで `Phase::ORDER` の順に1回ずつ呼び出されます．

---

## ステップ3 — `ModulePack` としてまとめる（推奨）

`ModulePack` は関連するメカニズムを名前付きバンドルにまとめます．これはCLIの `--module-pack` の概念と対応しており，1回の呼び出しで研究モジュール全体を有効化できます．

```rust,ignore
use socsim_config::{ModulePack, Params, Registry};

struct DemoPack;

impl ModulePack<CounterWorld> for DemoPack {
    fn pack_name(&self) -> &str { "demo" }

    fn register(&self, reg: &mut Registry<CounterWorld>) {
        reg.register("growth", |params| {
            let rate = params.get_f64("rate", 1.0);
            Ok(Box::new(GrowthMechanism { rate }))
        });
    }
}
```

`Params` はTOMLテーブルをバックエンドとした，型付きでデフォルト値を持つゲッター（`get_f64`, `get_u64`, `get_bool`, `get_str` など）を提供します．コンストラクターは常に適切なデフォルト値を設定してください．

---

## ステップ4 — `Registry` でメカニズムを登録・ビルドする

```rust,ignore
use socsim_config::Params;

// パックを通じて登録
let mut reg: Registry<CounterWorld> = Registry::new();
DemoPack.register(&mut reg);

// または個別のコンストラクターを直接登録することも可能
// reg.register("growth", |params| { ... });

// レジストリからインスタンス化
let params = Params::empty(); // またはTOMLテーブルから構築
let growth: Box<dyn Mechanism<CounterWorld>> = reg.build("growth", &params).unwrap();
```

---

## ステップ5 — `SimulationBuilder` で組み立てて実行する

`SimulationBuilder` はデフォルト値を持つfluentビルダーです：

| オプション | デフォルト |
|---|---|
| scheduler | `SequentialScheduler`（`AgentId` のソート順） |
| seed | `0` |
| recorder | `NullRecorder`（no-op） |

```rust,ignore
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let world = CounterWorld::new(10); // 10ステップ実行
let mut sim = SimulationBuilder::new(world)
    .add_mechanism(growth)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)          // 固定シード → 完全に決定論的
    .build();

sim.run().unwrap();

println!("Final value: {}", sim.world().value);
```

`Simulation::run` は `world.clock().is_done()` になる**か**，メカニズムが早期停止を要求するまでループします．細かい制御が必要な場合は `Simulation::step` を使って1ステップずつ進めます．

---

## 収束時の早期停止

多くのABMは `t_max` よりもずっと早く不動点に到達します．`run()` を諦めて `step()` ループを手書きしなくて済むように，2つのメカニズムが用意されています：

- **メカニズムの内部から** `ctx.request_stop()` を呼び出します．現在のステップは最後まで実行され（残りのすべてのメカニズムが動作する），その後 `run()` が終了します．後から `sim.stop_requested()` で問い合わせできます．
- **ドライバーから** `run_until(predicate)` を使います．これは各ステップの*後*にワールドに対して述語をチェックします：

```rust,ignore
// Stop as soon as the world reports convergence (but always at least one step).
sim.run_until(|w| w.is_converged())?;
```

```rust,ignore
// Equivalent from inside a mechanism (PostStep is a good place to check):
fn apply(&mut self, _p: Phase, ctx: &mut StepContext<'_, MyWorld>) -> Result<()> {
    if ctx.world.no_agent_moved_this_step() {
        ctx.request_stop();
    }
    Ok(())
}
```

---

## ステップごとの観測：`run_observed` / `StepReport`

**各ステップの後に**メトリクスが必要なとき — 収束曲線，ティックごとのカウント，進捗のライブ表示など — `step()` でループを自分で駆動し，`world()` / `scratch()` を読むこともできます．`run_observed` はそのパターンをまとめたもので，ループを手書きしたり，壊れやすい文字列ベースのスクラッチ読み出しに頼ったりする必要をなくします：

```rust,ignore
let mut history = Vec::new();
sim.run_observed(|report| {
    // report: StepReport { t, stopped, world, scratch }
    history.push(report.world.distinct_opinions());
})?;
```

クロージャは実行された各ステップごとに1回，そのステップの**後**の状態を反映した `StepReport` とともに呼び出されます：

| フィールド | 意味 |
|---|---|
| `t` | ステップ後のクロック時刻 |
| `stopped` | このステップ中にメカニズムが停止を要求した場合 `true` |
| `world` | ステップ後の共有 `&W` |
| `scratch` | ステップ後の共有 `&Blackboard`（メカニズムが残したステップ毎の値） |

終了条件は `run()` と同じです（クロック完了**または**停止要求）；オブザーバーは停止が要求されたステップでも呼ばれ（そのレポートは `stopped == true`），その後のステップでは呼ばれません．1ステップずつ進めたい場合は `step_reported()` が1ステップ分の同じ `StepReport` を返します．

これがライブラリモデルにおける推奨のステップごとループです — `crates/socsim-engine/examples/cellular_automata.rs` を参照してください．

---

## エージェントの部分集合に作用する

スケジューラーは**すべての**エージェントに対する活性化順序を返します．しかし多くのモデルでは，ある条件を満たす部分集合（分居モデルにおける*不満を持つ者*，感染モデルにおける*感染者*）にのみ作用します．慣用的なパターンは，**ステップ開始時に対象集合をスナップショットする**ことであり，その後活性化順序をそれに対してフィルタリングします：

```rust,ignore
fn apply(&mut self, _p: Phase, ctx: &mut StepContext<'_, MyWorld>) -> Result<()> {
    // Snapshot eligible agents BEFORE anyone acts, so mid-step state changes
    // (e.g. a neighbour moving away) don't pull extra agents into this step.
    let eligible: std::collections::HashSet<AgentId> = ctx.world
        .agent_ids().into_iter()
        .filter(|id| ctx.world.is_eligible(*id))
        .collect();

    for id in ctx.agent_order {              // shuffled by the scheduler
        if !eligible.contains(id) { continue; }
        if ctx.world.is_eligible(*id) {      // may have changed since snapshot
            ctx.world.act(*id);
        }
    }
    Ok(())
}
```

（すでにシャッフルされた）全体の順序をフィルタリングすることは，対象となる部分集合だけをシャッフルすることと統計的に等価です．**同期的か非同期的かのセマンティクスは重要です：** 対象集合をスナップショットすると同期的スタイルの更新になります（作用するエージェントの数はステップ開始時に固定される）；その時点で対象となっているエージェントに作用すると非同期的な更新になります．意図的に選択してください — ダイナミクスが変わります．

---

## ステップスコープのスクラッチ（`Blackboard`）

`ctx.scratch` は，エンジンが**各ステップの開始時にクリアする**型消去されたキー/バリューストアです．`WorldState` にステップ毎のブックキーピングフィールドを追加することなく，同じステップ内のメカニズム間で，あるいはドライバーへ一時的な値を渡すために使います：

```rust,ignore
// In a mechanism:
ctx.scratch.insert("n_moved", n_moved_usize);

// In a later mechanism the same step, or in the driver right after step():
let moved = sim.scratch().get::<usize>("n_moved").copied().unwrap_or(0);
```

ステップ中に書き込まれた値は，次の `step()` 呼び出しまで読み取り可能であり，その後クリアされます．

---

## 決定論性

決定論性は3つの設計原則により保証されています：

1. **シードされたChaCha20 RNG．** `SimRng::from_seed(seed)` は完全に決定論的なジェネレーターを作成します．同じシード＋同じコードは常に同じ軌跡を生成します．
2. **ソートされたエージェントID．** `HrWorld` の `WorldState::agent_ids` はIDをソート順に返し，チーム平均の集計はソートされたコピーで反復します．ハッシュマップの反復順序は結果に影響しません．
3. **`SimRng::derive` による子RNG．** メカニズムは `SimRng::derive(&[agent_id, phase_index])` を使い，親ストリームを変更せずにエージェントやフェーズごとの独立した子RNGを派生させることができます．

### ワールド初期化用RNGをエンジンのRNGと分離する

`SimulationBuilder::seed(seed)` はエンジンのRNGを内部で構築しますが，ビルダーが存在する**前**にもランダム性が必要になることがよくあります — 例えばワールドを構築するときにエージェントを配置する場合などです．2つの独立した `SimRng` を*同じ* `seed` でシードしても動作しますが，2つのストリームが結合してしまいます．きれいなパターンは，`seed` を**ルート**として扱い，ラベル付けされた子シードを派生させることです：

```rust,ignore
use socsim_core::{derive_seed, SimRng};

const RNG_WORLD_INIT: u64 = 0;
const RNG_ENGINE: u64 = 1;

let root = seed;
let mut init_rng = SimRng::from_seed(derive_seed(root, &[RNG_WORLD_INIT]));
let world = MyWorld::new(&mut init_rng);          // place agents, etc.

let mut sim = SimulationBuilder::new(world)
    .seed(derive_seed(root, &[RNG_ENGINE]))       // independent, labelled stream
    .build();
```

`derive_seed`（`socsim-core` から再エクスポートされています）は `SimRng::derive` が使うものと同じFNV-1aミックスなので，2つのストリームは無相関でありながら，単一のルートシードから完全に再現可能です．

#### RNGストリームのラベル付け規約

各モデルが独自のラベルを再発明しなくて済むように，socsim はほぼすべてのモデルが必要とする2つのストリームに対して，小さな固定規約を推奨します：

| ラベル | ストリーム |
|---|---|
| `derive_seed(root, &[0])` | ワールド初期化（エージェント配置，セルのランダム化） |
| `derive_seed(root, &[1])` | エンジン / スケジューラー（`SimulationBuilder::seed` に渡す） |

```rust,ignore
let root = seed;
let mut init_rng = SimRng::from_seed(derive_seed(root, &[0])); // world init
let world = MyWorld::new(&mut init_rng);
let mut sim = SimulationBuilder::new(world)
    .seed(derive_seed(root, &[1]))                              // engine
    .build();
```

モデルが所有する追加の独立ストリームには，さらなるラベル（`&[2]`，`&[3]`，…）を割り当ててください．`cellular_automata` の例はまさにこの規約に従っています．

自作のコードで決定論性を検証するには，同じシードで2つのシミュレーションを実行して出力を比較します — `custom_mechanism.rs` の例がまさにこれを行っています．

---

## メトリクスとイベントの記録

`Recorder` トレイトには3つの記録メソッドがあります：

```rust,ignore
fn record_metric(&mut self, t: u64, key: &str, value: f64);
fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value);
// Wide tabular row — many named columns sharing one t and table:
fn record_row(&mut self, t: u64, table: &str, row: &[(&str, f64)]);
```

`record_row` は，多数の列を持つ `metrics.csv` 形式の出力に自然な形です；デフォルト実装は1つの行を `"{table}.{column}"` をキーとする `record_metric` 呼び出しへと展開するので，これをオーバーライドしないレコーダーも引き続き動作します．

エンジンの**デフォルト**レコーダーは `NullRecorder`（`socsim-core` に定義）です．これはすべてを破棄するno-opシンクです．このため，エンジンはもはや `socsim-log` に依存しません：自前で出力を行う純粋なライブラリモデル（`cellular_automata` の例のような）は `socsim-core` / `socsim-engine` / `socsim-grid` だけで足り，具体的なレコーダーを取り込む必要はありません．メトリクス／イベントの記録が実際に必要なときにだけ `socsim-log` を追加し，`SimulationBuilder::recorder(...)` を呼び出します．

`socsim-log` には3つの具体的な実装が含まれています：

| 型 | 用途 |
|---|---|
| `InMemoryRecorder` | テスト；実行後に `metrics()` と `events()` を検査 |
| `JsonlRecorder<W>` | 本番環境；任意の `Write` シンクに1レコードあたり1行のJSONを書き出す |
| `CsvRecorder` | 表形式の出力；テーブルごとに `record_row` 呼び出しを蓄積し，列を揃えたCSVを描画する（加えてロングフォーマットの `metrics_csv()` も） |

```rust,ignore
use socsim_core::Recorder;
use socsim_log::CsvRecorder;

let mut rec = CsvRecorder::new();
rec.record_row(0, "metrics", &[("avg_same", 0.53), ("n_moved", 0.0)]);
rec.record_row(1, "metrics", &[("avg_same", 0.64), ("n_moved", 21.0)]);
let csv = rec.table_csv("metrics").unwrap();   // "t,avg_same,n_moved\n0,0.53,0\n1,0.64,21\n"
std::fs::write("metrics.csv", csv).unwrap();
```

デフォルトでは `CsvRecorder` は列が最初に観測された順序で列を発見します．**呼び出し側が定義した**列の順序とスキーマを固定したい場合 — 下流のツールが正確なヘッダーを期待する場合などに便利です — 描画の前に `set_columns` を呼び出します：

```rust,ignore
rec.set_columns("metrics", &["n_moved", "avg_same"]);  // 列順を固定
let csv = rec.table_csv("metrics").unwrap();            // ヘッダー: "t,n_moved,avg_same"
```

スキーマに列挙されていない列は省略されます；ある行に存在しないスキーマ列は空フィールドとして描画されます．`set_columns` は描画方法にのみ影響し，どの行が保存されるかには影響しません．

`sim.run()` 後にレコーダーを検査する：

```rust,ignore
use socsim_log::InMemoryRecorder;

let rec = sim.recorder()
    .as_any()
    .and_then(|a| a.downcast_ref::<InMemoryRecorder>())
    .unwrap();

for row in rec.metrics() {
    println!("t={} {}={}", row.t, row.key, row.value);
}
```

---

## HRライフサイクルモジュールをライブラリとして利用する

`socsim-hr-lifecycle` は `HrWorld`，`HrLifecyclePack`，そして個別エージェント `Employee` とチーム `Team` の構造体をエクスポートします．CLIを使わずプログラムから利用するには：

```rust,ignore
use socsim_hr_lifecycle::{HrWorld, HrLifecyclePack};
use socsim_config::{ModulePack, Params, Registry};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};
use socsim_core::SimRng;

let seed = 42u64;
let mut rng = SimRng::from_seed(seed);
let mut world = HrWorld::new(5, 8, 4, 0.1, &mut rng);
world.clock = socsim_core::SimClock::new(60);

let mut reg = Registry::new();
HrLifecyclePack.register(&mut reg);

let p = Params::empty();
let names = ["learning_curve","peer_effect","ocb","fit",
              "turnover","knowledge_loss","toxic_spread",
              "hiring","socialization","org_performance"];

let mut builder = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(seed);

for name in &names {
    builder = builder.add_mechanism(reg.build(name, &p).unwrap());
}

let mut sim = builder.build();
sim.run().unwrap();

println!("org_performance = {}", sim.world().org_performance);
```

完全な出力付きバージョンは `crates/socsim-hr-lifecycle/examples/hr_baseline.rs` を参照してください．

---

## `socsim-grid` による空間モデル

格子ベースのモデル（分居，格子上の感染，拡散）のために，`socsim-grid` は既製の2D空間を提供するので，近傍や距離を再実装せずに済みます：

```rust,ignore
use socsim_grid::{Grid, GridIndex, Boundary, Neighborhood, Metric};
use socsim_core::AgentId;

let mut idx = GridIndex::new(Grid::new(13, 16, Boundary::Fixed));
idx.place(AgentId(0), 3, 4).unwrap();

let nbrs = idx.grid().neighbors(3, 4, Neighborhood::Moore);     // 8-neighbourhood
let occupied = idx.occupant_neighbors(3, 4, Neighborhood::Moore);
let target = idx.nearest_vacant((3, 4), Metric::Chebyshev);     // greedy relocation
idx.move_to(AgentId(0), target.unwrap().0, target.unwrap().1).unwrap();
```

| 型 | 役割 |
|---|---|
| `Grid` | 寸法 + `Boundary`（`Fixed` / `Toroidal`）；`neighbors`, `neighbors_radius`, ラップ対応の `distance` |
| `Neighborhood` | `Moore`（8） / `VonNeumann`（4） |
| `Metric` | `Chebyshev` / `Manhattan` / `Euclidean` |
| `GridIndex` | `AgentId ↔ cell` の占有：`place`, `move_to`, `vacant_cells`, `nearest_vacant`, ソートされた `agent_ids` |
| `CellGrid<T>` | すべてのセルにセルごとの可変状態 `T`（セルオートマトン／格子属性モデル） |
| `Adjacency` | ホットな格子ループ向けの事前計算済みCSR近傍テーブル |

`WorldState` の内部に `GridIndex`（あるいは素の `Grid`）を保持し，`Mechanism` から移動を駆動します．

### アロケーションを伴わない近傍クエリ

`Grid::neighbors` は呼び出しごとに新しい `Vec` をアロケートします．これは時折のルックアップには問題ありませんが，ホットループでは無駄です．ステップごとの格子コードでは，次のいずれかを推奨します：

- `Grid::neighbors_into(r, c, nbhd, &mut buf)` / `neighbors_radius_into(...)` — 呼び出し側が所有する1つの `Vec` を呼び出し間で再利用し（クリアして再充填する），呼び出しごとのアロケーションを回避します．
- `Grid::neighbors_iter(r, c, nbhd)` — 半径1のイテレーターで，近傍をスタックから直接生成し，ヒープアロケーションを一切行いません．
- `Grid::adjacency(nbhd)` / `adjacency_radius(nbhd, radius)` — 近傍テーブル全体を **一度だけ事前計算** して `Adjacency`（CSR，行優先のフラットインデックス）にします．`adj.neighbors(idx)` はセル `idx = r * cols + c` の近傍をO(1)の借用 `&[usize]` として返します．これは*同じ*近傍集合を毎ティック問い合わせる場合（セルオートマトン，拡散，格子上の感染）に推奨される構造です：ワールド構築時に構築し，`WorldState` に保持してください．

4つともすべて同じ決定論的なソート済み行優先順で近傍を返すので，結果は相互に交換可能です．

### `CellGrid<T>` によるセルごとの状態

`GridIndex` が「このセルに*どのエージェント*がいるか」に答えるのに対し，`CellGrid<T>` は**すべての**セルに値 `T` を保持します — セルオートマトンや格子属性モデル（各セルが意見・戦略・カウンターを保持する）の基本要素です．`Grid` の境界対応の近傍クエリと，行優先のバッキング `Vec<T>` への直接的な可変アクセスを組み合わせます：

```rust,ignore
use socsim_grid::{CellGrid, Grid, Boundary, Neighborhood};

// 意見の格子を構築；各セルは座標から（あるいはRNGから）初期化する．
let grid = Grid::new(16, 16, Boundary::Toroidal);
let adjacency = grid.adjacency(Neighborhood::Moore);   // 一度だけ事前計算
let mut cells: CellGrid<u8> = CellGrid::from_fn(grid, |r, c| ((r + c) % 4) as u8);

// ホットループ：O(1)の近傍ルックアップ，直接的なセル変更，アロケーションなし．
let idx = 5 * 16 + 7;                       // セル (5, 7)，行優先のフラット
let nbr = adjacency.neighbors(idx)[0];      // 近傍のフラットインデックス
let opinion = *cells.get_idx(nbr).unwrap();
*cells.get_idx_mut(idx).unwrap() = opinion; // それをコピーする
```

コンストラクタ：`CellGrid::new(grid, fill)`（すべてのセル `= fill.clone()`）と `CellGrid::from_fn(grid, |r, c| ...)`．アクセスは座標で（`get` / `get_mut`），フラットインデックスで（`get_idx` / `get_idx_mut`，`Adjacency` と一致），あるいは行優先のスライス全体で（`cells` / `cells_mut`）；`neighbors` / `neighbor_values` は近傍を直接読み取ります．`CellGrid` + `Adjacency` で構築した動作するイベント駆動CAは `crates/socsim-engine/examples/cellular_automata.rs` にあります．

---

## 軽量：エンジンのみの利用（TOML / Runner なし）

`ModulePack` → `Registry` → シナリオTOML → `socsim-runner` という経路（上記のステップ3〜4）はオプションです．すでに独自のCLIと出力形式を持っている場合 — 例えば既存プロジェクトを移植する場合 — **エンジンコアだけ**を使い，TOML，レジストリ，ランナーを完全にスキップできます：

```rust,ignore
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

// 1. Build the world yourself (your own config struct, your own RNG).
let world = MyWorld::new(/* ... */);

// 2. Add mechanisms directly — no Registry, no ModulePack.
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(seed)
    .add_mechanism(Box::new(MyMechanism::new(/* ... */)))
    .build();

// 3. Drive it yourself and stop on convergence; write your own output.
sim.run_until(|w| w.is_converged())?;
write_my_csv(sim.world());          // your existing schema, no Recorder required
```

**どちらを選ぶか：**

| | フルスタック（ModulePack + TOML + Runner） | エンジンのみ |
|---|---|---|
| 設定 | シナリオ `.toml`，`socsim-runner` でスイープ | 独自の構造体 / CLI |
| 出力 | `JsonlRecorder` / ランナーサマリー | 自分で書くもの |
| 最適な用途 | 新規プロジェクト，パラメータスイープ，再現可能なシナリオファイル | 既存ツールへのエンジン埋め込み，カスタム出力スキーマ |

動作するライブラリモードの例は `crates/socsim-engine/examples/engine_only.rs`（収束する非空間モデル）と `crates/socsim-engine/examples/cellular_automata.rs`（`run_observed` を使い `CellGrid` + `Adjacency` 上に構築したイベント駆動の格子CA）にあります．

---

## スナップショット：保存と再開

World が `serde` を導出していれば，実行の**可変状態**（World + 厳密な RNG ストリーム + クロック + stop フラグ）を捕捉・復元できます．mechanisms・scheduler・recorder は捕捉されません — シミュレーションを再構築するときに用意するコードです（PyTorch の `state_dict` と architecture の分離）．

```rust,ignore
use socsim_engine::{SimulationBuilder, Snapshot};

// 1. World は serde シリアライズ可能（メモリ上スナップショットには Clone も）であること．
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct MyWorld { /* ... */ }

let mut sim = SimulationBuilder::new(MyWorld::new(/* ... */)).seed(7).build();
for _ in 0..100 { sim.step()?; }

// 2. 捕捉 — メモリ上または JSON ファイルへ．
let snap = sim.snapshot();            // W: Clone が必要
snap.save("run.snapshot.json")?;      // W: Serialize が必要

// 3. 後で（別プロセスでも）：同じ mechanisms を再構築してから復元．
let snap = Snapshot::load("run.snapshot.json")?;   // 版チェックあり
let mut resumed = SimulationBuilder::new(MyWorld::placeholder())
    .seed(0)                          // restore で上書きされる
    .add_mechanism(Box::new(MyMechanism::new(/* 以前と同じ */)))
    .build();
resumed.restore(snap);
resumed.run()?;                       // ステップ100からビット単位で継続
```

これらのメソッドは `W: Clone` / `Serialize` / `DeserializeOwned` でゲートした `impl` ブロックで追加されるため，`WorldState` トレイトは不変です — serde 非対応の World は単にこれらを持ちません．参照実装の `HrWorld`（`{nodes, edges}` としてシリアライズされる `SocialNetwork` を含む）は完全に serde 対応です．`examples/snapshot_resume.rs` を参照してください．同じ mechanisms で構築したシミュレーションに復元すれば，新しいシミュレーションのシードに関わらず，保存時点以降の実行がビット単位で再現されます．

---

## 学習ポリシー（MARL）

`Decision` フェーズは `socsim-marl`（Phase 6）で*学習可能*にできます：`PolicyMechanism` が `Policy` をラップし，他のメカニズムと同様に6フェーズループに差し込めます．既定の `Policy` は `DiscretePolicyNet`（[`burn`](https://burn.dev) の小さな MLP を CPU 上で REINFORCE 学習）で，重みは `SimRng` からシードされビット再現可能です．ポリシーはフラットな `&[f32]` 特徴と `usize` 行動を扱うため，3つの小さなトレイトで World を橋渡しします：

```rust,ignore
use socsim_marl::{
    ActionApplier, DiscretePolicyNet, MarlTrainer, NetConfig, ObsEncoder,
    PolicyMechanism, RewardFn, TrainConfig, TrajectoryBuffer,
};

struct MyEncoder;          // World + agent → 特徴ベクトル
impl ObsEncoder<MyWorld> for MyEncoder {
    fn obs_dim(&self) -> usize { 4 }
    fn encode(&self, w: &MyWorld, a: AgentId) -> Option<Vec<f32>> { /* ... */ }
}
struct MyApplier;          // 選択された行動インデックス → World の変更
impl ActionApplier<MyWorld> for MyApplier {
    fn n_actions(&self) -> usize { 2 }
    fn apply(&self, w: &mut MyWorld, a: AgentId, action: usize, rng: &mut SimRng) { /* ... */ }
}
struct MyReward;           // 各ステップ後に読むエージェント単位の報酬
impl RewardFn<MyWorld> for MyReward {
    fn reward(&self, w: &MyWorld, a: AgentId) -> f32 { /* ... */ }
}

// 外側の学習ループ：エピソードごとに collect モードのポリシーで新規 sim を構築．
let net = std::rc::Rc::new(std::cell::RefCell::new(
    DiscretePolicyNet::new(NetConfig::new(4, 2), &mut SimRng::from_seed(0))?,
));
let mut trainer = MarlTrainer::new(net);
let stats = trainer.train(
    &TrainConfig { episodes: 50, seed: 0 },
    |policy, buffer: std::rc::Rc<std::cell::RefCell<TrajectoryBuffer>>, seed| {
        SimulationBuilder::new(MyWorld::new(/* ... */))
            .seed(seed)
            .add_mechanism(Box::new(PolicyMechanism::collecting(
                policy, MyEncoder, MyApplier, buffer)))
            .build()
    },
    &MyReward,
)?;
```

学習後は `PolicyMechanism::inference(policy, …)` でメカニズムを構築すると**凍結**ポリシーを実行できます：貪欲行動を取り，RNG を消費せず，ビット再現可能です．`socsim-marl` は `burn` を取り込むため，hr-lifecycle 連携は `marl` feature でゲートしています（`cargo run -p socsim-hr-lifecycle --features marl --example marl_turnover`）．
