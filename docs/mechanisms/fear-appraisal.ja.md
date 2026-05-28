[English](fear-appraisal.md) | **日本語**

# 恐怖評価（`fear_appraisal`）

> 各従業員の発言への恐怖 $f_i$ は，本人（または隣接者）がこのステップで報復された場合に増加し，
> ステップごとの小さな減衰で減少し，上司の正の開放性によりさらに緩和されます．
> これは `retaliation_event` が設定したバッファを読み取るステップごとの評価ステップです．
> **フェーズ：** Decision．**出典：** Kish-Gephart et al. (2009)．**種別：** empirical（$\beta_{\text{fear}}$ スケール）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`fear_appraisal` は，同じ Environment フェーズで `retaliation_event` が設定した
一時バッファ `retaliation_this_step` を消費し，それを使って各従業員の `fear` フィールドを更新します．
毎ステップ3つの力が混合されます．

1. **報復ショック** — 影響を受けたエージェント（バッファに書き込まれた標的＋隣接者）の
   fear が `fear_sensitivity` だけ増加します．
2. **ベースライン減衰** — 小さな `DECAY = 0.02` がすべてのエージェントの fear を0方向に引き戻すため，
   報復のない穏やかな実行では蓄積した fear が徐々に消失します．
3. **上司開放性ボーナス** — 従業員の上司が正の開放性（$u_k > 0$）を示すとき，
   `OPEN_BONUS = 0.05 \cdot u_k$ の追加減少が fear に適用されます．
   これは可視的に開かれたリーダーが発言への恐怖を緩衝するという経験的知見をモデル化しています（Detert & Burris 2007）．

更新された `fear` は voice 決定ロジットの $-\beta_f \cdot f_i$ 項を通じて流れ込み，
（LLM 変種の場合は）プロンプトテンプレートにも反映されます．

## 2. 理論と出典

Kish-Gephart, Detert, Treviño & Edmondson (2009) は，職場での fear を動的な評価として扱います．
それは観察可能な制裁の後に上昇し，穏やかな風土では低下し，リーダーシップの手がかりにより緩和されます．
socsim はこれを，3項を持つクランプ付き加法更新として実装します．

$$\text{retaliation\_term}_i = \begin{cases} k_{\text{fear}} & i \in \text{retaliation\_this\_step} \\ 0 & \text{それ以外} \end{cases}$$

$$\text{openness\_term}_i = b_{\text{open}} \cdot \max(0, u_{k(i)})$$

$$f_i \leftarrow \operatorname{clip}_{[0,1]}\!\left( f_i + \text{retaliation\_term}_i - d - \text{openness\_term}_i \right)$$

- $f_i$（`Employee.fear`）— エージェントの発言への恐怖 $\in [0, 1]$．
- $k_{\text{fear}}$（`fear_sensitivity`，デフォルト 0.4）— このステップで報復されたエージェントに適用される
  加法バンプ（`calibration.rs` の `FEAR_SENSITIVITY` 定数）．
- $d$（`DECAY = 0.02`）— ベースラインへのステップごとの引き戻し．メカニズム内のコンパイル時定数．
- $b_{\text{open}}$（`OPEN_BONUS = 0.05`）— 上司が正の開放性を示す場合の fear の追加減少．
  こちらもコンパイル時定数．
- $u_{k(i)}$（`Team.supervisor_openness`）— エージェント $i$ が属するチームの上司開放性．
  正の部分のみが使用されます（敵対的な上司は本メカニズムでは fear を*上げません*．それはスパイラルが行います）．
- 結果は $[0, 1]$ にクランプされます．

voice ロジットにおける支配的な感情シグナルは fear であり，`calibration.rs` で
$\beta_{\text{fear}} = 1.5$ — voice 決定の予測子の中で最大の負の係数 — が設定されています．

## 3. データフロー

`SilenceWorld.retaliation_this_step`（同ステップで `retaliation_event` が設定）を読み取り，
すべてのチームの `Team.supervisor_openness`（エージェントごとの変更前に `Vec<f64>` にスナップショット）と
各エージェントの現在の `Employee.fear` を読み取ります．
すべてのエージェントについて更新された `Employee.fear` を書き戻します．他には何も触れません．

## 4. 6フェーズループにおける位置

3番目のフェーズである **Decision** で実行されます．voice 決定メカニズムの前に置くことで，
新たに更新された fear が，報復が観察されたまさに同じステップで voice ロジット
（および LLM プロンプト）に反映されることを保証します．

Decision 内では，同梱シナリオのメカニズム順は `fear_appraisal` → `voice_decision_rule`（あるいは `voice_decision`）です．
これらを並べ替えると fear 更新が1ティック遅れ，
設計が依存しているステップ内の評価→決定の連結が破綻します．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `SilenceWorld.retaliation_this_step` | ✓ | | O(1) 照会のため `HashSet<AgentId>` に構築． |
| `Team.supervisor_openness` | ✓ | | エージェントごとの変更ループの前に `Vec<f64>` にスナップショット． |
| `Employee.team` | ✓ | | 上司開放性スナップショットへのインデックス． |
| `Employee.fear` | ✓ | ✓ | その場で更新．[0, 1] にクランプ． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：** `retaliation_event`（Environment）が `retaliation_this_step` を
  正規のソート＋重複削除済み影響リストに設定している必要があります．
  報復メカニズムが無効な場合，バッファは毎ステップ空となり，
  `fear_appraisal` は純粋な減衰＋開放性ボーナスに帰着します．
- **下流（同ステップ）：**
  - `voice_decision_rule`（または `voice_decision`）は，voice ロジットの支配的な負項として
    `Employee.fear` を読み取ります．
  - `psafety_update`（PostStep）も報復バッファを読みますが，両者は独立であり異なるフィールドに書き込みます．
- **ステップをまたぐ依存：** fear はステップをまたいで持続するため，
  ステップごとの減衰がベースラインへの唯一の戻り道です．
  反復する報復イベントは fear を上限へ駆り立て，持続的な正の上司開放性は fear を抜き取ります．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `fear_sensitivity` | `0.4` | calibration scale（チューナブル） | Kish-Gephart et al. (2009) |

`DECAY = 0.02` と `OPEN_BONUS = 0.05` 定数はメカニズム本体に埋め込まれたコンパイル時定数であり，
シナリオパラメータとして公開されていません．voice ロジットで更新済み fear を消費する
`BETA_FEAR = 1.5` 係数は `calibration.rs` にあり，
[`voice_decision_rule`](voice-decision-rule.ja.md) ページに記載されています．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "fear_appraisal"
phase = "decision"
[mechanism.params]
fear_sensitivity = 0.4
```

