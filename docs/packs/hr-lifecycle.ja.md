[English](hr-lifecycle.md) | **日本語**

# `hr-lifecycle` パック

> 参照用の**従業員ライフサイクル**モデルです．エージェントはチームに所属する従業員であり，学習し，相互作用し，退職するか否かを決定し，そして補充されます．すべてのパラメータは，公表された組織行動研究の知見に対してキャリブレーションされています．
> **ワールド：** `HrWorld`．**メカニズム：** 10個．**Cargo フィーチャ：** `pack-hr-lifecycle`（デフォルトで有効）．**時間単位：** 1ステップ = 1か月．

[← パックカタログに戻る](../packs.ja.md)

## 1. 概要

`hr-lifecycle` パックは，組織の労働力が時間とともにどのように変化するかをモデル化します．各ステップ（1か月）において，すべての従業員が業務を通じて学習し，自チームに貢献し，満足度が上下し，留まるか退職するかを決定します．退職はチームの知識を流出させ，ネットワークカスケードを引き起こす一方，採用が欠員を補充します．これはパックの参照モジュールであり，その10個のメカニズムは公表された経験的相関（Schmidt & Hunter，Mas & Moretti，Kristof-Brown，Krackhardt，Nonaka，…）に対して[キャリブレーション](../architecture.ja.md#キャリブレーション哲学)されているため，ベースライン実行は現実的な月次の離職率，在職期間，知識ダイナミクスを再現します．

これは[ユースケースランブック](../usecases.ja.md)および [T5 シナリオパックチュートリアル](../tutorials/05-scenario-pack.ja.md)の背後にある実践例であり，コンパイル可能なライブラリドライバが [`crates/socsim-packs/examples/hr_baseline.rs`](../../crates/socsim-packs/examples/hr_baseline.rs) にあります．

## 2. ワールド：`HrWorld`

![HrWorld data model](../assets/pack-hr-lifecycle-world.svg)

`HrWorld` は，すべてのメカニズムが読み書きする共有状態を保持します．

| フィールド | 型 | モデル化対象 |
|---|---|---|
| `employees` | `BTreeMap<AgentId, Employee>` | 在職中のロスター（決定性のため id でソート） |
| `teams` | `Vec<Team>` | チームごとの `knowledge_stock` とキャッシュ済み `mean_theta` |
| `network` | `SocialNetwork` | [Watts–Strogatz](../architecture.ja.md) スモールワールドの結合グラフ |
| `org_performance` | `f64` | 集約された生産性．各 Reward フェーズで更新 |
| `base_mean_theta` | `f64` | *t = 0* における平均能力 θ．`peer_effect` の正規化基準 |
| `target_team_size` | `usize` | `hiring` が補充の目標とする人員数 |
| `new_hires_this_step` | `Vec<AgentId>` | 一時的：このステップの新規採用者 → `socialization` が消費 |
| `departed_this_step` | `Vec<(id, θ, tenure, team)>` | 一時的：このステップの退職者 → `knowledge_loss`，`org_performance` が消費 |
| `headcount_at_step_start` | `usize` | 一時的：`turnover` が取得するスナップショット → `turnover_rate` の分母 |

各 **`Employee`** は，メカニズムが作用する行動状態を保持します．
`theta`（真の能力，~N(`THETA_MEAN`, `THETA_SD`) から抽出），`tenure`，
`team`，`embeddedness`，`po_fit`，`pj_fit`，`satisfaction`，`socialization`，
`productivity`，`cum_reward`，`is_toxic`，そして `recent_quit_neighbors`（離職カスケードを駆動するカウンター）です．各 **`Team`** は `knowledge_stock` と，毎ステップ再計算されるキャッシュ済み `mean_theta` を保持します．

ワールドは，シード付き [`SimRng`](../architecture.ja.md) から `HrWorld::new(n_teams, team_size, ws_k, ws_beta, &mut rng)` によって構築されるため，あるシードは常に同じ初期組織を生成します．

> 3つの一時バッファはこのパックの中核です．これらは，10個の本来独立したメカニズムを1つのまとまった月次サイクルへと結びつける**共有状態の受け渡し**です（§4参照）．

## 3. 10個のメカニズム

パックは10個のメカニズムを登録します．各メカニズムは[メカニズムカタログ](../mechanisms.ja.md)の完全なページにリンクしており，そこで方程式，出典，状態コントラクト，パラメータを確認できます．

| メカニズム | フェーズ | 種別 | ライフサイクルにおける役割 |
|---|---|---|---|
| [`learning_curve`](../mechanisms/learning-curve.ja.md) | Environment | empirical | 在職期間に基づく学習効果（learning-by-doing）が各従業員の生産性を高める． |
| [`fit`](../mechanisms/fit.ja.md) | Decision | empirical | 個人–職務／個人–組織適合度が職務満足度を駆動する． |
| [`turnover`](../mechanisms/turnover.ja.md) | Decision | mixed | ロジスティックな月次退職ハザードと Krackhardt ネットワークカスケード． |
| [`hiring`](../mechanisms/hiring.ja.md) | Decision | empirical | チームを目標まで補充する．選抜は妥当性シグナルを通じて能力を観測する． |
| [`peer_effect`](../mechanisms/peer-effect.ja.md) | Interaction | empirical | チームの能力が各メンバーの実効生産性を引き上げる． |
| [`ocb`](../mechanisms/ocb.ja.md) | Interaction | tunable | 組織市民行動がチームの知識ストックに加算される． |
| [`toxic_spread`](../mechanisms/toxic-spread.ja.md) | Interaction | empirical | 有害な行動がネットワークのエッジに沿って広がる． |
| [`org_performance`](../mechanisms/org-performance.ja.md) | Reward | aggregation | 生産性を集約し，そのステップのメトリクスを記録する． |
| [`knowledge_loss`](../mechanisms/knowledge-loss.ja.md) | PostStep | mixed | 退職するベテランがチームの暗黙知を流出させる． |
| [`socialization`](../mechanisms/socialization.ja.md) | PostStep | calibration | 新規採用者をオンボーディングし，組織埋め込み度を高める． |

オプションの学習可能な [`policy`](../mechanisms/policy-mechanism.ja.md) メカニズム（MARL）は，`marl` Cargo フィーチャの背後で，ハードコードされた離職決定を置き換えられます — [アーキテクチャノート](../architecture.ja.md)を参照してください．

## 4. 6フェーズティックループにまたがる構成

メカニズムは socsim の固定の [6フェーズ順](../architecture.ja.md#6フェーズティックループ)，すなわち `PreStep → Environment → Decision → Interaction → Reward → PostStep` にわたって構成されます．フェーズ内では，シナリオでの宣言順に発火します．次の図は，月次サイクル全体と，フェーズを接続する共有状態の受け渡し（破線）を示します．

![hr-lifecycle mechanism pipeline](../assets/pack-hr-lifecycle-pipeline.svg)

順序が重要になるのは，これらの受け渡しがあるためです．

- **`turnover` → `knowledge_loss`，`org_performance`**（`departed_this_step` 経由）．
  `turnover` は両者より前に実行されなければなりません．また，誰かを削除する*前*に `headcount_at_step_start` を取得するため，`org_performance` は明確に定義された `turnover_rate` の分母を持ちます．
- **`hiring` → `socialization`**（`new_hires_this_step` 経由）．Decision フェーズ内では，`hiring` が同じステップの欠員を補充し，`headcount_at_step_start` が離職前の人員数を反映するように，`turnover` を `hiring` の*前*に宣言してください．
- **`fit` → `turnover`**：`fit` が `satisfaction` を更新し，`turnover` がそれを退職ロジットで読み取るため，`fit` は `turnover` に先行します．
- **`learning_curve` → `peer_effect`**：生産性は Environment で設定され，その後 `peer_effect` が Interaction でそれをスケールします．

スターターシナリオは，すでに正しい順序でメカニズムを宣言しています．各メカニズムページでは，それぞれの順序制約を詳細に解説しています（例：[turnover §6](../mechanisms/turnover.ja.md)）．

## 5. メトリクスと出力

`org_performance`（Reward）は，毎ステップ4つのメトリクスを記録し，JSONL ログに書き込み，`socsim summarize` で表示します（[CLI リファレンス](../cli.ja.md)参照）．

| メトリクス | 意味 |
|---|---|
| `org_performance` | 従業員 `productivity` の合計 — 集約された実効出力． |
| `avg_tenure` | ロスター全体の平均在職期間（月）． |
| `turnover_rate` | そのステップの `departed_this_step / headcount_at_step_start`． |
| `knowledge_stock` | 全チームにわたる `team.knowledge_stock` の合計． |

また，同じログに `turnover` および `hiring` の**イベント**（影響を受けた `agent_id` を含む）を出力するため，個々の異動を再構成できます．正確なレコード形状については [org_performance](../mechanisms/org-performance.ja.md) を参照してください．

## 6. 適用方法

### シナリオ / CLI

スターターシナリオを生成して実行します．

```sh
socsim init --module-pack hr-lifecycle --out scenarios/hr.toml
socsim run scenarios/hr.toml
```

スターター TOML は，Watts–Strogatz ネットワーク上の5チーム組織を60の月次ステップにわたって構成し，10個すべてのメカニズムを妥当な順序で宣言します．

```toml
[simulation]
name        = "hr_lifecycle_baseline"
module_pack = "hr-lifecycle"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[world]
n_teams           = 5
team_size_initial = 8
network_model     = "watts_strogatz"
network_k         = 4
network_beta      = 0.1

[[mechanism]]
name  = "learning_curve"
phase = "environment"
[mechanism.params]
lambda_learn = 0.15

[[mechanism]]
name  = "fit"
phase = "decision"
[mechanism.params]
rho_pj = 0.20
rho_po = 0.07

[[mechanism]]
name  = "turnover"     # before hiring, after fit
phase = "decision"
[mechanism.params]
rho_po_turn       = -0.35
base_quit_logit   = -4.82
quit_embed_sens   = 1.0
quit_sat_sens     = 0.8
quit_cascade_bump = 0.30

[[mechanism]]
name  = "hiring"
phase = "decision"
[mechanism.params]
rho_si  = 0.51
p_toxic = 0.04

[[mechanism]]
name  = "peer_effect"
phase = "interaction"
[mechanism.params]
alpha_peer = 0.17

[[mechanism]]
name  = "ocb"
phase = "interaction"
[mechanism.params]
alpha_k = 0.30

[[mechanism]]
name  = "toxic_spread"
phase = "interaction"
[mechanism.params]
p_toxic  = 0.04
p_spread = 0.46

[[mechanism]]
name  = "org_performance"
phase = "reward"

[[mechanism]]
name  = "knowledge_loss"
phase = "post_step"
[mechanism.params]
phi_tacit  = 0.85
beta_loss  = 1.0
kappa_loss = 0.40

[[mechanism]]
name  = "socialization"
phase = "post_step"

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["org_performance", "avg_tenure", "turnover_rate", "knowledge_stock"]
```

通常の CLI 動詞で検証，スイープ，要約を行います．

```sh
socsim validate scenarios/hr.toml
socsim run scenarios/hr.toml --seeds 0..30 --parallel
socsim summarize runs/hr_lifecycle_baseline_42.jsonl
```

### ライブラリ

パックを `Registry` に登録し，メカニズムを構築し，[`SimulationBuilder`](../library.ja.md) で駆動します．完全な実行可能版は [`examples/hr_baseline.rs`](../../crates/socsim-packs/examples/hr_baseline.rs) です．

```rust
use socsim_config::{ModulePack, Params, Registry};
use socsim_core::{SimClock, SimRng};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};
use socsim_packs::hr_lifecycle::{HrLifecyclePack, HrWorld};

let mut rng = SimRng::from_seed(42);
let mut world = HrWorld::new(5, 8, 4, 0.1, &mut rng);
world.clock = SimClock::new(60);

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let mut builder = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42);
for name in [
    "learning_curve", "fit", "turnover", "hiring",
    "peer_effect", "ocb", "toxic_spread",
    "org_performance", "knowledge_loss", "socialization",
] {
    builder = builder.add_mechanism(reg.build(name, &Params::empty())?);
}
let mut sim = builder.build();
sim.run()?;
```

## 7. キャリブレーション定数

すべての経験的パラメータは [`crates/socsim-packs/src/hr_lifecycle/calibration.rs`](../../crates/socsim-packs/src/hr_lifecycle/calibration.rs) にあり，出典を示すドキュメントコメントが付いています．代表的な値は次のとおりです．

| 定数 | 値 | 出典 |
|---|---|---|
| `RHO_SI` | `0.51` | 構造化面接の妥当性 — Schmidt & Hunter (1998) |
| `RHO_GMA` | `0.51` | GMA → パフォーマンス — Schmidt & Hunter (1998) |
| `ALPHA_PEER` | `0.17` | ピア効果乗数 — Mas & Moretti (2009) |
| `P_TOXIC` / `P_SPREAD` | `0.04` / `0.46` | 有害性の有病率と拡散 — Housman & Minor (2015) |
| `PHI_TACIT` | `0.85` | 退職時の暗黙知喪失 — Nonaka (1994) |
| `RHO_PJ` / `RHO_PO` | `0.20` / `0.07` | 適合度 → 満足度 — Kristof-Brown et al. (2005) |
| `RHO_PO_TURN` | `−0.35` | PO 適合度 → 離職意図 — Kristof-Brown et al. (2005) |
| `LAMBDA_LEARN` | `0.15` | 学習曲線率 — Bahk & Gort (1993) |
| `BASE_MONTHLY_QUIT_HAZARD` | `0.008` | 月あたり約0.8%のベースライン退職率 |
| `C_TURN` | `1.25` | 離職コスト（× 年俸） — Allen (2008) |

*キャリブレーションスケール*とタグ付けされた定数（`ALPHA_K`，`KAPPA_LOSS`，`BASE_QUIT_LOGIT`，`QUIT_*` の感度群，`THETA_*` の能力パラメータ）は，定常状態の流入 ≈ 離職流出となるように選ばれたチューナブルなツマミです．経験的 vs チューナブルの切り分けについては[アーキテクチャページ](../architecture.ja.md#キャリブレーション哲学)が解説しています．

## 8. 関連項目

- [Mechanism カタログ](../mechanisms.ja.md) — このパックが構成するすべてのメカニズム．
- [opinion-dynamics パック](opinion-dynamics.ja.md) — もう1つの同梱パック．
- [ユースケース＆レシピ](../usecases.ja.md) — 実行可能な HR ワークフロー（ベースライン，スイープ，要約）．
- [T5 — シナリオパック](../tutorials/05-scenario-pack.ja.md) — パックをゼロから構築する．
- [CLI リファレンス](../cli.ja.md) · [アーキテクチャ](../architecture.ja.md) · [ライブラリ API](../library.ja.md)
