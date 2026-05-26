[English](01-first-model.md) | **日本語**

# T1 — 最初のモデル

**作るもの：** 小さな「冷却」モデルをRustでゼロから — `WorldState` 1つ，`Mechanism` 1つ — 収束で自分自身を停止し，独自のCSVを書き出します．
**所要時間：** 30分．

## 前提

- [T0 — はじめに](00-getting-started.ja.md)（*パック / シナリオ / メトリクス* の概念に慣れていること）．
- 動作するRustツールチェーン．基本的なRust（構造体，トレイト，`impl`）で十分です．

以下のコードは同梱の実例 [`crates/socsim-engine/examples/engine_only.rs`](../../crates/socsim-engine/examples/engine_only.rs) です．このページと並べて開いてください — 上から下まで解説します．

## ステップ

### 1. モデル

各エージェントは「熱」を持ちます．1つのメカニズムが毎ステップ固定のレートで全エージェントを冷やし，すべての熱がゼロになると実行終了です．グリッドもネットワークもなく，制御フローを見るのに必要十分な状態だけです．

### 2. `WorldState` を定義する

`WorldState` は共有状態すべてを所有します．トレイトが要求するのはエージェント名簿とクロックの2つだけ．それ以外（ここでは各エージェントの熱）はあなた次第です：

```rust
struct CoolingWorld {
    clock: SimClock,
    heat: BTreeMap<AgentId, f64>,
}

impl WorldState for CoolingWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        // BTreeMap keys are already sorted — matches the determinism convention.
        self.heat.keys().copied().collect()
    }
    fn clock(&self) -> &SimClock {
        &self.clock
    }
    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}
```

`agent_ids` のコメントに注目：IDを **ソート順** で返すことはsocsimの決定論契約の一部です（`BTreeMap` ならこれが無料で得られます）．ワールドはドライバがポーリングする収束判定も公開します：

```rust
fn is_converged(&self) -> bool {
    self.heat.values().all(|h| *h <= 0.0)
}
```

### 3. `Mechanism` を1つ定義する

メカニズムは研究ロジックの1単位です．6フェーズティックループのどの **フェーズ** で実行するかを宣言し，ロジックを `apply` に書きます．ここでは冷却は意思決定なので `Phase::Decision` で実行します：

```rust
impl Mechanism<CoolingWorld> for CoolingMechanism {
    fn name(&self) -> &str {
        "cooling"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::Decision]
    }
    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, CoolingWorld>) -> Result<()> {
        let mut active = 0usize;
        let mut total = 0.0;
        for id in ctx.agent_order {
            if let Some(h) = ctx.world.heat.get_mut(id) {
                if *h > 0.0 {
                    *h = (*h - self.rate).max(0.0);
                    active += 1;
                }
                total += *h;
            }
        }

        // Hand the step's active count to the driver via step-scoped scratch.
        ctx.scratch.insert("active", active);

        // Wide tabular row — your own column schema.
        ctx.recorder.record_row(
            ctx.clock.t(),
            "cooling",
            &[("active", active as f64), ("total_heat", total)],
        );

        if active == 0 {
            ctx.request_stop();
        }
        Ok(())
    }
}
```

ここに **`StepContext`** の3つの部品が現れます．これらがライブラリモードの核心です：

- `ctx.agent_order` — このステップの活性化順（スケジューラが決めます）．順序が一貫し再現可能になるよう，`agent_ids()` ではなくこれを反復します．
- `ctx.scratch` — エンジンが毎ステップクリアする，ステップスコープのキー/値ストア．`WorldState` にフィールドを足さずに一時値（ここでは `active`）をドライバへ渡すのに使います．
- `ctx.recorder.record_row(...)` — 独自の列名でワイドな表形式の行を出力します．`ctx.request_stop()` はこのステップ後にエンジンを終了するよう要求します．

6つのフェーズは毎ステップ固定の順で実行されます：`PreStep → Environment → Decision → Interaction → Reward → PostStep`．メカニズムは自分が列挙したフェーズでのみ発火します．完全なモデルは [6フェーズティックループ](../architecture.ja.md#6フェーズティックループ) を参照してください．

### 4. `SimulationBuilder` と固定シードで組み立てる

```rust
let world = CoolingWorld::new(5, 1_000); // t_max is a safety cap we never hit
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(SequentialScheduler))
    .seed(42)
    .recorder(Box::new(CsvRecorder::new()))
    .add_mechanism(Box::new(CoolingMechanism { rate: 1.0 }))
    .build();
```

`.seed(42)` は実行を **完全に決定論的** にします：同じシード＋同じコードなら同じ軌跡をビット単位で再現します（socsimはシード付きChaCha20 RNGを使います）．デフォルトのレコーダは何もしない `NullRecorder` です．ここでは行を捕捉するため `CsvRecorder` を選びます．

### 5. `run_until` で駆動し独自の出力を書く

多くのモデルは `t_max` よりずっと前に固定点に達します．盲目的に上限まで走らせる代わりに，自分でループを駆動し収束で停止します：

```rust
sim.run_until(|w| w.is_converged())
    .expect("simulation completed");

let last_active = sim.scratch().get::<usize>("active").copied().unwrap_or(0);
println!(
    "converged at t = {} (t_max = {}), stop_requested = {}, last active = {}",
    sim.world().clock().t(),
    sim.world().clock().t_max(),
    sim.stop_requested(),
    last_active,
);
```

`run_until(predicate)` は各ステップ後にワールドに対して述語を評価し，成立したら停止します．実行後は `sim.world()` から最終状態を，`sim.scratch()` から最後のステップのscratchを読みます．最後にCSVをレコーダから直接取り出します — JSONLもrunnerも不要です：

```rust
let rec = sim
    .recorder()
    .as_any()
    .and_then(|a| a.downcast_ref::<CsvRecorder>())
    .expect("recorder is a CsvRecorder");
print!("{}", rec.table_csv("cooling").expect("table exists"));
```

## 実行する

```sh
cargo run -p socsim-engine --example engine_only
```

```
converged at t = 6 (t_max = 1000), stop_requested = false, last active = 1

t,active,total_heat
1,5,15
2,5,10
3,4,6
4,3,3
5,2,1
6,1,0
```

モデルは（`t_max = 1000` の上限よりはるか手前の）`t = 6` で収束し，CSVはレコーダ自身のテーブルです．もう一度実行しても，固定シードのため出力は同一です．

## 学んだこと

- モデル = **`WorldState`**（共有状態，ソート済み `agent_ids`，クロック）＋ 1つ以上の **`Mechanism`**（それぞれ選んだフェーズで動作）．
- **6フェーズループ** はメカニズムを固定順で実行し，`apply` は `agent_order`，`scratch`，`recorder`，`rng`，`world` を備えた **`StepContext`** を受け取ります．
- 固定 **シード** が実行を **決定論的・再現可能** にします．
- `run_until` は収束で停止し，`ctx.request_stop()` はメカニズム内から同じことを行います．
- 出力は自分で捕捉できます（ここでは `CsvRecorder`）— シナリオTOMLもrunnerも不要です．

各ステップの完全なリファレンスは [ライブラリAPI](../library.ja.md) を参照してください．

## 次へ

[T2 — ネットワーク上の意見ダイナミクス](02-opinion-network.ja.md)：エージェントにソーシャルグラフを与え，自作の代わりに同梱のメカニズムとメトリクスを **再利用** します．