Decision フェーズ内では `voice_decision_rule`（または `voice_decision`）の前に配置し，
fear 更新が同ティックで voice ロジットから見えるようにしてください．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("fear_appraisal", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

乱数を**一切**使用しません．メカニズムは `ctx.world.employees`（`BTreeMap`，`AgentId` でソート）を反復し，
スナップショットした報復集合とチーム開放性ベクトルから各エージェントを独立に更新します．
同じワールド状態に対する2つの実行はビット同一の fear ベクトルを生成します．

## 10. 期待される動作

報復イベントなし（$p_{\text{retaliate}} = 0$）かつ平均的に上司開放性がわずかに正であれば，
fear は数十ステップで 0 に向けて漂います — 減衰と開放性ボーナスが支配します．
`retaliation_event` が発火すると，影響を受けたコホートで fear がステップ的にジャンプし，
その後 20 ステップ前後で減衰して戻ります．
持続的な報復シナリオ（例：$p_{\text{retaliate}} = 0.2$）では，fear は実行のほとんどで高水準を保ち，
voice ロジットの $-\beta_f \cdot f$ の負項が支配して silence を上昇させ，
動機構成は `Defensive` 寄りに移ります．

## 11. 参考文献

- Kish-Gephart, J. J., Detert, J. R., Treviño, L. K., & Edmondson, A. C.
  (2009). Silenced by fear: The nature, sources, and consequences of fear at
  work. *Research in Organizational Behavior*, 29, 163–193.
- Detert, J. R., & Burris, E. R. (2007). Leadership behavior and employee
  voice: Is the door really open? *Academy of Management Journal*, 50(4),
  869–884.
