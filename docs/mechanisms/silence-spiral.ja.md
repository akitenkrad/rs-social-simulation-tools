[English](silence-spiral.md) | **日本語**

# 沈黙のスパイラル（`silence_spiral`）

> Interaction フェーズ末で各従業員の隣接沈黙比 $\rho_i$ をスナップショットし，
> その比に比例する小さな下方への押し下げを知覚された心理的安全性に適用します — 
> Noelle-Neumann (1974) のスパイラルを，ステップごとの $\psi$ 侵食として表現したものです．
> **フェーズ：** Interaction．**出典：** Noelle-Neumann (1974)．**種別：** empirical（$\epsilon$ スパイラル振幅）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`silence_spiral` は沈黙のスパイラル効果をステップ間で運ぶ運搬役です．
Decision フェーズが各エージェントの新しい `Expression` を設定した後の Interaction フェーズで実行され，
すべてのエージェントについて2つのことを連動して行います．

1. **$\rho_i$ をスナップショット** — エージェントのネットワーク隣接者のうち現在 `Silence` の割合 — し，
   それをエージェントのステップごとフィールド `Employee.neighbor_silence_ratio` に書き込みます．
   このスナップショットは*次ステップの* `voice_decision_rule` がロジットの $-\beta_C \cdot \rho_i$ 項として消費する値です．
2. $\rho_i$ に比例する小さな量だけ **$\psi_i$ を侵食**：局所沈黙比が高ければ知覚された心理的安全性が下方に押されます．

最初のアクションは，ステップ内の Expression 状態をステップをまたぐシグナルに変換します．
2番目は，スパイラルが時間とともに voicing を漸進的に困難にする*メカニズム*そのものです．

## 2. 理論と出典

Noelle-Neumann (1974) は沈黙のスパイラルを，自身を沈黙（または異論）の少数派と知覚するエージェントが
voicing を漸進的に控えるようになる動学として枠付けます．
局所知覚の操作化は隣接沈黙比です．

$$\rho_i(t) = \frac{|\{ j \in N(i) : \text{Expression}_j = \text{Silence}\}|}{|N(i)|}$$

socsim は Interaction フェーズ末で $\rho_i$ をエージェントごとのフィールドに書き込み，
$\psi$ に小さなステップごと侵食を適用します．

$$\psi_i \leftarrow \operatorname{clip}_{[0,1]}\!\left(\psi_i - \epsilon \cdot \rho_i \cdot 0.05\right)$$

- $\epsilon$（`epsilon`，デフォルト 0.25．`calibration.rs` の定数 `EPSILON_SPIRAL`）— スパイラル知覚の振幅．
- 係数 `0.05` は固定のステップごとスケールで，$\epsilon$ と掛けると最大侵食量は
  `0.25 · 1.0 · 0.05 = 0.0125`（隣接者がすべて沈黙のときステップあたり $\psi$ が 1.25 % ポイント低下）となります．
- 結果は $[0, 1]$ にクランプされます．

エージェントに隣接者がいない場合，`neighbor_silence_ratio(id)` は 0 を返し
（ヘルパーがゼロ除算を防ぐ），そのステップ中はエージェントの $\psi$ は変更されません．

## 3. データフロー

すべてのエージェントについて `Employee.expression` を読み，
`SilenceWorld.network` の隣接リストを読んで $\rho_i$ を計算します．
新しい $\rho_i$ を `Employee.neighbor_silence_ratio` に書き戻し，
すべてのエージェントについて更新された `Employee.psych_safety` を書き戻します．イベントは記録しません．

## 4. 6フェーズループにおける位置

4番目のフェーズである **Interaction** で実行されます．2つの順序不変条件が適用されます．

1. Decision フェーズの**後**．$\rho_i$ スナップショットが前ステップではなく
   このステップの新たに抽出された `Expression` を反映するため．
