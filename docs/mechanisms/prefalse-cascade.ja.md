[English](prefalse-cascade.md) | **日本語**

# 選好偽装カスケード（`prefalse_cascade`）

> 不動点まで反復：批判的な私的懸念を持つ silent エージェントは，
> 隣接 voice 比が個別の閾値を超えると Voice に反転し，これ以上反転が起こらなくなるまで
> 同期的なラウンドで繰り返されます．このティックで反転した総量が現在の母集団の
> `cascade_threshold`（デフォルト 5 %）を超えた場合，サイズと割合を含む `cascade` イベントを記録します．
> **フェーズ：** Interaction．**出典：** Kuran (1995)；Granovetter (1978)．**種別：** mixed（calibration scale + threshold）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`prefalse_cascade` は沈黙のスパイラルに対する破壊的な対抗力です．
公的表現チャネル上で Kuran (1995) の選好偽装カスケードを実装します．
私的に現状に反対し（`private_concern < 0`）公的に沈黙しているエージェントは，
自身のネットワーク隣接者の十分な割合が既に voicing している場合に Voice へ反転する機会を得ます．
反転は*そのエージェントの*隣接者の沈黙比を下げるため，
メカニズムは複数の反転を1つの Interaction フェーズに連鎖させることができます —
1ティックで境界にいる多くの silent dissenter を反転させうるカスケードです．

具体的には，apply メソッドは1つのパスで反転候補がなくなるまで同期パスを繰り返します．
各パスで次を行います．

1. 候補集合 — `Silence` かつ `private_concern < 0` のエージェント — を構築し，
   決定論のために `AgentId` でソートします．
2. 各候補について隣接 voice 比を計算します．その比がエージェントの `voice_threshold` を厳密に超えるなら，
   パス終了時の反転キューにエージェントを加えます．
3. キューされたすべての反転を同時に適用します（同期ラウンド）．反転時に `silence_motive` をクリアします．
4. ゼロ反転のパスになるまで繰り返します．

不動点到達後，反転した総量が `cascade_threshold` × `n_employees` を超えた場合，
絶対サイズと割合を含む `cascade` イベントを記録します．

## 2. 理論と出典

Kuran (1995) は選好偽装を導入しました：異論のコストが高いと知覚されると人々は公的に私的な見解を歪めて表現し，
集合的にこれが選好の真の分布を皆から隠します．知覚された公的合意のわずかな変化 — 数人の新たな voicer — が
多くの私的閾値を一気に超え，カスケードを解き放つことがあります．

Granovetter (1978) の閾値モデルはエージェントごとの定式化を与えます．
エージェント $i$ は，既に voicing している隣接者の割合が個別の $\theta_i$ を超えたときに silent から vocal へ反転します．
socsim はこれを直接採用し，候補集合を*silent dissenter*に限定して（現状に真に同意するエージェントが
巻き込まれないよう），不動点まで反復します．

$$\text{candidates}(t) = \{ i : \text{Expression}_i = \text{Silence} \wedge b_i < 0 \}$$

$$\rho_i^{V}(t) = \frac{|\{ j \in N(i) : \text{Expression}_j = \text{Voice}\}|}{|N(i)|}$$

各候補 $i$ について，$\rho_i^{V}(t) > \theta_i$ なら反転します．ここで $\theta_i$
（`Employee.voice_threshold`）はワールド構築時に
$\mathcal{N}(\text{THETA\_VOICE\_MEAN}, \text{THETA\_VOICE\_SD}^2)$ から抽出され $[0, 1]$ にクランプされた値です．
すべての反転を同時に適用し，反転がなくなるまで繰り返します．

$$\frac{\text{total flipped}}{n_{\text{employees}}} > \text{cascade\_threshold}$$

の場合に `cascade` イベントを記録します．

カスケード閾値（`cascade_threshold`，デフォルト `0.05`）は*イベント検出*の感度を設定するもので，
*カスケードのトリガー*ではありません．あるステップでの小規模な非ゼロの反転質量はイベントを記録しません．
母集団の少なくとも 5 % に触れる大規模カスケードのみが記録されます．

## 3. データフロー

すべてのエージェントの `Employee.expression`，`Employee.private_concern`，
`Employee.voice_threshold` と，エージェントごとの隣接 voice 比のための `SilenceWorld.network` の
隣接関係を読み取ります．反転した各エージェントに対して `Employee.expression = Voice` と
`Employee.silence_motive = None` を書き込みます．閾値を超えた場合，
反転した総数と母集団割合を含む `cascade` イベントを記録します．

## 4. 6フェーズループにおける位置

