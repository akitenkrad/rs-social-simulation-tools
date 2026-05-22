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
| recorder | `InMemoryRecorder` |

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

`Simulation::run` は `world.clock().is_done()` になるまでループします．細かい制御が必要な場合は `Simulation::step` を使って1ステップずつ進めます．

---

## 決定論性

決定論性は3つの設計原則により保証されています：

1. **シードされたChaCha20 RNG．** `SimRng::from_seed(seed)` は完全に決定論的なジェネレーターを作成します．同じシード＋同じコードは常に同じ軌跡を生成します．
2. **ソートされたエージェントID．** `HrWorld` の `WorldState::agent_ids` はIDをソート順に返し，チーム平均の集計はソートされたコピーで反復します．ハッシュマップの反復順序は結果に影響しません．
3. **`SimRng::derive` による子RNG．** メカニズムは `SimRng::derive(&[agent_id, phase_index])` を使い，親ストリームを変更せずにエージェントやフェーズごとの独立した子RNGを派生させることができます．

自作のコードで決定論性を検証するには，同じシードで2つのシミュレーションを実行して出力を比較します — `custom_mechanism.rs` の例がまさにこれを行っています．

---

## メトリクスとイベントの記録

`Recorder` トレイトには2つのメソッドがあります：

```rust,ignore
fn record_metric(&mut self, t: u64, key: &str, value: f64);
fn record_event(&mut self, t: u64, kind: &str, payload: serde_json::Value);
```

`socsim-log` には2つの実装が含まれています：

| 型 | 用途 |
|---|---|
| `InMemoryRecorder` | テスト；実行後に `metrics()` と `events()` を検査 |
| `JsonlRecorder<W>` | 本番環境；任意の `Write` シンクに1レコードあたり1行のJSONを書き出す |

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
