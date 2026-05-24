[English](turnover.md) | **日本語**

# 離職 (`turnover`)

> 各従業員は，組織埋め込み度，満足度，個人–組織適合度，ネットワーク伝染を基に退職するかどうかを決定します．
> **フェーズ:** Decision．**出典:** Kristof-Brown et al. (2005) + Krackhardt & Porter (1986)．**種別:** 混合（経験的 $\rho$ + チューナブルロジット重み）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`turnover` は自発的な従業員離職をモデル化します．ステップごとに固定ランダム順（`ctx.agent_order`）ですべてのアクティブな従業員を走査し，4つの組織行動予測変数——組織埋め込み度，満足度，個人–組織適合度，および先月退職したネットワーク隣接者数（Krackhardtカスケード）——をロジスティック回帰に通した確率でBernoulliコインを引きます．

従業員が退職すると，従業員ロスターとソーシャルネットワークの両方から削除され，そのレコードが `departed_this_step` に追加されます．その後カスケードが残りのすべての隣接者を更新します：各隣接者の `recent_quit_neighbors` カウンターが1増加し，`embeddedness` が0.02減少します（`[0, 1]` にクランプ）．更新された `recent_quit_neighbors` 値は*次の*ステップで `turnover` が読み取り，クラスター的退職波を生み出しうるフィードバックループを形成します．

`turnover` はまた，処理の最初に `headcount_at_step_start` を取得します（削除が行われる前）．これにより，`org_performance`（Reward）が離職率の分母を正確に計算できます．

## 2. 理論と出典

退職決定は標準的なロジスティックモデルに従います：

$$\begin{aligned}
\ell = {}& \text{BASE\_QUIT\_LOGIT}
       + \text{QUIT\_EMBED\_SENS}\,(1-\text{embeddedness}) \\
     & + \text{QUIT\_SAT\_SENS}\,(1-\text{satisfaction})
       + \rho_{\text{po\_turn}}\cdot\text{po\_fit}
       + \text{QUIT\_CASCADE\_BUMP}\cdot n_{\text{quit}}
\end{aligned}$$

$$p_{\text{quit}} = \sigma(\ell) = \frac{1}{1 + e^{-\ell}}$$

（$n_{\text{quit}} = \text{recent\_quit\_neighbors}$．）

`ctx.rng.gen::<f64>()` $< p_{\text{quit}}$ であれば従業員は退職します．

- `BASE_QUIT_LOGIT`（−4.82）は，独立時の基準月次離職率を約0.8%に設定します——業界平均にキャリブレーションされています．
- `QUIT_EMBED_SENS`（1.0）と `QUIT_SAT_SENS`（0.8）は，2つの「押し出し」要因に対するチューナブルな感度重みです．埋め込み度と満足度が高いほど $p_{\text{quit}}$ が下がります．
- $\rho_{\text{po\_turn}}$（−0.35）は Kristof-Brown et al. (2005) のメタ分析から得られた，PO適合度と離職意図の経験的相関です．負の符号は，適合度が高いほど離職が減ることを示します．
- `QUIT_CASCADE_BUMP`（0.30）はチューナブルな伝染重みです．先月退職した隣接者1人につき $\ell$ に0.30が加算され，Krackhardt & Porter (1986) が記録した「スノーボール」パターンを再現します．

**カスケードメカニズム（退職後）：**  
このステップの退職者集合を決定した後，メカニズムはまずすべての `recent_quit_neighbors` を0にリセットし，次に各退職者の旧隣接者リストを走査して `recent_quit_neighbors` カウンターをインクリメントし `embeddedness` を0.02デクリメントします．この2パスアプローチ（全リセット，その後バンプ）により，複数の退職者が隣接者を共有する場合でも正確な集計が保証されます．

## 3. データフロー

![turnover data flow](../assets/mech-turnover.svg)

最初に `headcount_at_step_start` が取得され，次に `ctx.agent_order` 順ですべての従業員が評価されます．退職者が削除された後，Krackhardtカスケードが旧隣接者を更新します．`departed_this_step` リストは下流の `knowledge_loss`（PostStep）と `org_performance`（Reward）に引き継がれます．

## 4. 6フェーズループにおける位置

3番目のフェーズである **Decision** で実行されます——`Environment`（`learning_curve` が個人生産性を設定）の後，`Interaction`（`peer_effect`，`ocb`，`toxic_spread` が生存ロスターに作用）の前です．

Decision 内では，`turnover` と `hiring` の両方が実行されます．両方がアクティブな場合，シナリオTOMLでは `turnover` を `hiring` より前に宣言する必要があります．これにより，(a) `headcount_at_step_start` が離職前の人数を反映し，(b) `hiring` が同ステップの離職で生まれた空席を埋めることができます．順序を逆にすると，`hiring` が同ステップで後に退職する人数分だけターゲットを超過してしまいます．

`fit`（同じくDecision）は離職評価の前に満足度を更新するため，宣言順では `fit` を `turnover` より前に置く必要があります．

