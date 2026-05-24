[English](org-performance.md) | **日本語**

# 組織パフォーマンス (`org_performance`)

> 従業員ごとの生産性と労働力メトリクスを集計し，次ステップのピア効果のためにチームの平均 $\theta$ を再計算する．
> **フェーズ:** Reward．**出典:** 集計処理．**種別:** n/a．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`org_performance` は各シミュレーションステップを締めくくる計測・帳簿管理メカニズムである．現在のワールド状態から4つの組織レベルメトリクスを集計してシミュレーションの出力ストリームに記録し，次のステップ開始時に `peer_effect` が最新のチームスナップショットを参照できるよう各チームの平均能力（`Team.mean_theta`）を再計算する．

このメカニズム自体は行動ダイナミクスを持たない — 純粋なオブザーバーおよび帳簿管理者である — しかしパックには欠かせない構成員である：これがなければ，`org_performance`（ワールド状態フィールド）は更新されず，`Team.mean_theta` が古くなり，`peer_effect` が静かに壊れてしまう．

## 2. 理論と出典

単一の較正出典はない；4つのメトリクスは標準的な HR アナリティクスの集計値である：

$$\text{org\_performance} = \sum_{i} \pi_i, \qquad \text{turnover\_rate} = \frac{\lvert\text{departed\_this\_step}\rvert}{\max(1,\ \text{headcount\_at\_step\_start})}$$

$$\text{avg\_tenure} = \frac{1}{|E|}\sum_{i \in E} \text{tenure}_i, \qquad \text{knowledge\_stock} = \sum_{k} K_k$$

ソートされた AgentId 順で生産性を合計することで，`BTreeMap` トラバーサルの実装詳細に関わらず f64 の累算が決定論的になる．`headcount_at_step_start` は `turnover` が削除前に捕捉するため，月次離職率の正しい分母が与えられる．

メトリクス記録後，このメカニズムは `recompute_team_means()` を呼び出し，各 `Team.mean_theta` をそのチームの現在のメンバーの平均 `theta` に設定する．このステップをまたがる引き継ぎが鍵となる順序の洞察である：`peer_effect`（Interaction，次ステップ）は，ここ（Reward，このステップ）で書き込まれた `mean_theta` を読み取る．

## 3. データフロー

![org_performance data flow](../assets/mech-org-performance.svg)

`Employee.productivity`，`.tenure`，`.theta`，`.team`，`HrWorld.departed_this_step`，`HrWorld.headcount_at_step_start` を読み取る．`HrWorld.org_performance` と `Team.mean_theta` を書き込み，4つのメトリクスを記録する．

## 4. 6フェーズループ内での位置

5番目のフェーズである **Reward** で，Interaction（`peer_effect` と `ocb` が動作する）と PostStep（`knowledge_loss` と `socialization` がクリーンアップする）の間に実行される．この配置により以下が保証される：

1. `productivity` はここで合計される前に `peer_effect`（Interaction）によって変調されている．
2. `departed_this_step` はこのステップの離脱者を保持したまま（PostStep 末尾の `knowledge_loss` によってのみクリアされる）であるため，`turnover_rate` が正確になる．
3. `Team.mean_theta` はこのステップの採用と離職がすべて確定した**後**に更新されるため，`peer_effect` が次のステップで正確なチームスナップショットを参照できる．

## 5. 状態読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `Employee.productivity` | ✓ | | `org_performance` のため AgentId でソートして合計する． |
| `Employee.tenure` | ✓ | | `avg_tenure` のため平均化する． |
| `Employee.theta` | ✓ | | `recompute_team_means` で使用する． |
| `Employee.team` | ✓ | | `recompute_team_means` で使用する． |
| `HrWorld.departed_this_step` | ✓ | | `turnover_rate` のためカウントする；ここではクリアしない． |
| `HrWorld.headcount_at_step_start` | ✓ | | `turnover_rate` の分母． |
| `HrWorld.org_performance` | | ✓ | 生産性の合計値に設定する． |
| `Team.knowledge_stock` | ✓ | | `knowledge_stock` メトリクスのため合計する． |
| `Team.mean_theta` | | ✓ | 次ステップの `peer_effect` のために再計算する． |

## 6. 依存関係と順序制約

- **上流（同一ステップ）:**
  - `learning_curve`（Environment）と `peer_effect`（Interaction）が，このメカニズムが合計する前に最終的な `productivity` 値を設定していること．
  - `turnover`（Decision）が，離職率計算のために `departed_this_step` を設定し `headcount_at_step_start` を記録していること．
  - `hiring`（Decision）が実行済みであること（新規採用者が `recompute_team_means` に含まれるため）．
- **下流（次ステップ）:** `peer_effect` が `Team.mean_theta` を読み取る；`peer_effect` が存在する場合は必ず `org_performance` を含めること．
- **下流（同一ステップ）:** `knowledge_loss`（PostStep）は，`turnover_rate` がここで計算される時点で `departed_this_step` がまだ存在するよう，このメカニズムの**後に**実行しなければならない．

## 7. パラメータ

なし．`org_performance` はチューナブルパラメータを持たない純粋な集計メカニズムである．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "org_performance"
phase = "reward"
```

`[mechanism.params]` ブロックは不要である．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let op = reg.build("org_performance", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(op)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

ランダム性を**引き出さない**．生産性は決定論的な f64 累算を保証するために AgentId のソート順で合計する．他のすべての集計（`avg_tenure`，`knowledge_stock`）も同様に順序非依存かまたは明示的にソートされる．

## 10. 期待される動作

`org_performance`（メトリクス）は，平均在職期間の蓄積と `learning_curve` による生産性の $\theta$ への向上に伴い，最初の 12〜24 ステップで上昇する．その後，採用と離職がバランスすると安定する．離職の急増は一時的な低下を引き起こす（新規採用者の生産性はほぼゼロ）；知識ショックイベントは `knowledge_stock` 系列に現れる．`avg_tenure` 系列は労働力安定性の補完的な視点を提供する．

## 11. 参考文献

外部引用なし．`org_performance` は標準的な集計メカニズムであり，4つのメトリクスは従来の HR アナリティクス指標である．
