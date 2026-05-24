[English](policy-mechanism.md) | **日本語**

# ポリシーメカニズム (`policy`)

> 任意の固定ヒューリスティックを学習可能なポリシーで置き換える汎用 Decision フェーズラッパーで，標準のシミュレーションループ内でマルチエージェント強化学習を可能にする．
> **フェーズ:** Decision．**出典:** MARL (§14.1)．**種別:** learnable．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`PolicyMechanism<W, P, E, A>` は `socsim-marl` クレートの汎用 Decision フェーズメカニズムで，共有 `Policy` を `ObsEncoder`（ワールド → 観測）と `ActionApplier`（行動 → ワールド変異）とともにラップする．これは任意の固定 Decision フェーズヒューリスティックへのドロップイン置き換えである：シミュレーションエンジンは他のメカニズムと全く同様に `apply()` を呼び出し，エンジンへの変更は不要である．

2つの動作モードにより，同一の型が推論と学習の両方に使える：

- **推論モード** — 貪欲な行動選択（`policy.act`），RNG を消費せず，凍結されたポリシーで決定論的．
- **収集モード** — 確率的サンプリング（`policy.sample`，`ctx.rng` を使用）に加え，`MarlTrainer`（REINFORCE，`DiscretePolicyNet` burn MLP，CPU）へ供給する共有 `TrajectoryBuffer` への軌跡記録．

`PolicyMechanism` は**ライブラリ専用**である：`HrLifecyclePack` には登録されておらず，`socsim` バイナリやシナリオ TOML ファイルからは利用できない．Rust コードで構築し `SimulationBuilder` に直接追加する必要がある．`socsim-hr-lifecycle` の `marl` feature フラグの後ろに配置されている．

## 2. 理論と出典

MARL（socsim 設計ドキュメントの §14.1）は各シミュレーションエージェントを強化学習のアクターとして扱う．各 Decision フェーズで，エンコーダーが現在のワールド状態をエージェントごとの観測ベクトルにマッピングし，ポリシーが離散的な行動インデックスを出力し，アプライアーがそのインデックスをワールドの変異に変換する．学習には `DiscretePolicyNet`（`burn` フレームワークで構築した浅い MLP，CPU 上で動作）による REINFORCE を使用する．

2つのモードは次の通りである：

```text
Inference mode:
    obs    ← encoder.encode(world, agent_id)       (skip agent if None)
    action ← policy.act(obs)                        (greedy, no RNG)
    applier.apply(world, agent_id, action, ctx.rng)

Collect mode:
    obs    ← encoder.encode(world, agent_id)       (skip agent if None)
    action ← policy.sample(obs, ctx.rng)            (stochastic, uses ctx.rng)
    applier.apply(world, agent_id, action, ctx.rng)
    buffer.begin_decision(agent_id, obs, action)    (record for trainer)
```

形式的には，収集モードではポリシー分布から行動をサンプリングし，推論モードでは貪欲な行動を選択する：

$$a_t \sim \pi_\theta(\,\cdot \mid o_t) \quad(\text{collect}), \qquad a_t = \arg\max_{a} \pi_\theta(a \mid o_t) \quad(\text{inference})$$

ポリシーはエピソード間でオフラインに REINFORCE 勾配推定で学習する：

$$\nabla_\theta J = \mathbb{E}\!\left[\sum_t \nabla_\theta \log \pi_\theta(a_t \mid o_t)\, G_t\right]$$

ポリシーは `Rc<RefCell<Policy>>` で共有されるため，`MarlTrainer` がエピソード間で重みを更新する一方で，メカニズムは実行中に同じ参照を保持する．

## 3. データフロー

![policy data flow](../assets/mech-policy-mechanism.svg)

読み書きされる状態は，呼び出し元が提供する `ObsEncoder` と `ActionApplier` の実装に完全に依存する．メカニズム自体は特定のワールドフィールドを直接操作しない；すべてのワールドアクセスは汎用のエンコーダー/アプライアーペアを通じて行われる．

## 4. 6フェーズループ内での位置

3番目のフェーズである **Decision** で実行される．これにより，HR ライフサイクルパックの `fit`，`turnover`，`hiring` と並置される．Decision 内の宣言順序が実行順序を決定する；`PolicyMechanism` は，その `ActionApplier` が導入する依存関係を尊重するように配置すること（例えば，アプライアーが `satisfaction` を変更する場合，設計意図に応じて `fit` がそのフィールドを更新した後に実行するか前に実行するかを決める）．

## 5. 状態読み書きコントラクト

コントラクトは**汎用**である：呼び出し元が指定する具体的な `ObsEncoder<W>` および `ActionApplier<W>` 型に依存する．

