[English](issue-salience.md) | **日本語**

# 課題顕在性（`issue_salience`）

> ワールドレベルの課題顕在性 $\sigma(t)$ は，トリガーイベントが固定の delta だけ
> 押し上げるまで，ベースライン水準で水平に保たれます．これは決定論的なステップ関数のショックであり，
> シナリオが実行途中の規制当局調査，告発，業績サプライズをシミュレートできるようにします．
> **フェーズ：** Environment．**出典：** scenario-driven（設計者が指定する $\sigma$ の軌跡）．**種別：** scenario-driven．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`issue_salience` は組織的沈黙パックの中で最も単純なメカニズムです．
1ステップに1回，ワールドレベルのスカラ $\sigma(t) \in [0, 1]$ を書き込みます．
これはすべての voice 決定メカニズムが読み取る値です．意図的に確率過程ではなく，
モデラーがいつ・どれだけ課題が顕在化するかを制御することで，
「トリガーイベント」のタイミングが再現可能かつアブレーション可能になります．

デフォルトでは，$\sigma(t)$ は `sigma_base`（0.3，軽度に可視な懸念）で維持され，
ステップ `shock_t`（24，慣例として60か月シナリオの中盤）で `shock_delta`（0.4）だけ跳ね上がって約 0.7 になります．
これは voice ロジットの `BETA_SALIENCE` 項を通じて，
voice と silence の境界にいる多くのエージェントの判断を反転させるのに十分な大きさです．
`shock_delta = 0` に設定するとショックは完全に無効になります．

## 2. 理論と出典

単一の経験的出典はありません．ステップ関数の形は設計書 §4 における意図的なモデル化の選択で，
「トリガーイベント後に何が変わるか？」を明快な前後比較に変えます．軌跡は次のとおりです．

$$\sigma(t) = \begin{cases} \sigma_{\text{base}} & t < t_{\text{shock}} \\ \sigma_{\text{base}} + \delta_{\text{shock}} & t \ge t_{\text{shock}} \end{cases}, \qquad \sigma(t) \leftarrow \operatorname{clip}_{[0,1]}(\sigma(t))$$

- $\sigma_{\text{base}}$（`sigma_base`，デフォルト 0.3）— ショック前のベースライン．
- $t_{\text{shock}}$（`shock_t`，デフォルト 24）— ショックが発火するステップ．
- $\delta_{\text{shock}}$（`shock_delta`，デフォルト 0.4）— 加算ジャンプの大きさ．
- 結果は $[0, 1]$ にクランプされ，公開される値は妥当な割合に保たれます．

別のシナリオでは（緩やかなランプ，矩形波などの）独自軌跡を，
`issue_salience` の代わりにカスタムメカニズムを登録して書き直すことができます．
ここで固定ショック形を既定とするのは，顕在性と沈黙の比較静学を解釈しやすくする最も単純な設計だからです．

## 3. データフロー

シミュレーションクロックを読み取り，上記ステップ関数から導いた値を
ワールドレベルのスカラ `SilenceWorld.issue_salience` に書き込みます．
エージェントごとの状態には触れず，イベントも記録しません．

## 4. 6フェーズループにおける位置

2番目のフェーズである **Environment** で実行されます．
顕在性の更新を Decision の前に置くことで，新しく書き込まれた $\sigma(t)$ が
同じステップ内のすべての voice 決定メカニズム（`voice_decision_rule` と LLM 変種の双方）から読み取られ，
ロジット／プロンプトに反映されることを保証します．

もう1つの Environment フェーズメカニズム `retaliation_event` との順序制約はありません．
両者は互いに素なワールドフィールドに書き込みます．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `ctx.clock.t()` | ✓ | | 現在のステップインデックス．ショックのゲートに使用． |
| `SilenceWorld.issue_salience` | | ✓ | $\sigma(t)$ に設定．[0, 1] にクランプ． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：** なし．クロックのみに依存します．
- **下流（同ステップ）：**
  - `voice_decision_rule`（Decision）はロジットの $\sigma$ 項として `issue_salience` を読み取るため，
    シナリオでは `issue_salience` を先に配置します．
  - `voice_decision`（Decision，LLM 変種）は同じ $\sigma$ をプロンプトに埋め込みます．
  - `org_learning`（PostStep）は `knowledge_stock` を増加させるか減衰させるかを判断する際に
    $\sigma$ を `salience_floor` と比較します．
- **ステップをまたぐ依存：** なし．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `sigma_base` | `0.3` | calibration scale（チューナブル） | 設計 §4 |
| `shock_t` | `24` | calibration scale（チューナブル） | 設計 §4 — 60か月シナリオの中盤 |
| `shock_delta` | `0.4` | calibration scale（チューナブル） | 設計 §4 — 境界エージェントを反転させる規模 |

デフォルトは [`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs) の
`SIGMA_BASE`，`SHOCK_T`，`SHOCK_DELTA` に存在します．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "issue_salience"
phase = "environment"
[mechanism.params]
sigma_base  = 0.3
shock_t     = 24
shock_delta = 0.4
```

`shock_delta = 0.0` に設定するとショックが無効化され，定常 $\sigma$ の対照条件を調べられます．
`shock_t` を下げるとトリガーが早く発火します．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("issue_salience", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

乱数を**一切**使用しません．軌跡はクロックと3つのパラメータの純粋関数なので，
同一パラメータの2つの実行はシードに関係なく同じ $\sigma(t)$ 列を書き込みます．

## 10. 期待される動作

ベースラインシナリオでは，最初の 20〜24 ステップで $\sigma = \sigma_{\text{base}}$ のもとで
`silence_rate` と `climate_of_silence` がほぼ定常水準に落ち着きます．
ステップ 24 でショックが発火した後，上がった $\sigma$ は `voice_decision_rule` を通じて流れ込み，
境界にあった多くのエージェントを Voice 側へ傾けます．典型的なベースラインでは続いて
カスケードメカニズムがその傾きを増幅し，silence rate は数ステップ以内に目に見えて下がります．
（ショックがスパイラルを上回る場合は）より低水準の沈黙均衡へ落ち着き，
（スパイラルと fear のフィードバックが支配する場合は）再び沈黙が増加します．
このショック後の分岐こそが設計の狙う比較静学です．

## 11. 参考文献

外部引用はありません．ステップ関数 $\sigma(t)$ の軌跡はモデル化の選択であり，
それを消費する `BETA_SALIENCE` 係数は Morrison (2014) に依拠し，
[`voice_decision_rule`](voice-decision-rule.ja.md) ページに記載されています．