4番目のフェーズである **Interaction** で実行されます．Interaction 内では同梱シナリオが
`silence_spiral` を先に，`prefalse_cascade` を次に宣言しています．この順序は厳密に必須というよりは
慣例です：スパイラルは $\rho_i$（沈黙比スナップショット）を書き込み $\psi$ を侵食しますが，
カスケードは $\rho_i^{V}$（現在の表現から新鮮に計算した*voice*比）を読み取ります — 両者は互いに素なフィールドに書き込みます．
スパイラルを先に宣言することは設計の意図と合致します：毎ステップ，まずスパイラルが沈黙の圧力を定量化し，
次にカスケードが1度の機会でそれを破る試みを得るのです．

`org_performance`（Reward）よりも前に実行されることで，カスケードの反転はこのティックのマクロ集約に反映されます．
成功したカスケードは（次ティックではなく）即座に `silence_rate` と `climate_of_silence` を下げます．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `Employee.expression` | ✓ | ✓ | 各パスで読み，パス終了時に `Voice` に反転． |
| `Employee.private_concern` | ✓ | | 候補集合を dissenter（$b < 0$）に制限． |
| `Employee.voice_threshold` | ✓ | | 個別閾値 $\theta_i$． |
| `Employee.silence_motive` | | ✓ | 反転時に `None` に設定． |
| `SilenceWorld.network` | ✓ | | 隣接 voice 比クエリのための隣接リスト． |
| `ctx.recorder` | | ✓ | 反転質量が母集団の `cascade_threshold` を超えると `cascade` イベントを記録． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：** `voice_decision_rule`（または `voice_decision`）はカスケード実行前に
  このステップの `Expression` を書き込んでいる必要があります．カスケードはエージェントごとの決定の上に重ねる*修正*であり置き換えではありません．
- **下流（同ステップ）：** `org_performance`（Reward）は `silence_rate`，`voice_volume`，`motive_mix` イベントを
  計算するときカスケード後の `Expression` を読みます．`psafety_update`（PostStep）も，
  あるエージェントがこのステップで voicing したかどうかを判定するときに同じ表現を読みます．
- **ステップをまたぐ依存：** なし．カスケードは単一ステップ内で不動点反復されます．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `cascade_threshold` | `0.05` | tunable（イベント検出感度） | Kuran (1995) — 「mass cascade」の定義 |

エージェントごとの閾値 $\theta_i$ は*この*メカニズムのパラメータではありません．
`Employee.voice_threshold` に格納され，ワールド構築時に `THETA_VOICE_MEAN`（0.4）と `THETA_VOICE_SD`（0.15）から populate されます．
[`calibration.rs`](../../crates/socsim-packs/src/organizational_silence/calibration.rs) を参照．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "prefalse_cascade"
phase = "interaction"
[mechanism.params]
cascade_threshold = 0.05      # kuran:1995
```

`cascade_threshold` を上げると `cascade` イベントの発火頻度が下がります（「mass cascade」のより厳しい定義）．
下げれば小さな反転群も記録します．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("prefalse_cascade", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

乱数を**一切**使用しません．各内側パスは候補を `Vec` に集め，
そのベクトルを `AgentId` でソートし，各候補をその隣接 voice 比に対して評価し，
すべての反転をキューし，パス終了時に適用します — 同期ラウンドです．
パス内の反転は互いの更新を見ないため，同じ開始 `Expression` 分布の2つの実行は
`BTreeMap` 反復実装に関わらずビット同一の不動点結果を生成します．

## 10. 期待される動作

ベースラインシナリオではカスケードは頻繁に発火します．デフォルト母集団（40 エージェント）と
デフォルト閾値の組み合わせでは，少数の voicer でも境界にいる silent dissenter を閾値を超えて押し出し，
同期ラウンドがそれを増幅します．典型的なシード 0 実行ではほとんどのステップで `cascade` イベントが
0.10〜0.20 の範囲の反転割合で記録されます．したがってカスケードは，
スパイラルと fear 更新がステップ間で再構築する silence プールを，定常的に侵食する作用となります．

`cascade_threshold` を 0.5 に上げると記録イベントは稀になります．
背後の反転自体は引き続き起こりますが，最も大きなカスケードのみがイベントログに記される形になります．
`voice_threshold` 事前分布を上方へ調整（ソースで `THETA_VOICE_MEAN` を引き上げ）すると，
カスケードのトリガーが難しくなり長期の silence rate は高水準のまま保たれます．

## 11. 参考文献

- Granovetter, M. (1978). Threshold models of collective behavior.
  *American Journal of Sociology*, 83(6), 1420–1443.
- Kuran, T. (1995). *Private Truths, Public Lies: The Social Consequences
  of Preference Falsification*. Harvard University Press.
