[English](architecture.md) | **日本語**

# アーキテクチャ

---

## クレートワークスペース

ワークスペースは3層に整理された11個のクレートで構成されています：

```
socsim-cli          ← バイナリ（エントリーポイント）
    └── socsim-runner      ← マルチシード実行，スイープ，サマリー
            ├── socsim-engine      ← Simulation, SimulationBuilder, スケジューラー
            │       └── socsim-log         ← InMemoryRecorder, JsonlRecorder, CsvRecorder
            ├── socsim-config      ← Params, Registry, ModulePack, Scenarioローダー
            │       └── socsim-core        ← トレイト (Mechanism, WorldState, …), AgentId, Phase, Blackboard
            ├── socsim-hr-lifecycle ← リファレンスモジュール（10メカニズム）
            │       └── socsim-net         ← SocialNetwork（ER, WS, BAジェネレーター）
            ├── socsim-grid        ← Grid, GridIndex, 近傍, 距離（空間モデル）
            ├── socsim-marl        ← 学習ポリシー(MARL): Policy, PolicyMechanism, MarlTrainer（burn; ライブラリ専用）
            └── socsim-rng         ← SimRng (ChaCha20), derive_seed
```

依存ルール：

- `socsim-core` と `socsim-rng` は**内部依存なし** — これらが基盤です．
- `socsim-config` は `socsim-core` に依存しますが，循環を避けるため `socsim-engine` には**依存しません**．
- `socsim-engine` は `socsim-core`，`socsim-log`，`socsim-config` に依存します．
- `socsim-runner` は上記すべてに依存し，並列処理のために `rayon` を追加します．
- `socsim-cli` はすべてを `socsim` バイナリとして結合します．
- `socsim-hr-lifecycle`，`socsim-net`，`socsim-grid` はエンジン層の隣に位置し，直交しています；`socsim-grid` は `socsim-core` にのみ依存します．
- `socsim-marl`（Phase 6）は `socsim-engine` と `socsim-core` に依存します．**ライブラリ専用**（`socsim` バイナリには含まれません）で，ニューラルネットフレームワーク `burn` を取り込むため，hr-lifecycle 連携は `marl` feature でゲートしています．

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

すべてのキャリブレーション定数は，引用元を記載したdocコメントとともに `crates/socsim-hr-lifecycle/src/calibration.rs` にあります．

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
