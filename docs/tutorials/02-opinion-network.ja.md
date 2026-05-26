[English](02-opinion-network.md) | **日本語**

# T2 — ネットワーク上の意見ダイナミクス

**作るもの：** スモールワールドのソーシャルグラフ上の有界信頼意見モデル — まず仕組みを見るために手で書き，次に自作の代わりに同梱の `socsim-mechanisms` と `socsim-metrics` を **再利用** して作り直します．
**所要時間：** 40分．

## 前提

- [T1 — 最初のモデル](01-first-model.ja.md)（`WorldState`，`Mechanism`，`StepContext`，シード）．
- ソーシャルグラフ（ノード＋エッジ）の概念に親しんでいること．

このチュートリアルには，どちらもCIでコンパイルされる2つの裏付け成果物があります：

- スクラッチ版：[`crates/socsim-engine/examples/opinion_dynamics.rs`](../../crates/socsim-engine/examples/opinion_dynamics.rs)；
- 再利用版：[`crates/socsim-packs/src/opinion.rs`](../../crates/socsim-packs/src/opinion.rs) の `opinion-dynamics` パック（T0でCLIから実行済み）．

## パートA — `socsim-net` を使ってゼロから

### 1. グラフを保持するワールド

`socsim-net` は `AgentId` をキーとするグラフと再現可能なジェネレータを提供するので，Watts–Strogatzや近傍探索を再実装する必要はありません．これを `WorldState` に，各エージェントの意見と並べて保持します：

```rust
struct OpinionWorld {
    clock: SimClock,
    net: SocialNetwork,
    /// Opinion of each agent, indexed by `AgentId.0 as usize`.
    opinions: Vec<f64>,
    /// Largest single-step opinion change in the previous step (convergence).
    last_max_delta: f64,
}

impl OpinionWorld {
    fn new(n: usize, k: usize, beta: f64, init_rng: &mut SimRng) -> Self {
        let ids: Vec<AgentId> = (0..n as u64).map(AgentId).collect();
        // The network is built from the *world-init* stream, not the engine's.
        let net = SocialNetwork::watts_strogatz(&ids, k, beta, init_rng);
        let opinions: Vec<f64> = (0..n).map(|_| init_rng.gen::<f64>()).collect();
        Self { clock: SimClock::new(u64::MAX), net, opinions, last_max_delta: f64::INFINITY }
    }
}
```

RNGのコメントに注目：グラフと初期意見は，エンジンのストリームとは別の **world-init** ストリームから引かれます（後述）．

### 2. 有界信頼の更新

各ステップ，すべてのエージェントは，まだ *信頼する* 近傍 — 信頼半径 `epsilon` 内のもの — の平均意見へ向けて割合 `mu` だけ移動します．更新は **同期的** です：全員の現在の意見を読み，その後で新しい意見を書きます：

```rust
fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, OpinionWorld>) -> Result<()> {
    let n = ctx.world.opinions.len();
    let current = ctx.world.opinions.clone();   // read old
    let mut next = current.clone();             // write new
    let mut buf: Vec<AgentId> = Vec::new();     // reused across agents — no per-agent alloc
    let mut max_delta = 0.0_f64;

    for i in 0..n {
        let xi = current[i];
        let mut sum = xi;                       // an agent always trusts itself
        let mut count = 1usize;

        ctx.world.net.neighbors_into(AgentId(i as u64), &mut buf); // zero-alloc neighbour read
        for &AgentId(j) in &buf {
            let xj = current[j as usize];
            if (xj - xi).abs() <= self.epsilon { sum += xj; count += 1; }
        }
        let mean = sum / count as f64;
        next[i] = xi + self.mu * (mean - xi);
        max_delta = max_delta.max((next[i] - xi).abs());
    }

    ctx.world.opinions = next;
    ctx.world.last_max_delta = max_delta;
    if max_delta < self.tol { ctx.request_stop(); }  // opinions stopped moving ⇒ converged
    Ok(())
}
```

`neighbors_into(id, &mut buf)` は近傍を，エージェント間で再利用するバッファに読み込みます — ホットループでの各エージェントごとのヒープ確保がありません．このメカニズムは `Phase::Interaction` で実行されます（近傍の影響は相互作用です）．

### 3. 1つのシードから2つのRNGストリーム

実例は1つのルートシードから2つの **ラベル付き** 子ストリームを導出し，world-initのランダム性（グラフ＋意見）をエンジンのスケジューラストリームと無相関にします：

```rust
let root = 7u64;
let mut init_rng = SimRng::from_seed(socsim_core::derive_seed(root, &[0])); // [0] = world init
let world = OpinionWorld::new(200, 6, 0.1, &mut init_rng);

let mut sim = SimulationBuilder::new(world)
    .seed(socsim_core::derive_seed(root, &[1]))                              // [1] = engine
    .add_mechanism(Box::new(BoundedConfidence { epsilon: 0.2, mu: 0.5, tol: 1e-4 }))
    .build();
```

この `&[0]` = world / `&[1]` = engine 規約はsocsimのモデル全体で繰り返し現れます．実例は `connected_components()` と `average_clustering_coefficient()` を使った1行のトポロジーサマリも出力します — どちらも `socsim-net` が無料で提供する解析ヘルパです．

### パートAを実行する

