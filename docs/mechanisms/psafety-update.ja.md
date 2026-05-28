[English](psafety-update.md) | **日本語**

# 心理的安全性の更新（`psafety_update`）

> 知覚された心理的安全性に対するステップ終了時の Edmondson 更新．
> このステップで voicing したエージェントは $\psi$ を `psafety_learn` だけ上方に，
> 報復されたエージェントは同じ量だけ下方に押します．
> **フェーズ：** PostStep．**出典：** Edmondson (1999)．**種別：** empirical（$\psi$ 学習率）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`psafety_update` は知覚された心理的安全性のステップ内ループを閉じます．
Decision，Interaction，Reward フェーズがそのステップの行動を確定させた後に，
この PostStep メカニズムがすべての従業員を走査し，2つの二値シグナルに基づいて固定学習率で $\psi$ を調整します．

- エージェントはこのステップで voicing したか？観察された制裁なしの voicing は
  「発言は思ったより安全」というエージェントへの証拠です．
- エージェントはこのステップで報復されたか？`retaliation_this_step` に印付けられることは，
  voicing したかどうかにかかわらず「発言は罰せられる」直接の証拠です．

両効果は同じ学習率 `psafety_learn` を使います．2つのシグナルは独立で，
エージェントが同じステップで voicing し*かつ*報復されることもあり得ます．その場合，
正味の $\Delta \psi$ はゼロです．

更新された $\psi$ は次ステップの `voice_decision_rule` ロジットの $+\beta_\psi \cdot \psi_i$ 項に流れ込み，
Edmondson (1999) が言う「心理的安全性の経験ベースの更新」の経験的学習ループを完成させます．

## 2. 理論と出典

Edmondson (1999) のチーム学習研究は，心理的安全性を経験的に更新可能な信念として扱います．
否定的結果なしの発言の観察可能な事例ごとにチームメンバーの知覚される安全性が上昇し，
観察された制裁ごとにそれが低下します．socsim はこれを，共通の学習率を持つ
2シグナル加法更新として操作化します．

$$\Delta \psi_i = \underbrace{\eta_\psi \cdot \mathbf{1}[\text{Expression}_i = \text{Voice}]}_{\text{このステップで voicing}} - \underbrace{\eta_\psi \cdot \mathbf{1}[i \in \text{retaliation\_this\_step}]}_{\text{報復された}}$$

$$\psi_i \leftarrow \operatorname{clip}_{[0,1]}(\psi_i + \Delta \psi_i)$$

- $\eta_\psi$（`psafety_learn`，デフォルト 0.1．`calibration.rs` の定数 `PSAFETY_LEARN`）— 学習率．
  0.1 では，報復のない 10 回連続の voicing で $\psi$ を上限まで引き上げるのに十分です．
- $\mathbf{1}[\cdot]$ — 二値シグナルの指示関数．
- 結果は $[0, 1]$ にクランプされます．

これは `silence_spiral` のスパイラル駆動侵食とは異なる更新です．
スパイラルは行動に関係なく毎ステップ $\rho$ に比例して作用しますが，
本メカニズムはこのステップの明示的な行動–帰結のペアにのみ作用します．

## 3. データフロー

すべてのエージェントの `Employee.expression` と `SilenceWorld.retaliation_this_step`（O(1) 照会のため
`HashSet<AgentId>` に収集）を読み取ります．
すべてのエージェントについて更新された `Employee.psych_safety` を書き戻します．
報復バッファは**ここで**はクリアされません — それは次ステップ開始時の `retaliation_event` の責務です．

## 4. 6フェーズループにおける位置

6番目で最後のフェーズである **PostStep** で実行されます．2つの理由があります．

1. 更新は*このステップの結果に基づいて*行われます — voice 決定（Decision で設定）と
   報復バッファ（Environment で設定）．シグナルが正しいためには両方とも実行済みである必要があります．
2. 更新された $\psi$ は*次ステップの* voice 決定に見えることを意図しています．
   PostStep での実行はその意図と正確に一致します — 新しい $\psi$ 値は次ティックの Decision フェーズで
   `voice_decision_rule` が読む値です．

PostStep 内では `climate_silence` や `org_learning` との厳密な順序要件はありませんが，
同梱シナリオは慣例として `psafety_update` を先に宣言します．これにより
ワールド集約 `climate_silence` の再計算前にエージェントごとの状態が落ち着きます．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `SilenceWorld.retaliation_this_step` | ✓ | | O(1) 照会のため `HashSet<AgentId>` に構築． |
| `Employee.expression` | ✓ | | 「voicing した」シグナルを決定． |
| `Employee.psych_safety` | ✓ | ✓ | その場で更新．[0, 1] にクランプ． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：**
  - `voice_decision_rule`（Decision）または `voice_decision`（Decision）が `Expression` を設定している必要があります．
  - `retaliation_event`（Environment）はこのステップの `retaliation_this_step` を populate している必要があります．
  - `prefalse_cascade`（Interaction）もカスケードしたエージェントの一部の `Expression` を `Voice` に書き換えた可能性があります．
    カスケードの反転は $\psi$ 更新では voicing として数えられます．これは意図したモデル化（カスケードは観察可能な発言）です．
- **下流（次ステップ）：** `voice_decision_rule` はロジットの $+\beta_\psi$ 項として
  `Employee.psych_safety` を読みます．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `psafety_learn` | `0.1` | empirical（Edmondson 1999 学習率） | Edmondson (1999) — `PSAFETY_LEARN` |

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "psafety_update"
phase = "post_step"
[mechanism.params]
psafety_learn = 0.1           # edmondson:1999
```

`psafety_learn = 0.0` に設定すると $\psi$ は初期抽出値に固定されます．
スパイラルが `silence_spiral` 経由で侵食することは依然可能ですが，行動が信念を更新しなくなります．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("psafety_update", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

乱数を**一切**使用しません．2つのシグナルはステップの早い段階で設定された状態の純粋関数です．
反復は `BTreeMap` のソート順を使用し，各更新はエージェントごとの加算操作なので，
同じワールド状態に対する2つの実行は同一の $\psi$ ベクトルを生成します．

## 10. 期待される動作

報復がまれな安定 voice シナリオでは，$\psi$ は実行全体で 1.0 方向に漂います — 
voicing の成功ごとに信念が $\eta_\psi$ ずつ上方に押し上げられます．
漂流は上限クランプと `silence_spiral` のステップごとの侵食（ステップごとに最大 1 % ポイント差し引きうる）によって境界付けられます．

高報復シナリオでは非対称性が反転します．ほとんどのステップで $\Delta \psi$ がゼロまたは $-\eta_\psi$ となり，
$\psi$ は 0 に向かって傾きます．いったん下限に達すると $+\beta_\psi$ 項は voice ロジットに寄与しなくなり，
voicing の生存はシステムの上司／顕在性項頼みになります．

したがってこのメカニズムは `voice_decision_rule` の速いステップ内行動を補完する遅い学習ループです：
行動が表現を書き，表現が信念を更新し，信念が次ステップの行動を導きます．

## 11. 参考文献

- Edmondson, A. C. (1999). Psychological safety and learning behavior in
  work teams. *Administrative Science Quarterly*, 44(2), 350–383.
