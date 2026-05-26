[English](03-spatial-grid.md) | **日本語**

# T3 — 空間グリッドモデル

**作るもの：** トーラス格子上のイベント駆動ボーターセルオートマトン — 1ティックに多数のマイクロイベント，O(1)の近傍探索，コンセンサスでの停止 — に加えて，グリッドから空間メトリクスを読む方法．
**所要時間：** 40分．

## 前提

- [T1 — 最初のモデル](01-first-model.ja.md)（`WorldState`，`Mechanism`，`run_observed`，シード）．
- T2は不要です（これはネットワークモデルの格子版の兄弟です）．

裏付けの実例（CIコンパイル済み）：[`crates/socsim-engine/examples/cellular_automata.rs`](../../crates/socsim-engine/examples/cellular_automata.rs)．このページと並べて開いてください．

## ステップ

### 1. 格子ワールド：`CellGrid` + 事前計算した `Adjacency`

`socsim-grid` は2D空間を提供します．ここで重要なのは2つ：`CellGrid<T>` は **すべての** セルに値 `T`（ここでは `u8` の意見）を格納し，`Adjacency` は一度だけ構築する **事前計算済み** の近傍テーブルで，ホットな毎ステップループがO(1)探索を確保なしで行えるようにします：

```rust
struct VoterWorld {
    clock: SimClock,
    /// Per-cell opinion, row-major over the grid.
    cells: CellGrid<u8>,
    /// Precomputed CSR neighbour table (flat row-major indices).
    adjacency: Adjacency,
}

impl VoterWorld {
    fn new(rows: usize, cols: usize, n_opinions: u8, rng: &mut socsim_core::SimRng) -> Self {
        let grid = Grid::new(rows, cols, Boundary::Toroidal);
        // Precompute the Moore (8-neighbour) adjacency once; reused every tick.
        let adjacency = grid.adjacency(Neighborhood::Moore);
        let cells = CellGrid::from_fn(grid, |_r, _c| rng.gen_range(0..n_opinions));
        Self { clock: SimClock::new(0), cells, adjacency }
    }
}
```

