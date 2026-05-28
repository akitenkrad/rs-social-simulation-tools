[English](org-learning.md) | **日本語**

# 組織学習（`org_learning`）

> Argyris (1977) のダブルループ学習をステップごとの二値スイッチとして表現したものです．
> 少なくとも1名の従業員が voicing し*かつ*課題顕在性が `salience_floor` 以上のときに，
> 各 voicer のチームには，そのチームの voicer 数に比例する知識バンプが付与されます．
> それ以外では知識ストック全体が小さな `decay_rate` で減衰します．
> これは「voice には価値がある」を，沈黙の風土の上で数値パフォーマンスシグナルに変換するメカニズムです．
> **フェーズ：** PostStep．**出典：** Argyris (1977)．**種別：** calibration（介入モデル）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`org_learning` は，組織的沈黙パックで voicing が `team.knowledge_stock` — ひいては記録される `org_performance` メトリクス — に
影響する唯一の経路を提供します．各 PostStep で2つを確認します．

1. このステップに voicer は存在するか？カスケード後にチームごとの `Voice` エージェントを数えます．
2. 課題は十分に顕在か？`SilenceWorld.issue_salience` を `salience_floor` と比較します．

**両方**の条件が成立する場合（`total_voicers > 0` AND `sigma > salience_floor`），
各チームの `knowledge_stock` が `learning_rate × team_voicers[i]` だけ増加します —
そのチームの voicer 寄与に比例するチームごとの累積です．
Argyris (1977) はこれを*ダブルループ学習*と呼びます：組織は固定された前提のもとで既存ルーチンを調整する（シングルループ）のではなく，
浮かび上がった懸念に応じてルーチンそのものを更新するのです．

いずれかの条件が成立しない場合 — 完全沈黙の風土，または浮かび上がった懸念に意味のない低顕在性期 — 
*すべての*知識ストックがチームごとに `decay_rate` で減衰します．減衰は更新されない暗黙知をモデル化します：
沈黙する組織で更新されなくなったルーチン，判断，非公式実践．

## 2. 理論と出典

Argyris (1977) はシングルループ学習（固定された前提のもとで行動を修正）とダブルループ学習
（前提自体を改訂）を区別し，voicing がない組織は古びた規範に対するシングルループ反復に閉じ込められると論じました．
socsim はこの類型をチームレベルの二値スイッチに集約し，顕在性ゲートにより浮上した懸念が学習をトリガするに十分な重みを持つ場合のみ学習が発火するようにします．

$$\Delta K_k(t) = \begin{cases} \eta_{\text{learn}} \cdot V_k(t) & V_{\text{total}}(t) > 0 \wedge \sigma(t) > \sigma_{\text{floor}} \\ -\, \delta \cdot K_k(t) & \text{それ以外} \end{cases}$$

$$K_k(t+1) = \max(0, K_k(t) + \Delta K_k(t))$$

- $K_k(t)$（`Team.knowledge_stock`）— 時刻 $t$ におけるチームの知識ストック．
- $V_k(t)$ — ステップ $t$ にチーム $k$ で `Expression = Voice` のエージェント数．
- $V_{\text{total}}(t)$ — 全チームを合計した voicer 数．
- $\eta_{\text{learn}}$（`learning_rate`，デフォルト 0.05）— on 分岐における voicer 1名あたりの知識増加．
- $\delta$（`decay_rate`，デフォルト 0.01）— off 分岐におけるステップごとの比例減衰（≈ 1 %/step）．
- $\sigma_{\text{floor}}$（`salience_floor`，デフォルト 0.3）— voicing が学習をトリガしない顕在性閾値．

on 分岐は*チーム局所の* voicer ごとの累積，off 分岐は*グローバルな比例*減衰
（すべてのチームが同じ比で減衰）です．この非対称性は，たとえ1チームだけが voicing していても，
そのステップは*すべての*チームでグローバル減衰が抑制されることを意味します — 
voice の巨視的効果を組織全体のシグナルに結びつける設計 §5.3 の意図的なモデル化選択です．

## 3. データフロー

すべてのエージェントの `Employee.expression` と `Employee.team`（`BTreeMap` 反復経由）を読み，
さらに `SilenceWorld.issue_salience` とチーム数を読みます．
すべてのチームの `Team.knowledge_stock` を書き込みます — voicer ごとに増加（on 分岐）または比例減衰（off 分岐）．

## 4. 6フェーズループにおける位置

6番目のフェーズである **PostStep** で実行されます．2つの理由があります．

