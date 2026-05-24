[English](hiring.md) | **日本語**

# 採用（`hiring`）

> 各チームは，正規分布から能力を抽出し選考シグナルで選別した新規従業員によって，ターゲット人数まで補充されます．
> **フェーズ：** Decision．**出典：** Schmidt & Hunter (1998)．**種別：** 経験的（$\rho_{\text{SI}}$）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`hiring` はステップごとに1回，`turnover` が退職者を削除した後に実行され，`target_team_size` を下回るすべてのチームを補充します．各空席について，キャリブレーション済みの正規分布から候補者の真の能力 $\theta$ を抽出し，標準化された能力スコアと測定ノイズをブレンドした選考シグナルを構築したうえで（不完全な評価ツールを模したもの），無条件で採用します．新規従業員はソーシャルネットワーク内の既存チームメンバー最大2人と接続され，`new_hires_this_step` に追加されます．`socialization` メカニズム（PostStep）が同じステップの後半でこのリストを処理し，社会的統合を初期化します．

このように `hiring` は2つの構造的な役割を担います．人員数の維持と，オンボーディングを駆動する `new_hires_this_step` バッファへの書き込みです．

## 2. 理論と出典

選考モデルは，Schmidt & Hunter (1998) の人材選考妥当性に関するメタ分析の枠組みに従います．その中心にあるのは，選考ツールは真の能力についてノイズ混じりのシグナルしか捉えられない，という考え方です．

$$\theta \sim \mathcal{N}(\theta_{\text{mean}}, \theta_{\text{sd}}^2), \qquad \theta \leftarrow \max(\theta, \theta_{\text{floor}})$$

$$\text{signal} = \rho_{\text{SI}}\, z_\theta + \sqrt{1 - \rho_{\text{SI}}^2}\;\varepsilon, \qquad z_\theta = \frac{\theta - \theta_{\text{mean}}}{\theta_{\text{sd}}}, \quad \varepsilon \sim \mathcal{N}(0,1)$$

- $\rho_{\text{SI}}$（0.51）は Schmidt & Hunter (1998) が示した経験的な選考妥当性，すなわち選考シグナルと真の職務パフォーマンスとの相関です．完璧なツールなら $\rho_{\text{SI}} = 1$，まったくランダムなツールなら $\rho_{\text{SI}} = 0$ になります．
- この構成により，$\rho_{\text{SI}}$ の値によらず $\operatorname{Var}(\text{signal}) = 1$ が保証され，シグナルが適切に標準化されます．
- 現在の実装では，採用は**無条件**です．シグナルは計算されイベントログに記録されますが，採用判定のゲートとしてはまだ機能していません．これは意図的なモデリング上の選択であり，将来的に閾値方式や top-k 選考ポリシーを導入する余地を残すためのものです．

各新規採用者には `is_toxic` フラグも割り当てられます．このフラグは確率 `p_toxic`（0.04）の Bernoulli として抽出され，Housman & Minor (2015) が報告した基準有病率を再現します．

挿入後，採用者は Watts–Strogatz ソーシャルネットワーク内でランダムに選ばれた既存チームメンバー最大2人と接続されます．

## 3. データフロー

![hiring data flow](../assets/mech-hiring.svg)

各チームの不足分について，`hiring` は `ctx.rng` から $\theta$ と $\varepsilon$ を抽出し，新しい `Employee` レコードを挿入し，最大2本のエッジを持つネットワークノードを追加して，新規エージェントのIDを `new_hires_this_step` に追加します．その後，`socialization` メカニズムが同じステップの後半でそのリストを消費します．

## 4. 6フェーズループにおける位置

3番目のフェーズである **Decision** で，`Environment` の後に実行されます．Decision 内では，`hiring` は `turnover` の**後に**実行する必要があります．理由は次の2点です．

1. `turnover` が取得する `headcount_at_step_start` が，`org_performance` で用いる離職前の人数を反映するようにするため．
2. `hiring` が離職後のチームサイズを確認し，適切な数の空席を埋められるようにするため．

また `hiring` は `socialization`（PostStep）の**前に**実行する必要があります．`socialization` が読み取る `new_hires_this_step` を，`hiring` が生成するためです．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `HrWorld.teams` | ✓ | | 人数不足のチームを探すために走査されます． |
| `HrWorld.target_team_size` | ✓ | | チームあたりのターゲット人数． |
| `HrWorld.employees` | ✓ | ✓ | チーム人数の取得に使用；新規 Employee が挿入されます． |
| `HrWorld.network` | | ✓ | 新規ノードが追加；既存チームメンバーへの最大2本のエッジ． |
| `HrWorld.new_hires_this_step` | | ✓ | 追加；`socialization` が消費します． |
| `HrWorld.next_id` | ✓ | ✓ | 新しい `AgentId` を生成するためにインクリメントされます． |
| `Employee.theta` | | ✓ | N(1.0, 0.2) から抽出し，下限0.1で打ち切る． |
| `Employee.is_toxic` | | ✓ | Bernoulli(p_toxic)． |