2. `prefalse_cascade`（同じく Interaction）の**前**．カスケードはエージェントの `Expression` と
   `voice_threshold` を読みますが $\rho_i$ スナップショットは読まないため，
   両者の間で $\rho_i$ が変わってもカスケードからは見えません．
   同梱シナリオはこの順序を明示するため `silence_spiral` を `prefalse_cascade` より先に宣言します．

スナップショットはその後変更されずに次ステップの Decision フェーズへ運ばれ，
`voice_decision_rule` が `Employee.neighbor_silence_ratio` 経由で読み取ります．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `Employee.expression` | ✓ | | `BTreeMap` 順ですべての従業員について読む．隣接者内の `Silence` 数が $\rho_i$ を生む． |
| `SilenceWorld.network` | ✓ | | すべてのエージェントの隣接リスト． |
| `Employee.neighbor_silence_ratio` | | ✓ | ステップごとスナップショット．その場で上書き． |
| `Employee.psych_safety` | ✓ | ✓ | $\epsilon \cdot \rho \cdot 0.05$ だけ侵食．[0, 1] にクランプ． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：** `voice_decision_rule`（または `voice_decision`）は本メカニズムより前に
  このステップの `Expression` を書き込んでいる必要があります．さもなくば $\rho_i$ は前ステップの沈黙パターンを反映します．
- **下流（同ステップ）：** `prefalse_cascade` は `Expression`，`voice_threshold`，
  `private_concern` を読みますが，これらはどれも本メカニズムが書きません．
  2つの Interaction メカニズムは書き込み集合が独立です．
- **下流（次ステップ）：** `voice_decision_rule` はロジットの $-\beta_C \cdot \rho_i$ 項として
  `Employee.neighbor_silence_ratio` を読みます．スナップショットはスパイラル効果の正規のステップをまたぐ運搬役です．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `epsilon` | `0.25` | empirical（スパイラル知覚の振幅） | Noelle-Neumann (1974) — `EPSILON_SPIRAL` |

式内のステップごと係数 `0.05` はコンパイル時定数であり，
シナリオパラメータとして公開されていません．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "silence_spiral"
phase = "interaction"
[mechanism.params]
epsilon = 0.25                # noelle-neumann:1974
```

`epsilon = 0.0` を設定すると $\psi$ 侵食は無効化されますが，
$\rho_i$ スナップショットはそのまま保たれるので，`voice_decision_rule` は引き続き
$-\beta_C$ 項で読み取れます．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("silence_spiral", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

乱数を**一切**使用しません．順序非依存のカウントである $\rho_i$ の計算で
決定論的な f64 累積を保証するため，メカニズムは `employees.keys()` を集めて結果の `Vec<AgentId>` をソートし，
ソート済みリストを走査します．事前計算した `(id, rho)` ペアもその同じソート順で適用されるため，
`BTreeMap` 反復順の実装変更があっても出力は影響を受けません．

## 10. 期待される動作

ほとんどのエージェントが voicing している場合，$\rho_i$ はすべてのエージェントで 0 付近にとどまり，
$\psi$ はせいぜい marginal にしか漂いません — スパイラルはほとんど効果を発揮しません．
silence が局所的に密になると，それらの近傍で $\rho_i$ が 1 に近づき，
$\psi$ はステップごとに最大で 1 % ポイント侵食され，これが次ステップの voice ロジットに
$\beta_\psi \cdot \psi_i$ 項（直接）と $-\beta_C \cdot \rho_i$ 項（スナップショット経由）の双方を通じてフィードバックします．
両者が合わさって設計の狙う暴走スパイラルが生まれます — 沈黙の地域はより悪い知覚風土となり，
voicing がより困難となり，地域が沈黙のまま留まります．

パック内でこのメカニズムに対する唯一の対抗力は `prefalse_cascade` です．
隣接 voice 比がエージェントの個別閾値を超えれば，1つの Interaction フェーズでスパイラルを破壊できます．

## 11. 参考文献

- Noelle-Neumann, E. (1974). The spiral of silence: A theory of public
  opinion. *Journal of Communication*, 24(2), 43–51.