| 操作 | アクター | 備考 |
|---|---|---|
| ワールド状態の読み取り | `encoder.encode(world, aid)` | `Option<obs>` を返す；`None` の場合エージェントをスキップ． |
| ワールド状態の書き込み | `applier.apply(world, aid, action, ctx.rng)` | 任意の変異を許可． |
| 軌跡の書き込み | `buffer.begin_decision(aid, obs, action)` | 収集モードのみ． |

`PolicyMechanism` レベルで固定のフィールドコントラクトはない．コントラクトはエンコーダーとアプライアーの実装でドキュメント化すること．

## 6. 依存関係と順序制約

- **上流:** `ObsEncoder` が読み取るものは最新でなければならない．エンコーダーが `Employee.productivity` を読み取る場合，`learning_curve` と `peer_effect` を先に実行する必要があるが，それらは Environment と Interaction のメカニズムであり Decision より前に発火するため，フェーズの順序付けが自動的にこれを処理する．
- **下流:** `ActionApplier` が書き込むものは後続のメカニズムが消費する．必要に応じて，Decision フェーズ内でそれらのコンシューマーより前に `PolicyMechanism` を宣言すること．
- **学習ループ:** 収集モードでは，`MarlTrainer` はステップ後に `buffer.close_step(rewards)` を呼び出し，エピソード間に学習更新を実行しなければならない．完全な学習ループパターンは [library.ja.md#learnable-policies-marl](../library.ja.md#learnable-policies-marl) を参照．
- **Feature フラグ:** `Cargo.toml` に `socsim-marl`（`marl` feature 付き）を追加すること．

## 7. パラメータ

`PolicyMechanism` にはシナリオ TOML パラメータがない．ポリシーの重み，ネットワークアーキテクチャ，学習ハイパーパラメータは `MarlTrainer` と `Policy` 実装が管理し，メカニズムレジストリは関与しない．

## 8. 適用方法

`PolicyMechanism` は**ライブラリ専用**である — `[[mechanism]]` TOML ブロックは存在しない．Rust で構築し `SimulationBuilder` に直接追加すること．

### ライブラリモード

```rust
use std::cell::RefCell;
use std::rc::Rc;

use socsim_marl::{PolicyMechanism, TrajectoryBuffer};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

// Your encoder and applier implementations.
let encoder = MyObsEncoder::new();
let applier = MyActionApplier::new();

// Shared policy (e.g. a trained DiscretePolicyNet loaded from disk).
let policy = Rc::new(RefCell::new(my_policy));

// --- Inference mode (frozen policy, no RNG, bit-reproducible) ---
let infer_mech = PolicyMechanism::inference(
    Rc::clone(&policy),
    encoder.clone(),
    applier.clone(),
);

// --- Collect mode (stochastic, records trajectories for MarlTrainer) ---
let buffer = Rc::new(RefCell::new(TrajectoryBuffer::new()));
let collect_mech = PolicyMechanism::collecting(
    Rc::clone(&policy),
    encoder,
    applier,
    Rc::clone(&buffer),
);

// Add to SimulationBuilder like any other mechanism.
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(collect_mech)
    .build();
sim.run()?;
```

完全な学習ループ（REINFORCE 更新，報酬の割り当て，エピソードリセット）については [学習ポリシー (MARL)](../library.ja.md#learnable-policies-marl) を参照．

## 9. 決定論性と RNG

**推論モード:** ランダム性を**引き出さない**（`policy.act` は貪欲で決定論的）．同じワールド状態が与えられれば，凍結されたポリシーによる実行はビット単位で再現可能である．

**収集モード:** ステップごと，行動可能なエージェントごとに `policy.sample(obs, ctx.rng)` を通じて `ctx.rng` から1回引き出す．イテレーション順は `ctx.agent_order` に従い，これはシミュレーションスケジューラーによって決定される（通常，メカニズムが発火するより前のステップセットアップで引き出されたランダムな順列）．したがって，収集モードでの再現性には同一シード**および**同一エージェント順スケジュールが必要である．

`ActionApplier` が確率的なワールド変異を必要とする場合も `ctx.rng` を消費する可能性がある；アプライアーの実装でこれをドキュメント化すること．

## 10. 期待される動作

よく学習されたポリシーによる推論モードでは，固定ヒューリスティックのベースラインと比較して，シミュレーションはより高い `org_performance` またはより低い `turnover_rate`（報酬シグナルに依存）を生成し，ポリシーが学習した戦略を反映するはずである．学習初期の収集モードでは動作は本質的にランダムであり，REINFORCE が収束するにつれてポリシーは学習した目標に向かってシフトする．

## 11. 参考文献

- socsim 設計ドキュメント §14.1 — 学習ポリシー (MARL)．
- Williams, R. J. (1992). Simple statistical gradient-following algorithms for
  connectionist reinforcement learning. *Machine Learning*, 8(3–4), 229–256.
  (REINFORCE algorithm)
