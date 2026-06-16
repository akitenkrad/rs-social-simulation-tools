[English](architecture.md) | **日本語**

# アーキテクチャ

---

## クレートワークスペース

ワークスペースは3層に整理された17個のクレートで構成されています：

![Crate dependency graph](assets/arch-crates.svg)

```
socsim-cli          ← バイナリ（エントリーポイント）
    └── socsim-runner      ← マルチシード実行，スイープ，サマリー
            ├── socsim-engine      ← Simulation, SimulationBuilder, スケジューラー
            │       └── socsim-log         ← InMemoryRecorder, JsonlRecorder, CsvRecorder
            ├── socsim-config      ← Params, Registry, ModulePack, Scenarioローダー
            │       └── socsim-core        ← トレイト (Mechanism, WorldState, …), AgentId, Phase, Blackboard
            ├── socsim-packs        ← バンドルされた CLI pack: hr-lifecycle（10メカニズム）＋ opinion-dynamics World ＋ organizational-silence（10メカニズム＋オプションの LLM voice_decision）；`pack-hr-lifecycle` / `pack-opinion-dynamics` / `pack-organizational-silence`（いずれも既定で有効）の背後にある CliPack 群．`pack-organizational-silence-llm` はオプトイン
            │       ├── socsim-net         ← SocialNetwork（ER, WS, BAジェネレーター）
            │       └── socsim-mechanisms  ← opinion pack が使う意見ダイナミクスメカニズム（HK, Deffuant, …）
            ├── socsim-grid        ← Grid, GridIndex, 近傍, 距離（空間モデル）
            ├── socsim-marl        ← 学習ポリシー(MARL): Policy, PolicyMechanism, MarlTrainer（burn; ライブラリ専用）
            └── socsim-rng         ← SimRng (ChaCha20), derive_seed

socsim-mechanisms ← 汎用の社会ダイナミクスクレート: HegselmannKrauseMechanism, DeffuantMechanism, SocialJudgementMechanism, LorenzMechanism, SiContagionMechanism, ThresholdContagionMechanism, PerAgentThresholdContagionMechanism, AxelrodMechanism, GroupConformityMechanism, MeanOperator（→ socsim-core のみ; ライブラリ専用）
socsim-llm      ← オプションのLLMエージェント層: LlmClient, CachingClient, SharedCachingClient, build_live_client, complete_with_logprobs + TokenLogprob（logprob 公開）, extract_first_choice（自由文 → 選択肢）; LlmConfig のオプトインな生成忠実度設定（system / omit_seed / allow_blank / top_logprobs） —— 空応答は既定で拒否; LlmError::EmptyResponse（socsim 依存なし; feature ゲート; ライブラリ専用）
socsim-results  ← リーフの出力ヘルパ: timestamp, create_run_dir, write_csv/json, refresh_latest_symlink（socsim 依存なし; ライブラリ専用）
socsim-survey   ← 設定駆動のサーベイマイクロデータ recode: SurveySchema（人口統計ごとの valmap ＋ アウトカムマップ ＋ 年齢ビン），ANES 2012/2016/2020 の組み込みスキーマ，CES 拡張ポイント; recode_row / demo_label / actual_outcome / estimate_distributions（socsim 依存なし; エンジン非依存; ライブラリ専用）
socsim-reproduce ← 論文アンカーの PASS/off 再現ハーネス: Anchor / AnchorStatus / compare_anchor / build_rows / write_reproduce_summary / write_paper_anchors / find_latest; 呼び出し側が自前の &[Anchor] ＋ 観測クロージャを供給（→ CSV I/O のため socsim-results に依存; エンジン非依存; ライブラリ専用）
socsim-metrics  ← feature ゲートされた観測メトリクス: 依存ゼロの `stats` コア（mean/variance/gini/entropy/hhi/clusters/bimodality/polarization/deltas）＋ 依存ゼロの `distribution`（KL ダイバージェンス / カイ二乗均質性 ＋ Wasserstein/NEMD/MD/SDD の順序分布距離）＋ 依存ゼロの `agreement`（tetrachoric / Cohen の κ / ICC / Cramér の V / prop-test），すべて常にコンパイル ＋ オプションの `core`（意見抽出子＋MetricsMechanism<W> → socsim-core），`network`（次数/クラスタリング/連結成分/カスケード → socsim-net），`spatial`（Schelling 分離度 → socsim-grid）アダプタ；読み取り専用/派生量；ライブラリ専用
```