`Boundary::Toroidal` は格子を巻き込みにします（端なし），`Neighborhood::Moore` は8セル近傍です．`adjacency` を最初に一度だけ構築するのが鍵となる空間イディオムです — [ライブラリAPI，非確保の近傍クエリ](../library.ja.md#アロケーションを伴わない近傍クエリ) を参照．

`CellGrid` ワールドにはエージェント名簿がありません — 「エージェント」はセルで，1つのメカニズムから一括で駆動します — そのため `agent_ids` は空を返します：

```rust
impl WorldState for VoterWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        Vec::new()
    }
    // clock / clock_mut as usual
}
```

### 2. 1ティックに多数のマイクロイベント

ボーターモデルに自然な「ステップ」はありません：単一セルの更新（セルを選び，ランダムな近傍の意見をコピー）を撃ち続けるだけです．イディオムは，各イベントに独自のステップを与えるのではなく，**多数のマイクロイベントを1つのエンジンティックにまとめる** ことです：

```rust
fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, VoterWorld>) -> Result<()> {
    let n = ctx.world.cells.len();
    if n == 0 { return Ok(()); }

    // Batch of micro-events, all driven by ctx.rng for reproducibility.
    for _ in 0..self.events_per_step {
        let idx = ctx.rng.gen_range(0..n);
        let nbrs = ctx.world.adjacency.neighbors(idx);   // O(1) borrowed &[usize]
        if nbrs.is_empty() { continue; }
        let nbr = nbrs[ctx.rng.gen_range(0..nbrs.len())];
        let opinion = *ctx.world.cells.get_idx(nbr).expect("in-range");
        if let Some(cell) = ctx.world.cells.get_idx_mut(idx) { *cell = opinion; }
    }

    if ctx.world.distinct_opinions() <= 1 { ctx.request_stop(); }  // consensus
    Ok(())
}
```

2点に注目．`ctx.rng` がセル選びと近傍選びの両方を駆動するので，実行全体がシードから再現可能です．そして `adjacency.neighbors(idx)` は借用スライスを返します — 1ティックに数百のイベントを撃つとき重要な，イベントごとの確保ゼロです．メカニズムは `Phase::Interaction` で実行され，格子が一様になった（吸収状態）時点でエンジンに停止を求めます．

### 3. `run_observed` でステップごとのメトリクスを観測する

`run_observed` は実行されたステップごとに，そのステップ *後* の状態を反映した `StepReport` を伴ってクロージャを呼びます — `step()` ループを手で書かずに収束曲線を集める，使い勝手のよい方法です：

```rust
sim.run_observed(|report| {
    let distinct = report.world.distinct_opinions();
    if report.t <= 5 || report.t % 10 == 0 || report.stopped {
        println!("  {:>3}  {}", report.t, distinct);
    }
})
.expect("simulation completed");
```

ここでの `distinct_opinions()` は小さなローカルメトリクス（異なるセル値の数）です．**ラベル付きの空間構造** — 「格子はどれだけ分離しているか」 — には，`socsim-metrics` が `Grid` からラベルアクセサクロージャ経由で直接読める，既製の空間メトリクスを同梱しています：

```rust,ignore
use socsim_metrics::spatial::{local_similarity, segregation_index};
use socsim_grid::Neighborhood;

// label(r, c) -> Some(category) for an occupied cell, None for vacant.
let s = segregation_index(&grid, Neighborhood::Moore, |r, c| Some(cells.get(r, c)?.clone()));
// `s` → 1.0 under perfect segregation; near the population share under a random layout.
```

`segregation_index` は，占有された全セルにわたる `local_similarity`（同種近傍の割合）の平均 — 標準的なシェリング指標です．純粋な読み取り専用関数なので，実行を乱しません．（ボーターの実例は *コンセンサス* を追跡し *分離* ではないので，より単純な `distinct_opinions` を使います．セルがカテゴリラベルを持ち空間的選別が気になるときは `segregation_index` に差し替えてください．）

### 4. シードと安全上限

実例はT2と同じ `&[0]` world-init / `&[1]` engine 分割を使い，コンセンサスが通常上回る `t_max` 安全上限（`500`）を設定します：

```rust
let root = 7u64;
let mut init_rng = socsim_core::SimRng::from_seed(socsim_core::derive_seed(root, &[0]));
let mut world = VoterWorld::new(16, 16, 4, &mut init_rng);
world.clock = SimClock::new(500);

let mut sim = SimulationBuilder::new(world)
    .seed(socsim_core::derive_seed(root, &[1]))
    .add_mechanism(Box::new(VoterModel { events_per_step: 256 }))
    .build();
```

## 実行する

```sh
cargo run -p socsim-engine --example cellular_automata
```

```
=== socsim cellular_automata (voter model) ===
16x16 toroidal lattice, 4 opinions, 256 events/tick

  t   distinct opinions
  ---------------------
    1  4
   50  3
  110  2
  ...
  390  2
  396  1

reached consensus at t = 396 (distinct = 1)
```

4つの初期意見が1つに崩壊し（コンセンサス），`500` の上限を上回る `t = 396` で停止します．同じシード → 毎回同じ軌跡です．

## 学んだこと

- `socsim-grid` は2D空間を提供します：各セル状態には `CellGrid<T>`，**事前計算済み** のO(1)近傍テーブルには `Adjacency` — 一度作って毎ティック再利用します．
- **イベントを1ティックにまとめる** イディオムは，非同期なイベントモデルをエンジンの離散ループに写します．`ctx.rng` が全ランダム選択を駆動するので，実行は再現可能なままです．
- `run_observed` はきれいなステップごとの観測フック（`StepReport`）を与えます．
- `socsim-metrics::spatial`（`segregation_index`，`local_similarity`）はラベルクロージャ経由で `Grid` から空間構造を読みます — 純粋で再現性に安全なメトリクスです．

`socsim-grid` の全API面は [ライブラリAPIのグリッドの節](../library.ja.md#socsim-grid-による空間モデル) を参照してください．

## 次へ

[T4 — LLM駆動エージェント](04-llm-agent.ja.md)：実行を決定論的に保ちつつ，1つのフェーズの中に言語モデルを置きます．