```sh
cargo run -p socsim-engine --example opinion_dynamics
```

```
=== socsim opinion_dynamics (bounded-confidence DeGroot on a graph) ===
200 agents, Watts–Strogatz(k=6, beta=0.1): 1 component(s), avg clustering 0.447

  t   clusters   max-delta
  ----------------------------
    1     16      0.06395
    5     13      0.04117
  ...
  270      8      0.00011
  275      8      0.00010

Converged after 275 steps into 8 opinion cluster(s).
```

異なる意見クラスタが減り `max-delta` が `tol` を下回ると，メカニズムは停止を要求します．

## パートB — 自分で書かない：`socsim-mechanisms` + `socsim-metrics` を再利用する

上で手書きした `BoundedConfidence` は，`socsim-mechanisms` クレートが既に `HegselmannKrauseMechanism` として同梱しているものそのものです（意見と近傍を公開できる任意のワールドに対してジェネリック）．T0で実行した `opinion-dynamics` パックはこの方式で作られています．汎用メカニズムが *あなたの* ワールドを駆動できるようにする契約が，2つの小さな **能力トレイト** です — `crates/socsim-packs/src/opinion.rs` より：

```rust
impl ScalarOpinions for OpinionWorld {
    fn opinion(&self, id: AgentId) -> f64 { self.opinions[id.0 as usize] }
    fn set_opinion(&mut self, id: AgentId, value: f64) { self.opinions[id.0 as usize] = value; }
}

impl Neighbors for OpinionWorld {
    fn neighbors_of(&self, id: AgentId) -> Vec<AgentId> { self.net.neighbors(id) }
}
```

この2つのトレイトを実装すれば，更新の数式を書かずにクレートの *どの* 意見メカニズム（`HegselmannKrauseMechanism`，`DeffuantMechanism`，`SocialJudgementMechanism`，`LorenzMechanism`）も差し込めます．パックはHKを構築するだけで登録します：

```rust
reg.register("hegselmann_krause", |p: &Params| {
    let epsilon = p.get_f64("epsilon", 0.2);
    let p_fallback = p.get_f64("p", 1.0);
    let mean = parse_mean(p.get_str("mean", "A"), p_fallback)
        .map_err(socsim_core::SocsimError::Config)?;
    Ok(Box::new(HegselmannKrauseMechanism::new(epsilon, mean))
        as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
});
```

### メトリクスも再利用する

同じ考え方が観測にも当てはまります．平均 / 分散 / スプレッド / クラスタリングを再実装する代わりに，パックの `OpinionMetricsMechanism` は標準統計量を `socsim-metrics` に委譲します：

```rust
let mean = socsim_metrics::stats::mean(&curr);
let variance = socsim_metrics::stats::variance(&curr);
let spread = socsim_metrics::stats::spread(&curr);
let clusters = socsim_metrics::stats::distinct_clusters(&curr, self.tol) as f64;
```

`socsim-metrics` は純粋でライブラリ専用の観測層です（RNGなし，状態変更なし）．そのため再利用してもモデルの再現性は決して変わりません．`ScalarOpinions` を実装したワールド向けには，宣言的な `MetricsMechanism` まで提供され，メカニズムを書かずに名前付きメトリクスの集合を記録できます — [ライブラリAPIのメトリクスの節](../library.ja.md#socsim-metrics-による再利用可能なメトリクス) を参照してください．

### パートBを実行する（CLI経由）

パックは `socsim` バイナリに組み込まれているので，シナリオとして実行します（Rust不要）：

```sh
socsim run scenarios/opinion_dynamics_baseline.toml
```

```
Running 'opinion_dynamics_baseline' (pack=opinion-dynamics, t_max=60, seeds=[42], parallel=false)

t               clusters         max_delta              mean            spread          variance
10               22.0000            0.1238            0.5092            0.9769            0.0360
30               15.0000            0.0127            0.5094            0.9769            0.0243
60               12.0000            0.0010            0.5098            0.9769            0.0232
```

パートAと同じ物理ですが，更新の数式も統計量もすべて共有クレート由来です．こうしたパックを自分で作る方法はT5で示します．

## 学んだこと

- `socsim-net` は再現可能なグラフジェネレータ（`watts_strogatz` など）とゼロ確保の近傍読み出し（`neighbors_into`），そしてトポロジーメトリクス（`connected_components`，`average_clustering_coefficient`）を提供します．
- `&[0]` world-init / `&[1]` engine のRNG規約は，1つのシードから再現可能なまま2ストリームを無相関に保ちます．
- **能力トレイト** `ScalarOpinions` + `Neighbors` により，汎用 `socsim-mechanisms`（HK，Deffuant…）があなたのワールドを駆動できます — 2メソッドを実装すれば更新の数式を再利用できます．
- `socsim-metrics` は標準統計量を純粋な観測層として提供します．mean/variance/clustersを再実装せず再利用しましょう．

再利用可能な全メカニズムは [Mechanismカタログ](../mechanisms.ja.md) を，`socsim-net` の全API面は [ライブラリAPI](../library.ja.md#socsim-net-によるネットワークモデル) を参照してください．

## 次へ

[T3 — 空間グリッドモデル](03-spatial-grid.ja.md)：ネットワークを格子に置き換え，イベント駆動のセルオートマトンを実行します．