依存ルール：

- `socsim-core` と `socsim-rng` は**内部依存なし** — これらが基盤です．
- `socsim-config` は `socsim-core` に依存しますが，循環を避けるため `socsim-engine` には**依存しません**．
- `socsim-engine` は `socsim-core`，`socsim-log`，`socsim-config` に依存します．
- `socsim-runner` は上記すべてに依存し，並列処理のために `rayon` を追加します．
- `socsim-cli` はすべてを `socsim` バイナリとして結合します．これは **World 多態**です：各コマンドハンドラは，オブジェクトセーフで World を消去した `CliPack` トレイト（`name` / `starter_toml` / `mechanism_names` / `run_seeds` / `run_sweep`，いずれも World 非依存の `RunResult` / `SweepPoint` を返す）を介して動作し，登録された各 pack が自身の World 型に対して汎用の `socsim-runner` 関数を内部で monomorphize します．したがってバイナリは具体的な World 型を**一切名指しせず**，pack はレジストリから名前で引かれます．バンドルされた World 群は今や **`socsim-packs`**（hr-lifecycle，opinion-dynamics，organizational-silence の各 pack をまとめたクレート；各 pack は `pack-hr-lifecycle` / `pack-opinion-dynamics` / `pack-organizational-silence` の `optional` 依存でゲートされる `CliPack`．organizational-silence はオプトインの `pack-organizational-silence-llm` フィーチャで LLM 駆動の voice 決定メカニズムを追加できる）に置かれ，CLI 自体には含まれません — 追加の pack は run/sweep/validate/list パイプラインに手を入れずにその隣へ差し込めます．
- `socsim-packs`，`socsim-net`，`socsim-grid` はエンジン層の隣に位置し，直交しています；`socsim-grid` は `socsim-core` にのみ依存します．`socsim-packs` は `socsim-net`（HR／意見／沈黙ネットワーク）と `socsim-mechanisms`（意見ダイナミクスメカニズム）に依存します．`pack-organizational-silence-llm` の下では `socsim-llm` にも依存し，LLM 駆動の voice 決定メカニズムを取り込みます．パックごとのドキュメントは[モジュールパックカタログ](packs.ja.md)を参照してください．
- `socsim-marl`（Phase 6）は `socsim-engine` と `socsim-core` に依存します．**ライブラリ専用**（`socsim` バイナリには含まれません）で，ニューラルネットフレームワーク `burn` を取り込むため，`socsim-packs` の hr-lifecycle 連携は `marl` feature でゲートしています．
- `socsim-llm` はエンジン層の隣に位置する**直交した，オプションの**層です．**`socsim-*` 依存はなく**（`serde`/`serde_json`/`thiserror` のみ，加えて feature 越しの `ureq`），**ライブラリ専用**です．ライブのプロバイダバックエンドは feature ゲート（`ollama`，`openai`，および両者をまとめた `live`）されており，デフォルトビルドはネットワーク依存を一切取り込みません．LLM 駆動モデルの `Decision` フェーズで使用します．素の `complete` に加え，`complete_with_logprobs` / `TokenLogprob` / `LlmResponse.logprobs` でトークンレベルの logprob を公開します（デフォルト実装は `LlmError::Unsupported`，Ollama/OpenAI バックエンドが上書き）．また `LlmConfig` にオプトインの生成忠実度設定（`system` プロンプト，`omit_seed`，`allow_blank`，`top_logprobs`）を備え，その既定値は従来の挙動を保ちます —— 空応答は依然**既定で拒否**され，`allow_blank` でオプトアウト可能になりました．`SharedCachingClient` は内部可変なキャッシュで，それ自体が `LlmClient` を `impl` するため，キャッシュ付きクライアントを `&dyn LlmClient` として注入できます（`wrap_client_shared` / `build_shared_live_client[_from_settings]`）．
- `socsim-results` は**リーフクレート**で，**`socsim-*` 依存はなく**（`std` に加えて `serde`/`serde_json`/`csv`/`chrono` のみ），軽量ライブラリモード向けの出力ボイラープレートを提供します．`socsim-log`/`-config`/`-runner` を一切取り込みません．
- `socsim-survey` は**リーフかつエンジン非依存**のクレートで（`csv`/`serde` のみ，**`socsim-*` 依存なし**），**ライブラリ専用**です．`sun2024` replication で年ごとにハードコードされていた ANES recode を，データ駆動の `SurveySchema`（人口統計ごとの列＋値→ラベル valmap，アウトカムマップ，年齢ビン）へ一般化し，ANES 2012/2016/2020 の組み込みスキーマと CES 拡張ポイント，さらに汎用の `recode_row` / `demo_label` / `actual_outcome` / `estimate_distributions` ヘルパを提供します．
- `socsim-reproduce` は**ライブラリ専用かつエンジン非依存**のクレートで，**`socsim-results` のみ**に依存します（CSV I/O のための一方向依存）．論文アンカーの PASS/off 再現ハーネスで，*仕組み*（`Anchor` / `AnchorStatus` / `compare_anchor` / `build_rows` / `write_reproduce_summary` / `write_paper_anchors` / `find_latest`）は備える一方，アンカー値は**一切持ちません** —— 各呼び出し側が自前の `&[Anchor]` スライスと観測ルックアップのクロージャを供給するため，再現実行は生成を再実行せずキャッシュ済みの観測を読み直し，論文の参照値に対して分類します．
- `socsim-mechanisms` はエンジン層の隣に位置する**直交した，オプションの**クレートです．**`socsim-core` のみ**に依存し（`ScalarOpinions` / `BinaryState` / `CultureVectors` / `Neighbors` / `ActivationThreshold` 能力トレイトのため），**ライブラリ専用**です — `ModulePack` を持たず，`socsim` バイナリには組み込まれません．これは**汎用メカニズムカタログ**です：再利用可能でドメイン非依存な構成要素を，4つの Cargo **フィーチャーファミリー**（既定で全て有効 — `opinion-dynamics`，`contagion`，`cultural`，`group-dynamics`）に整理し，合計8つのメカニズムを提供します：意見ダイナミクス（有界信頼の `HegselmannKrauseMechanism` と `DeffuantMechanism`，`SocialJudgementMechanism`，`LorenzMechanism`，および A/G/H/P/R の `MeanOperator` ファミリー），ネットワーク伝播（`SiContagionMechanism` と `ThresholdContagionMechanism` — 後者にはエージェントごとの閾値を用いる `PerAgentThresholdContagionMechanism` バリアントがあります），文化伝播（`AxelrodMechanism`），グループダイナミクス（`GroupConformityMechanism`） — を提供し，`socsim-packs` クレートにバンドルされたシナリオ固有の pack 群（その opinion-dynamics pack が本クレートに依存します）とは区別されます．
- `socsim-metrics` は `socsim-results` / `socsim-llm` の隣に位置する**ほぼリーフの，feature ゲートされた**クレートです．常にコンパイルされる `stats`，`distribution`，`agreement` モジュールは**依存なし**（`&[f64]`/`&[u32]` 上の純粋な数値プリミティブ；`distribution` は KL ダイバージェンスと Pearson のカイ二乗均質性 —— 後者の p 値は統計クレートではなく手書きの正則化上側不完全ガンマ関数で算出 —— に加え，順序分布マッチングの距離 `wasserstein_1d` / `nemd`（NEMD）/ `mean_diff`（MD）/ `sd_diff`（SDD）を追加；依存ゼロの `agreement` モジュールはクロス表の一致度統計 —— `tetrachoric`，`cohen_kappa`，`icc` / `average_icc`，`cramers_v`，`prop_agree`，`prop_test`，ヘルパの `bvn_cdf` —— を追加）なので，既定の `cargo build -p socsim-metrics` は **`socsim-*` クレートを一切取り込みません**．アダプタは Cargo feature でオプトインします：`core` は意見 World 抽出子と汎用の `MetricsMechanism<W>` を追加（→ `socsim-core`），`network` は次数/クラスタリング/連結成分/カスケードのメトリクスを追加（→ `socsim-net`，`core` を含意），`spatial` は Schelling 流の分離度メトリクスを追加（→ `socsim-grid`，`core` を含意）します．**ライブラリ専用**かつ**構造上読み取り専用**です：すべての関数が純粋な観測/派生量（RNG なし，World 変更なし）であり，公開する唯一のメカニズムも `PostStep` で `Recorder` に記録するだけなので，採用してもいかなるモデルの**キャリブレーションにも影響しません**．