その他のすべての `Employee` フィールド（tenure，socialization，embeddedness，po_fit，pj_fit，satisfaction，productivity，cum_reward，recent_quit_neighbors）は構築時にデフォルト値で初期化されます．

## 6. 依存関係と順序制約

**必ず後に実行すべきもの：**
- `turnover`（Decision）．そのステップの離職による空席が見えるようになり，`headcount_at_step_start` が取得されるようにするため．

**必ず前に実行すべきもの：**
- `socialization`（PostStep）．`hiring` が `new_hires_this_step` を生成し，`socialization` がそれを消費します．同ステップで `hiring` を実行せずに `socialization` を実行すると，空のリストを処理することになります．

**共有状態の引き継ぎ：**

| 生産者 | フィールド | 消費者 |
|---|---|---|
| `turnover` | `employees` / `teams` の空席 | `hiring` |
| `hiring` | `new_hires_this_step` | `socialization` |

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `rho_si` | `0.51` | 経験的（選考妥当性） | Schmidt & Hunter (1998) |
| `p_toxic` | `0.04` | 経験的（有害有病率） | Housman & Minor (2015) |

`THETA_MEAN`（1.0），`THETA_SD`（0.2），`THETA_FLOOR`（0.1）はコンパイル定数であり，現在はシナリオパラメータとして公開されていません．

## 8. 使い方

### シナリオTOML

```toml
[[mechanism]]
name  = "turnover"
phase = "decision"
[mechanism.params]
rho_po_turn       = -0.35
base_quit_logit   = -4.82
quit_embed_sens   =  1.0
quit_sat_sens     =  0.8
quit_cascade_bump =  0.30

[[mechanism]]
name  = "hiring"
phase = "decision"
[mechanism.params]
rho_si  = 0.51
p_toxic = 0.04
```

`hiring` は，フェーズ内の正しい順序を保つため，TOML 内で `turnover` の後に記述する必要があります．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let mut params = Params::empty();
params.set("rho_si",  0.51_f64);
params.set("p_toxic", 0.04_f64);

let hiring = reg.build("hiring", &params)?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(hiring)
    .build();
sim.run()?;
```

## 9. 決定論性とRNG

`hiring` は新規採用者ごとに `ctx.rng` から乱数を引きます．内訳は，$\theta$ 用の `Normal` サンプル1回，選考シグナルのノイズ項 $\varepsilon$ 用の `Normal` サンプル1回，`is_toxic` 用の `Bernoulli` 抽出1回，そしてネットワークエッジの接続先（最大2人のチームメンバー）のサンプリングです．ステップあたりの採用数はチームの不足人数で決まり，それ自体も与えられたシードと履歴に対して決定論的なので，同じシードによる実行ではすべての抽出シーケンスが再現されます．

## 10. 期待される動作

ベースラインシナリオでは，次のような挙動が見られます．

- `turnover` が有効な場合，`hiring` はほとんどのステップで，メンバーを失ったチームを補充するために発火し，人員数を `target_team_size × num_teams` に近づけます．
- 新規採用者は `tenure = 0` かつ生産性ほぼゼロで入社します（`learning_curve` を参照）．離職と補充の波が起きると，チームの平均生産性は一時的に低下し，その後の数ヶ月で新規採用者が学習曲線を上っていくにつれて回復します．
- `rho_si` を1.0に近づけると，採用される候補者の $\theta$ が高能力の側に集中し（シグナルが能力をより正確に反映するため），長期の実行では `org_performance` を徐々に押し上げます．
- `p_toxic = 0.04` は，新規採用者の約25人に1人が入社時に有害であることを意味し，`toxic_spread`（Interaction）の発生源となるプールを提供します．

## 11. 参考文献

- Schmidt, F. L., & Hunter, J. E. (1998). The validity and utility of selection
  methods in personnel psychology: Practical and theoretical implications of
  85 years of research findings. *Psychological Bulletin*, 124(2), 262–274.
- Housman, M., & Minor, D. (2015). Toxic workers. *Harvard Business School
  Working Paper* 16-057.
