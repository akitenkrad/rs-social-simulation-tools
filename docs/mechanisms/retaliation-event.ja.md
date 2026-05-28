[English](retaliation-event.md) | **日本語**

# 報復イベント（`retaliation_event`）

> ステップあたり低確率のショック．確率 $p_{\text{retaliate}}$ で，最近の voicer 1名が標的に選ばれ，
> その voicer のすべての隣接者（および voicer 本人）がこのステップで報復されたと印付けられます．
> この印は，同ティック内で fear 更新と心理的安全性更新に流し込まれます．
> **フェーズ：** Environment．**出典：** Kish-Gephart et al. (2009)．**種別：** stochastic．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`retaliation_event` は，voice の負の帰結をモデルに注入する突発ショックチャネルです．
毎ステップまず前ステップの報復リストを消去し，次に確率 $p_{\text{retaliate}}$ で単一のイベントを発火します．
イベントが発火すると，現在の voicer から標的を1名選び（voicer がいなければ任意のエージェントにフォールバック），
その標的のネットワーク隣接者と標的自身をこのステップで報復されたと印付け，
標的 id と影響を受けたエージェント数を含む `retaliation` イベントを記録します．

印は一時バッファ `SilenceWorld.retaliation_this_step` に置かれます．
これを `fear_appraisal` が同じ Decision フェーズで読み取り fear を増加させ，
`psafety_update` が PostStep で読み取り知覚された心理的安全性を下方へ押します．
このバッファは突発ショックとエージェントレベルの状態更新との間の正規の受け渡し点です．

## 2. 理論と出典

Kish-Gephart, Detert, Treviño & Edmondson (2009) は，
報復 — 発言に対する公式または非公式の制裁 — が組織における沈黙の中心的な駆動因であるとする
経験的証拠をレビューしています．報復は*ステップあたり稀だが発生時には顕著*です．
同僚が発言に対して罰せられたという一度の観察が，職場の発言への恐怖に数か月分の刻印を残しうるのです．
socsim はこれを，低い確率（`calibration.rs` で $p_{\text{retaliate}} = 0.05$）のステップごとベルヌーイ抽出と，
ネットワーク局所的な影響半径（標的の直接隣接者）でミラーします．

$$\Pr[\text{イベントが } t \text{ で発火}] = p_{\text{retaliate}}, \qquad \text{affected}(t) = \{ \text{target} \} \cup N(\text{target})$$

標的は *現在の voicer*（公的な異論が組織から観察可能なコホート）から一様抽出します．
このステップで誰も voicing していない場合（例えば実行初期や深い沈黙均衡），
メカニズムは全エージェント母集団からの一様抽出にフォールバックします．
これは「まだ公的に voicing されていない知覚された異論」を標的とする，
より散漫なケースをモデル化します．

影響を受けたリストはその後重複削除とソートが行われます — `affected.sort(); affected.dedup();` —
これにより `fear_appraisal` の下流での `HashSet` 構築は，
ネットワークの隣接リストがどのように確保されたかに関係なく順序非依存になります．

## 3. データフロー

`SilenceWorld.employees`（現在の voicer にフィルタ）と `SilenceWorld.network`（標的の隣接者用）を読み取ります．
`SilenceWorld.retaliation_this_step` に重複削除とソート済みの影響リストを書き込み，
イベント発火時には `retaliation` イベントを記録します．
このリストは同ステップの Decision フェーズで `fear_appraisal` が読み，
PostStep で `psafety_update` が読みます．

## 4. 6フェーズループにおける位置

2番目のフェーズである **Environment** で実行されます．
Decision の前に配置することで，まさに同じステップ内で `fear_appraisal` が
新たに書き込まれた `retaliation_this_step` を読み取れることを保証します．
カスケードや voice 決定よりも前に実行されるため，増加した fear はエージェントの選択に
（次ティックではなく）即座にフィードバックされます．

Environment 内のもう1つのメカニズム `issue_salience` との順序制約はありません．
両者は互いに素なワールドフィールドに書き込みます．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `ctx.clock.t()` | ✓ | | 記録イベントのタイムスタンプ． |
| `ctx.rng` | ✓ | | 1回のベルヌーイ抽出．イベント発火時は1回の一様インデックス抽出も追加． |
| `SilenceWorld.employees` | ✓ | | 候補集合のため現在 `Voice` のエージェントにフィルタ． |
| `SilenceWorld.agent_ids()` | ✓ | | voicer がいない場合のフォールバック候補集合． |
| `SilenceWorld.network` | ✓ | | 標的の隣接者のための隣接リスト． |
| `SilenceWorld.retaliation_this_step` | | ✓ | 毎ステップ消去．発火時のみ再書き込み． |
| `ctx.recorder` | | ✓ | `{target, n_affected}` を持つ `retaliation` イベントを記録． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：** なし．バッファクリア後 Environment 内で最初に実行されます．
- **下流（同ステップ）：**
  - `fear_appraisal`（Decision）は影響を受けた各エージェントの fear を増加させるためにバッファを読みます．
  - `psafety_update`（PostStep）は影響を受けた各エージェントの $\psi$ を下方に押すためにバッファを読みます．
- **ステップをまたぐ依存：** バッファは毎ステップ上書きされ蓄積されないため，
  報復はステップをまたいで漏れません．代わりに報復の持続的な刻印は，
  更新された `Employee.fear` と `psych_safety` フィールドに保持されます．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `p_retaliate` | `0.05` | empirical（ステップあたり報復確率） | Kish-Gephart et al. (2009) |

デフォルトは [`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs) の
`P_RETALIATE` にあり，doc コメントは Kish-Gephart et al. (2009) を引用しています．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "retaliation_event"
phase = "environment"
[mechanism.params]
p_retaliate = 0.05            # kish-gephart:2009
```

`p_retaliate = 0.0` を設定するとショックチャネルが完全に無効化されます
（スパイラルや IVT メカニズムを切り分けたいときに有用なアブレーション）．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("retaliation_event", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

イベント発火時に**2つ**の RNG 値を抽出します．ベルヌーイゲート用の `f64` 1つと，
標的選択のための `gen_range(0..candidates.len())` 1つです．
抽出シーケンスを実行間で再現可能に保つため，候補集合は RNG 呼び出しの前に
`SilenceWorld.employees`（`BTreeMap` なので反復は既に `AgentId` 順）を反復して構築されます．
フォールバックの `agent_ids()` もワールドヘルパー内部でソートされます．
標的を選んだ後，影響を受けたリストはバッファへ書き込まれる前にソートと重複削除が行われるため，
下流で `HashSet` を構築するメカニズムでさえも順序非依存です．

## 10. 期待される動作

同梱の `org_silence_baseline.toml`（60ステップ，シード 0）では，イベントが実行全体で
おおむね 2〜4 回発火します — `p_retaliate = 0.05` のベルヌーイ率と整合する頻度です．
各発火はオーダーとして 6〜8 名（標的本人＋約 6 名の Watts–Strogatz 隣接者）を巻き込みます．
影響を受けたコホートでは即座に fear が上昇し，silence rate も短期間上がります．
心理的安全性はその後数ステップで下方へ漂います．多シードで `p_retaliate` を上げると，
長期の `climate_of_silence` が押し上げられ，定常状態の動機構成も `Defensive` 寄りに移ります．

## 11. 参考文献

- Kish-Gephart, J. J., Detert, J. R., Treviño, L. K., & Edmondson, A. C.
  (2009). Silenced by fear: The nature, sources, and consequences of fear at
  work. *Research in Organizational Behavior*, 29, 163–193.