## 5. 状態読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `HrWorld.employees` | ✓ | ✓ | 退職者が削除されます． |
| `HrWorld.network` | ✓ | ✓ | 退職者のノードとエッジが削除されます． |
| `HrWorld.departed_this_step` | | ✓ | 退職者ごとに `(id, $\theta$, tenure, team)` を追加；`knowledge_loss` と `org_performance` が消費します． |
| `HrWorld.headcount_at_step_start` | | ✓ | `apply` の最初，削除前に1回設定されます． |
| `Employee.embeddedness` | ✓ | ✓ | 離職ロジットに使用；カスケード隣接者に対して0.02デクリメントされます． |
| `Employee.satisfaction` | ✓ | | 離職ロジットに使用． |
| `Employee.po_fit` | ✓ | | 離職ロジットに使用． |
| `Employee.recent_quit_neighbors` | ✓ | ✓ | カスケードロジット項に使用；0にリセット後，隣接者についてインクリメントされます． |
| `network.neighbors(id)` | ✓ | | カスケードに使用する隣接者リスト． |

## 6. 依存関係と順序制約

**必ず後に実行すべきもの：**
- `fit`（Decision）——`satisfaction` が退職決定の評価前にその ステップの個人適合度更新を反映するようにするため．

**必ず前に実行すべきもの：**
- `hiring`（Decision）——`headcount_at_step_start` が離職前の数値になり，採用がそのステップの空席を埋められるようにするため．
- `knowledge_loss`（PostStep）——`turnover` が `departed_this_step` を生成し，`knowledge_loss` はそれを読み取って暗黙知の流出を計算します．`turnover` が先に実行されていない状態で同ステップに `knowledge_loss` を実行しないでください．
- `org_performance`（Reward）——`departed_this_step` と `headcount_at_step_start` を使って離職率を計算します．

**共有状態の引き継ぎ：**

| 生産者 | フィールド | 消費者 |
|---|---|---|
| `turnover` | `departed_this_step` | `knowledge_loss`，`org_performance` |
| `turnover` | `headcount_at_step_start` | `org_performance` |
| `turnover` | 更新された `recent_quit_neighbors` | `turnover`（次ステップ） |

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `rho_po_turn` | `−0.35` | 経験的 | Kristof-Brown et al. (2005)，メタ分析相関 |
| `base_quit_logit` | `−4.82` | チューナブル | 月次基準離職率~0.8%にキャリブレーション |
| `quit_embed_sens` | `1.0` | チューナブル | 埋め込み度不足に対する離職ロジットの感度 |
| `quit_sat_sens` | `0.8` | チューナブル | 満足度不足に対する離職ロジットの感度 |
| `quit_cascade_bump` | `0.30` | チューナブル | 直近に退職した隣接者1人あたりの伝染重み |

## 8. 使い方

### シナリオTOML

```toml
[[mechanism]]
name  = "fit"
phase = "decision"
[mechanism.params]
rho_pj = 0.20
rho_po = 0.07

[[mechanism]]
name  = "turnover"
phase = "decision"
[mechanism.params]
rho_po_turn       = -0.35
base_quit_logit   = -4.82
quit_embed_sens   =  1.0
quit_sat_sens     =  0.8
quit_cascade_bump =  0.30
```

`turnover` はDecisionフェーズ内の正しい順序を維持するため，TOML内で `fit` の後かつ `hiring` の前に記述する必要があります．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let mut params = Params::empty();
params.set("rho_po_turn",       -0.35_f64);
params.set("base_quit_logit",   -4.82_f64);
params.set("quit_embed_sens",    1.0_f64);
params.set("quit_sat_sens",      0.8_f64);
params.set("quit_cascade_bump",  0.30_f64);

let turnover = reg.build("turnover", &params)?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(turnover)
    .build();
sim.run()?;
```

## 9. 決定論性とRNG

`turnover` は `ctx.rng` から引数を取得します——`ctx.agent_order` で定義された固定反復順序でステップごとに従業員1人につき1回の `gen::<f64>()` 呼び出しです．`agent_order` は各ステップの開始時にシミュレーションシードから決定論的に導出されるパーミュテーションなので，同じシードによる2回の実行はロジットに `f64` の累積が関わっていても同一の退職シーケンスを生成します．

Krackhardtカスケードはソートされた隣接者順に適用されるため，`embeddedness` のデクリメントも順序非依存です．

## 10. 期待される動作

ベースラインシナリオ（デフォルトパラメータ，60ヶ月実行）では：

- 満足度と埋め込み度が均衡値付近にある場合，月次離職率は0.8〜2%前後で変動します．
- 単一クラスターの退職が発生すると，隣接者の `recent_quit_neighbors` が上昇し，翌月の離職確率が高まります．これにより，`org_performance` が記録する `turnover_rate` の時系列にスパイクとして現れる短命な複数期間カスケードバーストが発生することがあります．
- `quit_cascade_bump` を除去（0に設定）するとスパイクが抑制され，分散の小さい滑らかな離職曲線が得られます．
- `knowledge_loss` は `departed_this_step` リストをチーム単位の暗黙知流出に変換するため，在職期間の長い退職者は `knowledge_stock` に対して不均衡に大きな負の影響を与えます．

## 11. 参考文献

- Kristof-Brown, A. L., Zimmerman, R. D., & Johnson, E. C. (2005). Consequences
  of individuals' fit at work: A meta-analysis of person–job, person–organization,
  person–group, and person–supervisor fit. *Personnel Psychology*, 58(2), 281–342.
- Krackhardt, D., & Porter, L. W. (1986). The snowball effect: Turnover embedded
  in communication networks. *Journal of Applied Psychology*, 71(1), 50–55.