1. voicer をカウントするのに使う表現は，このステップで*最終*であるべきです — カスケードが落ち着いた後．
   PostStep での実行はこれを保証します．
2. ここで書き込む知識ストックは*次ステップの* `org_performance`（Reward）が
   $\Pi(t) = K(t) \cdot (1 - C(t))$ を計算するときに読み取ります．PostStep で実行することで，
   書き込みが2つの `org_performance` 記録の間に配置され，メトリクス系列は遅延のない
   ステップごとの累積を示します．

PostStep 内では `psafety_update` や `climate_silence` に対する厳密な順序はありませんが，
同梱シナリオは慣例として `org_learning` を最後に宣言します．これによりすべてのエージェントごとの更新が
ワールド集約が動く前に落ち着きます．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `SilenceWorld.issue_salience` | ✓ | | `salience_floor` と比較． |
| `Employee.expression` | ✓ | | `BTreeMap` 反復順でチームごとにカウント． |
| `Employee.team` | ✓ | | チームごと voicer カウンタへのインデックス． |
| `Team.knowledge_stock` | ✓ | ✓ | on 分岐：voicer ごとに増加．off 分岐：比例減衰．0 にフロア． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：**
  - `voice_decision_rule`（Decision）または `voice_decision`（Decision）がエージェントごとの `Expression` を書き込みます．
  - `prefalse_cascade`（Interaction）はこれらの `Expression` の一部を `Voice` に書き換える可能性があります — 
    カスケードの反転は on 分岐の voicer 合計にカウントされ，これが意図したモデル化です．
  - `issue_salience`（Environment）がここで読まれる $\sigma$ を書き込みます．
- **下流（次ステップ）：**
  - `org_performance`（Reward）は `org_performance` と `knowledge_stock` メトリクスを計算するとき
    `total_knowledge_stock()` 経由で更新された `Team.knowledge_stock` を読みます．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `learning_rate` | `0.05` | calibration scale（チューナブル） | Argyris (1977) |
| `decay_rate` | `0.01` | calibration scale（チューナブル） | Argyris (1977) — 遅い暗黙知ドリフト |
| `salience_floor` | `0.3` | calibration scale（チューナブル） | `SIGMA_BASE` と一致し，on 分岐がショック後にのみ発火するように設定 |

3つのパラメータはいずれもメカニズムの `from_params` のローカルデフォルトに存在し，
経験的法則性というより*介入モデル*をエンコードするため `calibration.rs` で `pub const` として公開されていません．
デフォルトは [`OrganizationalSilencePack`](../packs/organizational-silence.ja.md#3-10個のメカニズム)
ページに記載されています．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "org_learning"
phase = "post_step"
[mechanism.params]
learning_rate  = 0.05
decay_rate     = 0.01
salience_floor = 0.3
```

`learning_rate = 0.0` を設定すると on 分岐が無効化されます（知識ストックは単調に減衰）．
`decay_rate = 0.0` に設定すると off 分岐が無効化されます（ストックは減少しません）．
`salience_floor` を 0 に下げると顕在性に関係なく任意の voice が on 分岐をトリガします．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("org_learning", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

乱数を**一切**使用しません．voicer カウントの走査は `BTreeMap` 反復（`AgentId` でソート）を使用し，
チーム更新の走査は挿入順で `Vec<Team>` を反復するため，同じワールド状態に対する2つの実行は同一の知識ストックを生成します．

## 10. 期待される動作

ベースラインシナリオでは，$t = 24$ で顕在性ショックが発火するといったん on 分岐がほとんどのステップでアクティブになります．
上がった $\sigma$ は `salience_floor = 0.3` を超え，カスケードが少なくとも数人の voicer を生み出し続けるためです．
`knowledge_stock` はショック後の点からおおむね線形に上昇し，
`org_performance` メトリクスもそれにつれて上昇します（$C$ が小さいため）．
ショック前は，あるステップが偶然 voicer 0 を生む場合に限り off 分岐が断続的に発火しますが，
同梱シナリオではカスケードが通常十分な voicer を作り出し on 分岐をアクティブに保ちます．

カスケードを無効化する（あるいはカスケード閾値を上げてカスケード反転が起きないようにする）と，
ショック前期間に長い off 分岐の連続が観察されることが多くなります：知識ストックはショックが voicing を復活させるまで定常的に減衰します．

## 11. 参考文献

- Argyris, C. (1977). Double loop learning in organizations. *Harvard
  Business Review*, 55(5), 115–125.