---

## 6フェーズティックループ

各離散時間ステップは，`Phase::ORDER` で定義された固定順序で6つのフェーズを実行します：

```
PreStep → Environment → Decision → Interaction → Reward → PostStep
```

エンジンの `Simulation::step` メソッドは：

1. クロックをティック（`t += 1`）します．
2. `Scheduler` にエージェントの活性化順序を問い合わせます．
3. `Phase::ORDER` の各フェーズで，そのフェーズを登録したすべてのメカニズムを挿入順に呼び出します．

ステップ2で計算された活性化順序は `StepContext::agent_order` としてすべてのフェーズに渡され，同じステップ内のメカニズムが同じ順序を見ることを保証します．

メカニズムは `Mechanism::phases` から `'static` スライスを返すことでフェーズを登録します．登録された各フェーズで1ステップに1回呼び出されます．HRライフサイクルメカニズムの典型的なフェーズ割り当ては以下の通りです：

| メカニズム | フェーズ |
|---|---|
| `learning_curve` | Environment |
| `peer_effect` | Interaction |
| `ocb` | Interaction |
| `fit` | Decision |
| `turnover` | Decision |
| `hiring` | Decision |
| `knowledge_loss` | PostStep |
| `socialization` | PostStep |
| `toxic_spread` | Interaction |
| `org_performance` | Reward |

