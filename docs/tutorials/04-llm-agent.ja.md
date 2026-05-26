[English](04-llm-agent.md) | **日本語**

# T4 — LLM駆動エージェント

**作るもの：** エージェントが言語モデルで意思決定する小さな噂モデル — `socsim-llm` のハーネスとスクリプト化モックを使い，完全に決定論的かつ **ネットワーク不要** で実行可能に保ちます．
**所要時間：** 45分．

## 前提

- [T1 — 最初のモデル](01-first-model.ja.md)（`WorldState`，`Mechanism`，`run_observed`，シード）．
- **任意**，ライブパスのみ：稼働中の [Ollama](https://ollama.com)（またはOpenAIキー）．実行可能なチュートリアル本体にはどちらも不要です．

裏付けの実例（**デフォルトフィーチャ** でCIコンパイル済み）：[`crates/socsim-llm/examples/tutorial_llm_agent.rs`](../../crates/socsim-llm/examples/tutorial_llm_agent.rs)．このページと並べて開いてください．

## 決定論の契約

socsimのコアは決定論的です：同じシード → 同じ軌跡，ビット単位で一致．LLMは本質的に *非* 決定論的なので，socsimは1つのフェーズに閉じ込め，**2層** で擬似決定論化します：

1. **エンジンシード**（通常の `SimRng`），そして
2. **LLM層**：`temperature = 0` ＋ 固定の生成シード ＋ ウォームキャッシュで同一応答を再生する **プロンプト → 応答キャッシュ**．

`socsim-llm` はこれをパッケージ化しているので，クライアントの配線を手書きする必要はありません．[ライブラリAPIのLLMの節](../library.ja.md#ライブラリモードでの-llm-エージェントと結果出力) を参照してください．

## ステップ

### 1. モデル

直線上の5エージェントがそれぞれ1語の信念（`"rumor"` または `"calm"`）を持ち，エージェント0だけが噂を持って始まります．各ステップ，まだ落ち着いている各エージェントは，既に何人の近傍が広めているかを踏まえ，広め始めるかをモデルに尋ねます．

```rust
struct GossipWorld {
    clock: SimClock,
    /// Per-agent belief, indexed by `AgentId.0 as usize`: "rumor" or "calm".
    beliefs: Vec<String>,
}
```

### 2. LLM呼び出しを `Decision` メカニズムに閉じ込める

`LlmClient::complete` は同期的なので，`apply` にそのまま入ります．メカニズムはクライアントと `MetadataCollector`（各呼び出しが何と話したかを記録）を *所有* します．LLMはすべてここ，`Phase::Decision` に存在します：

```rust
struct GossipDecision {
    client: LiveClient,
    settings: LlmSettings,
    collector: MetadataCollector,
}

impl Mechanism<GossipWorld> for GossipDecision {
    fn name(&self) -> &str { "gossip_decision" }
    fn phases(&self) -> &'static [Phase] { &[Phase::Decision] }

    fn apply(&mut self, _phase: Phase, ctx: &mut StepContext<'_, GossipWorld>) -> Result<()> {
        let cfg = llm_config(&self.settings);
        let prev = ctx.world.beliefs.clone();   // snapshot: synchronous update
        // ... build a prompt per calm agent, call the model, collect decisions ...
    }
}
```

`llm_config(&settings)` は `LlmConfig::deterministic()`（temperature 0，seed 0）に設定の `temperature` と `seed` を適用した `LlmConfig` を作ります — LLM決定論層の前半です．

### 3. 1回のLLM呼び出し

まだ落ち着いている各エージェントについて，プロンプトを作り，モデルを1回呼び，メタデータを記録します．エラー変換に注目：LLM層は独自の `LlmError` を持つので，この境界でエンジンの `SocsimError` へ変換します：

```rust
let resp = self
    .client
    .complete(&prompt, &cfg)
    .map_err(|e| socsim_core::SocsimError::Mechanism(e.to_string()))?;
self.collector.record(resp.metadata);
decisions.push((id, resp.text.trim().to_string()));
```

`self.client` は `LiveClient` = `CachingClient<Box<dyn LlmClient>>` です．`complete` が `&mut self` を取るのは，キャッシュ **ミス** が新しい応答をキャッシュに書くからです．**ヒット** はバックエンドに触れず再生します — これが決定論層の後半です．

### 4. `wrap_client` + `ScriptedClient` で実行可能（ネットワーク不要）なクライアントを配線する

ここが要点です．同じ `LiveClient` 型が，本番とテストの2通りで生成される — そのためメカニズムのコードは変わりません．実行可能パスは `ScriptedClient`（クロージャで定義する決定論的なインメモリ「モデル」）を `wrap_client` でキャッシュに包んで使います：

```rust
let backend = ScriptedClient::new("gossip-mock", |prompt: &str| {
    // The scripted "model": adopt the rumor once *any* neighbour is spreading.
    let m: usize = extract_after(prompt, "neighbours, ", " are spreading");
    if m >= 1 { "rumor".to_string() } else { "calm".to_string() }
});
let client: LiveClient = wrap_client(backend, PromptCache::in_memory());
```

**ライブ** パスも示しますが，デフォルトビルドがネットワークを一切引き込まないよう `live` フィーチャでゲートします：

```rust
#[cfg(feature = "live")]
let _live: socsim_llm::LiveClient =
    socsim_llm::build_live_client_from_settings(&settings).expect("live client");
```

後で `--features live` を有効にしてクライアントに `_live` を使えば，実モデルに切り替えられます — モデルの残りは同一です．設定はLLM層の2つのつまみとキャッシュ位置を運びます：

```rust
let settings = LlmSettings {
    temperature: 0.0,
    seed: 42,
    cache_path: None, // in-memory cache for this demo
};
```

### 5. いつもどおりビルドして実行する

LLMメカニズムも単なるメカニズムなので，組み立ては通常のライブラリモードです（**エンジン** シード — 第1の決定論層 — は設定内のLLMシードとは別物である点に注意）：

```rust
let mut sim = SimulationBuilder::new(world)
    .seed(7) // engine seed — the FIRST determinism layer
    .add_mechanism(Box::new(GossipDecision { client, settings, collector: MetadataCollector::new() }))
    .build();

sim.run_observed(|report| { /* print beliefs + spreading count */ })
   .expect("simulation completed");
```

## 実行する

デフォルトフィーチャ — Ollama不要，ネットワーク不要，決定論的：

```sh
cargo run -p socsim-llm --example tutorial_llm_agent
```

```
=== socsim tutorial_llm_agent (LLM-driven gossip on a line) ===
5 agents; only agent 0 starts with the rumor.

  t   beliefs                          spreading
  ------------------------------------------------
   1   ["rumor", "rumor", "calm", "calm", "calm"]  2
   2   ["rumor", "rumor", "rumor", "calm", "calm"]  3
   3   ["rumor", "rumor", "rumor", "rumor", "calm"]  4
   4   ["rumor", "rumor", "rumor", "rumor", "rumor"]  5

The rumor reached every agent at t = 4.
```

噂は1ステップに1ホップずつ伝播し，コンセンサスでモデルは自身を停止します．もう一度実行しても出力は同一です — エンジンシード＋スクリプト化モデル＋ウォームキャッシュが決定論的にするからです．この決定論こそが **LLMモデルをユニットテスト** できる理由です：クロージャをテストオラクルに差し替え，軌跡をアサートしてください．

ライブパスがまだコンパイルできることを確認するには（実行しない限り外部呼び出しはしません）：

```sh
cargo build -p socsim-llm --all-features --example tutorial_llm_agent
```

## 学んだこと

- LLMを **1つの `Decision` メカニズム** に閉じ込める．`complete` は同期的で `apply` に収まります．
- `socsim-llm` の **ハーネス** は，本番（`build_live_client_from_settings`，`live` の背後）とテスト（`wrap_client` + `mock::ScriptedClient`）の両方に1つの `LiveClient` 型を与えます — モデルのコードはどちらでも同一です．
- **2層の決定論**：エンジンシード *に加えて* LLM層（`temperature = 0`，固定シード，応答を再生する `PromptCache`）．
- `MetadataCollector` は各呼び出しが何と話したか（モデル / エンドポイント / キャッシュヒット）を，実行の来歴として記録します．
- メカニズムの境界で `LlmError` を `SocsimError` へ変換します．

`build_live_client`，`RunMetadata`，`llm_meta.json` サイドカーの永続化については [ライブラリAPIのLLMの節](../library.ja.md#ライブラリモードでの-llm-エージェントと結果出力) を参照してください．

## 次へ

[T5 — シナリオパック](05-scenario-pack.ja.md)：メカニズムを `ModulePack` にまとめ，シナリオTOMLとCLIから駆動します．
