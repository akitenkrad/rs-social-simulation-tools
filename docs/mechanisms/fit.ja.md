[English](fit.md) | **日本語**

# 適合-満足度（`fit`）

> 従業員の職務満足度は，person–job fit と person–organisation fit の
> 加重ブレンドとして毎ステップ更新され，知覚された適合と態度的成果の間の
> 経験的なリンクを捉えます．
> **フェーズ：** Decision．**出典：** Kristof-Brown et al. (2005)．**種別：** empirical（$\rho_{\text{pj}}$，$\rho_{\text{po}}$）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`fit` は静的な適合ディメンション（person–job fit と person–organisation fit）を，
職務満足度という動的な態度的成果に変換します．毎ステップ，2つの適合スコアから目標満足度を計算し，
等しい重み（0.5/0.5）の移動平均を用いてその目標と従業員の以前の満足度をブレンドします．
このスムージングは満足度が不連続に跳躍することを防ぎ，
態度が知覚された適合に対して段階的に反応するという経験的観察をモデル化します．

下流のメカニズム — 特に `ocb`（知識共有）と `turnover`（離職確率）— が
`satisfaction` に強く依存するため，`fit` は HR ライフサイクルモジュールの
中心的な態度的ゲートウェイとして機能します．

## 2. 理論と出典

Kristof-Brown et al. (2005) は4つのディメンションにわたる適合の結果をメタ分析し，
職務満足度との相関として $\rho \approx 0.20$（person–job）と $\rho \approx 0.07$（person–organisation）を報告しています．
socsim はこれを，実行中の満足度値にブレンドされる線形合成目標として実装しています：

$$\text{sat}_{\text{new}} = \rho_{\text{pj}} \cdot \text{pj\_fit} + \rho_{\text{po}} \cdot \text{po\_fit}$$

$$\text{satisfaction} \leftarrow \operatorname{clip}_{[0,1]}\!\left(0.5\,\text{satisfaction} + 0.5\,\text{sat}_{\text{new}}\right)$$

- $\text{pj\_fit}$（`Employee.pj_fit`）— person–job fit $\in [0, 1]$；従業員のスキルや興味が職務にどれだけ合っているかを捉える．
- $\text{po\_fit}$（`Employee.po_fit`）— person–organisation fit $\in [0, 1]$；文化的・価値観的な整合性を捉える．
- $\rho_{\text{pj}}$（`rho_pj` = 0.20）— PJ 適合と満足度の経験的相関．
- $\rho_{\text{po}}$（`rho_po` = 0.07）— PO 適合と満足度の経験的相関．
- 移動平均の重み 0.5 は固定値；$\text{sat}_{\text{new}}$ が現在値からずれたとき，半減期は1ステップとなる．
- 結果は $[0, 1]$ にクランプされる．

## 3. データフロー

![fit data flow](../assets/mech-fit.svg)

このメカニズムは各従業員の `pj_fit`，`po_fit`，および以前の `satisfaction` を読み取り，
`new_sat` を計算して移動平均ブレンドを適用し，更新された `satisfaction` を書き戻します．
チームやワールドレベルの状態には触れません．

## 4. 6フェーズループにおける位置

3番目のフェーズである **Decision** で実行されます．`fit` をここに配置することで，
更新された `satisfaction` が他の Decision フェーズメカニズム（`turnover`，`hiring`）と，
同じステップ内の後続の Interaction フェーズメカニズム `ocb` で利用可能になります．

`fit` は Decision フェーズ内で `turnover` や `hiring` に対する順序制約はありませんが，
典型的なシナリオでは `turnover` が前のステップの満足度ではなく現ステップの満足度を使用できるよう，
`fit` を先に宣言します．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `Employee.pj_fit` | ✓ | | Person–job fit；採用時/シナリオ初期化時に設定される． |
| `Employee.po_fit` | ✓ | | Person–organisation fit；採用時/シナリオ初期化時に設定される． |
| `Employee.satisfaction` | ✓ | ✓ | 移動平均ブレンド；[0, 1] にクランプされる． |

## 6. 依存関係と順序制約

- **上流：** 同ステップ内で依存関係なし．`pj_fit` と `po_fit` は採用時に設定される外生的入力として扱われ，
  デフォルト構成では他のメカニズムによって更新されません．
- **下流（同ステップ）：**
  - `ocb`（Interaction）は知識貢献を計算するために `satisfaction` を読み取ります；
    Interaction は Decision の後に続くため，`fit` より後に実行されます．
  - `turnover`（Decision）は離職確率の駆動因として `satisfaction` を使用します；
    正確な同ステップ順序を確保するため，Decision フェーズ内で `fit` を `turnover` より前に宣言してください．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `rho_pj` | `0.20` | empirical（PJ 適合–満足度相関） | Kristof-Brown et al. (2005) |
| `rho_po` | `0.07` | empirical（PO 適合–満足度相関） | Kristof-Brown et al. (2005) |

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "fit"
phase = "decision"
[mechanism.params]
rho_pj = 0.20
rho_po = 0.07
```

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let fit = reg.build("fit", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(fit)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

乱数を**一切**使用しません．移動平均式は各従業員に独立に適用されます．
反復順序は結果に影響せず，このメカニズムは同じワールド状態に対して完全に決定論的です．

## 10. 期待される動作

`pj_fit` と `po_fit` が比較的高い分布（例：一様分布 [0.5, 1.0]）から引かれる職場では，
`satisfaction` は数ステップ以内に安定した水準に収束するはずです．
$\rho_{\text{pj}}$ と $\rho_{\text{po}}$ が比較的小さいため，目標 $\text{sat}_{\text{new}}$ は控えめ（高適合従業員で典型的に 0.07〜0.27），
満足度は主に自身の慣性によって駆動されます．
これは，高い初期満足度で採用された従業員が適合が平凡であっても満足度を維持し，
逆も同様であることを意味します —
満足度が部分的に安定した個人的傾向であるという経験的知見に合致しています（Staw et al., 1986）．
在職期間の延長と正の選択によって平均満足度が上昇するにつれ，離職率は低下するはずです．

## 11. 参考文献

- Kristof-Brown, A. L., Zimmerman, R. D., & Johnson, E. C. (2005). Consequences
  of individuals' fit at work: A meta-analysis of person–job, person–
  organization, person–group, and person–supervisor fit. *Personnel Psychology*,
  58(2), 281–342.