### イベント駆動 / サブティックモデル

固定のティックループは，socsim を1ティックあたりエージェント1アクションのモデルに制限**しません**．イベント駆動・サブティックのダイナミクス（Gillespie型反応，投票者モデル，接触過程の感染）は，単純な慣用句でサポートされます：**多数の微小イベントを1つの `Mechanism::apply` 内でバッチ処理し，それらのイベントを1ティックにマッピングする**ことです．1回の `apply()` 呼び出しが `events_per_step` 個のランダムな単一セル／エージェント更新（すべて `ctx.rng` から引く）を実行するので，エンジンのティックが観測／チェックポイントの間隔となり，イベントごとの更新セマンティクスは保たれます．モデルが吸収状態に達したとき，メカニズムは `ctx.request_stop()` を呼べます．動作する格子投票者モデルは `crates/socsim-engine/examples/cellular_automata.rs` を参照してください．

---

## 2つの利用経路：シナリオCLI vs. ライブラリモード

socsim は2通りの使い方ができ，**どちらもファーストクラス**です：

![Two usage paths: scenario-CLI vs. library mode](assets/arch-usage-paths.svg)

- **シナリオTOML / CLI経路** — `ModulePack` → `Registry` → シナリオ `.toml` → `socsim-runner` → `socsim` バイナリ．新規プロジェクト，再現可能なシナリオファイル，パラメータスイープに最適です．
- **ライブラリモード** — `socsim-core` / `socsim-engine`（および任意で `socsim-grid`）だけに依存し，ワールドを自分で構築し，メカニズムを `SimulationBuilder` に直接追加し，`run` / `run_until` / `run_observed` で駆動し，独自のレコーダーを持ち込みます（あるいは持ち込まない — デフォルトは `NullRecorder` なので，エンジンは `socsim-log` 依存を強制しません）．既存ツールへのエンジン埋め込み，カスタム出力スキーマ，自己完結型の格子／CAモデルに最適です．

2つの経路は同じエンジンと決定論性の保証を共有します；プラットフォームごとではなくプロジェクトごとに選択してください．トレードオフ表は[ライブラリガイド](library.ja.md#軽量エンジンのみの利用toml--runner-なし)を参照してください．

---

## 決定論的RNG

`socsim-rng` は `rand_chacha::ChaCha20Rng` をラップして再現可能なストリームを提供します．主なAPIは：

- `SimRng::from_seed(seed: u64)` — ルートRNGを作成します．
- `SimRng::derive(&[u64])` — ラベル（エージェントID，フェーズインデックスなど）から子RNGを派生させます（親を変更しません）．FNV-1a風のハッシュミックスを使用します．

エンジンはシナリオの `seed` フィールドからルートRNGをシードします．同じシードは，マシンアーキテクチャやRustのバージョンに関わらず，常に同じエージェント軌跡を生成します．

エージェントとチームの集計は常にソートされた `AgentId` 順で反復し，ハッシュマップの反復非決定性を排除します．

---

## スナップショット：保存と再開

シミュレーションの**可変状態**を捕捉・復元できます — PyTorch の `state_dict`（状態）と model architecture（コード）の分離に相当します．`Snapshot<W>` は World（`SimClock` を含む），`SimRng` の厳密なストリーム位置（`rand_chacha` の `serde1` でシリアライズ），early-stop フラグを保持します．mechanisms・scheduler・recorder は*コード*であり再構築側が用意するため，意図的に含めません．

- `Simulation::snapshot()` / `restore(snapshot)` — メモリ上での捕捉/復元（`snapshot()` は `W: Clone` が必要）．
- `Snapshot::save(path)` / `Snapshot::load(path)` — JSON 永続化，`SNAPSHOT_VERSION` で版チェック．

**同じ** mechanisms で構築した `Simulation` にスナップショットを復元すると，保存時点以降の実行がビット単位で再現されます — *別シード*で構築した sim に復元しても無中断実行と一致することをテストで検証しています．境界はオプトイン（`W: Serialize` / `DeserializeOwned` でゲートした `impl`）なので `WorldState` トレイトは不変で，serde 非対応の World は単にこれらのメソッドを持ちません．`SocialNetwork` は `{nodes, edges}` のペアとしてシリアライズし（petgraph の `NodeIndex` は永続化せず再構築），petgraph のバージョン差にも安定です．

---

## 学習ポリシー（MARL, Phase 6）

`socsim-marl` は `Decision` フェーズを学習可能にします：`PolicyMechanism` が `Policy`（`burn` の小さな MLP を REINFORCE で学習する `DiscretePolicyNet` が実装）をラップし，他のメカニズムと同じ6フェーズループに差し込めます — エンジンの変更は不要です．`ObsEncoder`/`ActionApplier`/`RewardFn` が具体的な World とフラットな特徴・行動空間を橋渡しし，`TrajectoryBuffer` がエピソードを収集，`MarlTrainer` が外側の学習ループを回します．重みは `SimRng` からシードされ全テンソル演算は CPU 上なので，凍結ポリシーはビット単位で再現可能です．使い方は[ライブラリガイド](library.ja.md#学習ポリシーmarl)を参照してください．

---

## LLM層（socsim-llm）

`socsim-llm` は LLM 駆動エージェント向けのオプション層です．socsim コアは**決定論的で LLM フリー**なので，このクレートはモデルの非決定性を1箇所に閉じ込め，*疑似決定論化*します — これは意図的な**2層決定論**の設計です：socsim コアがシード決定論的であり，その上に LLM 層を*キャッシュ疑似決定論的*に重ねます．規約として LLM 呼び出しはメカニズムの `Decision` フェーズに閉じ込めます（LLM 呼び出しは `Mechanism::apply` 内でインラインに行う同期的な `complete` にすぎません）．

![LLM layer: two-layer determinism](assets/arch-llm-layer.svg)

すべてはプロバイダ非依存の単一トレイトの上に構築されています：

```rust,ignore
pub trait LlmClient {
    fn model(&self) -> &str;
    fn endpoint(&self) -> &str;
    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError>;
}
```

本番スタックは `live` feature 越しに1回の呼び出しで組み立てます：

```rust,ignore
let client: CachingClient<Box<dyn LlmClient>> =
    socsim_llm::build_live_client(cache_path /* Option<&Path> */)?;
```

`build_live_client` は環境変数から **Ollama-first → OpenAI-fallback → 型消去 → キャッシュ**を構成します：

- **Ollama**（プライマリ）— `OLLAMA_HOST`（既定 `http://localhost:11434`）と `OLLAMA_MODEL`（既定 `llama3.1`）．
- **OpenAI**（ベストエフォートのフォールバック）— `OPENAI_API_KEY` と `OPENAI_MODEL`（既定 `gpt-4o-mini`）．`OPENAI_API_KEY` が未設定ならプレースホルダを構築し，Ollama 自体が失敗したときにのみエラーになります（Ollama 単体の構成でも動作します）．
- バックエンドは `Box<dyn LlmClient>` に型消去され，本番スタックでもテスト用モックの注入でも同じ具体的な戻り値型で扱えます．

構築は**遅延**です — キャッシュミス時に `CachingClient::complete` が呼ばれるまでネットワーク呼び出しは発生しません．

疑似決定論は2つの要素から生まれます：

- **`PromptCache`** — `hash(prompt + model)`（`cache_key`）をキーとするプロンプト → レスポンスのキャッシュで，インメモリ（`PromptCache::in_memory`）または JSON ファイルバック（`PromptCache::open`，アトミック保存）です．`LlmConfig::deterministic()` は `temperature = 0` と固定 `seed` を設定し，ウォームキャッシュと組み合わせると再実行で同一のレスポンスを再生し，ノイズの多いモデルを再現可能なオラクルに変えます．
- **`MetadataCollector`** / **`RunMetadata`** — `CallMetadata` が呼び出しごとに model / endpoint / temperature / seed / `cache_hit` を記録し，`MetadataCollector::summary()` がこれらをシリアライズ可能な `RunMetadata`（model，endpoint，生成設定，総呼び出し数，キャッシュヒット数，キャッシュヒット率）に集約します．replication はこれを（例：`llm_meta.json` として）永続化します．

決定論的なテストには `mock::ScriptedClient` があります — クロージャで応答するネットワークフリーの `LlmClient` で，ライブバックエンドとまったく同じように `CachingClient` に差し込めます．

ライブモデルから使える回答までの経路を堅牢にする補助が2つあります：ライブの Ollama/OpenAI バックエンドは空（空白のみ）の応答を `LlmError::EmptyResponse { endpoint, model }` で拒否します —— reasoning/harmony 系モデルは `num_predict` バジェットを丸ごと隠れた思考トレースに費やして可視の回答を出さないことがあり，これをエラーとして表面化させることで，呼び出し側は黙って空文字列を伝播させる代わりにリトライやバジェット増加を選べます —— また，常にコンパイルされる `extract_first_choice(text, vocab)` は，ラベル → シノニム表を単語境界で走査して自由文出力を離散ラベルへ写像します（markdown/句読点に寛容，最初の出現が勝ち，同位置では最長シノニムでタイブレーク）．

このクレートは**ライブラリ専用**で `socsim` バイナリには組み込まれていません；軽量 replication は git 依存で直接取り込みます．

---

## 結果出力ヘルパ（socsim-results）

`socsim-results` は，軽量ライブラリモードの replication がそろってコピーしていた出力ボイラープレートを切り出したものです．これらの replication は独自の `main.rs` + clap CLI を備え出力を直接書き込む（`Recorder`/`Scenario` 機構を使わない）ため，このクレートは依存の少ない**リーフクレート**です — `std` に加えて `serde`/`serde_json`/`csv`/`chrono` のみで，**`socsim-*` 依存はない**ので，取り込んでも `socsim-log`/`-config`/`-runner` を一切引き込みません．

![Result output convention](assets/arch-results.svg)

共有の `results/` 出力規約を提供します：

- `timestamp()` — 現在のローカル時刻を `YYYYMMDD_HHMMSS` のスタンプで返します．
- `create_run_dir(base)` — タイムスタンプ付き実行ディレクトリ `base/<timestamp>` を作成します；`ensure_dir(path)` は冪等な `mkdir -p` です．
- `refresh_latest_symlink(base, target)` — `base/latest` を最新の実行に（再）指定します（Unix のシンボリックリンク；それ以外ではベストエフォートの no-op）．
- `write_csv(rows, path)` / `write_json(value, path)` — serde ベースの CSV/JSON ライタ（`socsim-llm` の `RunMetadata` はこの JSON ライタで永続化されます）．I/O / CSV / JSON の失敗源をまとめた `WriteError` を返します．

設計上ドメイン非依存です：汎用のシリアライズプリミティブのみを提供するので，ドメイン型（`socsim-llm` の `RunMetadata` など）はそれぞれの所有クレートに置き，ここでは `write_json` 経由で書き込みます．

---

## ソーシャルネットワーク層

`socsim-net` は `SocialNetwork` を提供します — `petgraph::UnGraph<AgentId, ()>` の薄いラッパーで，O(1) ルックアップのための `AgentId → NodeIndex` マップを持つ無向グラフです．3つのランダムグラフジェネレーターが含まれており，すべて `&mut SimRng` を受け取ります：

| ジェネレーター | モデル |
|---|---|
| `SocialNetwork::erdos_renyi(ids, p, rng)` | Erdős–Rényi G(n,p) |
| `SocialNetwork::watts_strogatz(ids, k, beta, rng)` | Watts–Strogatz スモールワールド |
| `SocialNetwork::barabasi_albert(ids, m, rng)` | Barabási–Albert 優先的結合 |

HRライフサイクルベースラインは `watts_strogatz(k=4, beta=0.1)` を使用して従業員間のスモールワールドネットワークをモデル化します．`toxic_spread` と `turnover` メカニズムは各ステップで隣接リストを問い合わせます．

---

## キャリブレーション哲学

HRライフサイクルモジュールはパラメータを2つのカテゴリに分けています：

### 経験的相関（ρ）

これらは発表されたメタ分析から直接引用した**固定された影響強度**です．文献で記録された効果の方向と相対的な大きさを表します．基礎となる引用を置き換える場合を除き，変更すべきではありません．

| 定数 | 値 | 出典 |
|---|---|---|
| `RHO_SI` | 0.51 | Schmidt & Hunter (1998) — 構造化面接の妥当性 |
| `ALPHA_PEER` | 0.17 | Mas & Moretti (2009) — ピア生産性乗数 |
| `P_TOXIC` | 0.04 | Housman & Minor (2015) — 有害労働者の基準有病率 |
| `P_SPREAD` | 0.46 | Housman & Minor (2015) — 有害行動の感染確率 |
| `PHI_TACIT` | 0.85 | Nonaka (1994) — 暗黙知対総知識の比率 |
| `RHO_PJ` | 0.20 | Kristof-Brown et al. (2005) — PJ適合の相関 |
| `RHO_PO` | 0.07 | Kristof-Brown et al. (2005) — PO適合の相関 |
| `RHO_PO_TURN` | −0.35 | Kristof-Brown et al. (2005) — PO適合対離職意図 |
| `LAMBDA_LEARN` | 0.15 | Bahk & Gort (1993) — 学習曲線成長率 |

### 月次ダイナミクススケールパラメータ（チューナブル）

これらは，シミュレーションの月次ダイナミクスのペースと大きさを制御する**キャリブレーション制御パラメータ**です．直接的な経験的対応物はありませんが，モデルが妥当な軌跡（例：年間~15〜22%の自発的離職率，徐々に成長するが発散しない知識ストック）を生成するように調整されています．

| 定数 | 値 | 制御対象 |
|---|---|---|
| `BASE_MONTHLY_QUIT_HAZARD` | 0.008 | 基準~0.8%/月の離職確率 |
| `BASE_QUIT_LOGIT` | −4.82 | ロジット切片（`logit(0.008)`） |
| `QUIT_EMBED_SENS` | 1.0 | (1 − 埋め込み度)に対する離職ロジットの感度 |
| `QUIT_SAT_SENS` | 0.8 | (1 − 満足度)に対する離職ロジットの感度 |
| `QUIT_CASCADE_BUMP` | 0.30 | 離職した隣接者ごとの加算的ロジットバンプ（Krackhardtカスケード） |
| `ALPHA_K` | 0.30 | チーム知識ストックへのOCB流入係数 |
| `BETA_LOSS` | 1.0 | 在職期間（年単位）に対する知識損失指数 |
| `KAPPA_LOSS` | 0.40 | 知識損失の大きさ係数 |
| `THETA_MEAN` | 1.0 | 採用時の真の能力θの平均 |
| `THETA_SD` | 0.2 | θの標準偏差 |

すべてのキャリブレーション定数は，引用元を記載したdocコメントとともに `crates/socsim-packs/src/hr_lifecycle/calibration.rs` にあります．

---

## シナリオTOMLスキーマ

シナリオTOMLには4つのセクションがあります：

```toml
[simulation]   # name, module_pack, t_max, seed, scheduler
[world]        # ワールドファクトリーに転送される自由形式のパラメータ
[[mechanism]]  # 順序付き配列；構成するメカニズムごとに1エントリー
[output]       # log_pathテンプレートとメトリクスキー
```

`[[mechanism]]` 配列は**順序保存**されます：構成順序は宣言順序と等しくなります．各 `Phase` 内では，メカニズムはシナリオファイルに現れる順に発火します．

`output.log_path` テンプレートは `{name}` と `{seed}` の置換をサポートします．

2つのスケジューラーが利用可能です：`sequential`（ソートされた `AgentId` 順，完全に決定論的）と `random_activation`（シミュレーションRNGを使って各ステップでシャッフル）．
