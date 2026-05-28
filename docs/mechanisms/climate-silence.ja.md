[English](climate-silence.md) | **日本語**

# 沈黙の風土（`climate_silence`）

> ワールドレベルの `climate_of_silence` 集約 $C(t)$ — `Silence` かつ批判的私的懸念を持つエージェントの割合 — を
> 冪等にステップ終了時に再計算します．Reward フェーズと PostStep の変化をすべて反映する
> 正規の「公開値」ステップとして機能します．
> **フェーズ：** PostStep．**出典：** Morrison & Milliken (2000)．**種別：** aggregation．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`climate_silence` は，ワールドレベルの $C(t)$ フィールドがステップ終了時のエージェントロスターと整合することを
保証する記帳メカニズムです．純粋な集約器です — 乱数を抽出せず，パラメータを取らず，内部状態を持ちません — 
ただしパックの必須メンバーです．PostStep が走る時点で，カスケードが `org_performance`（Reward）が
$C(t)$ を計算した*後*にエージェントを `Voice` に反転させていることがあり，
本メカニズムは集約を再実行して公開値をステップ終了時のワールドに一致させます．

具体的には，実装は `SilenceWorld::recompute_macro_aggregates` を呼び出します．
これは現在の `employees` ロスターから `climate_of_silence` と `voice_volume` の両方を再計算します．

## 2. 理論と出典

Morrison & Milliken (2000) は*沈黙の風土*を — エージェントごとの属性ではなく — 組織状態として枠付けます．
それは批判的私的見解を保持しつつ公的に沈黙を保つ労働力の割合です．

$$C(t) = \frac{|\{ i : \text{Expression}_i = \text{Silence} \wedge b_i < 0 \}|}{|E(t)|}$$

ここで $|E(t)|$ は現在の在職従業員数です．分子は「隠された異論」コホート — 
別の環境であれば発言したであろうエージェント — です．
socsim はこの式から $C(t)$ を毎ステップ公開します．本メカニズムは正規の再計算ポイントです．

$C(t)$ の値は `org_performance` の Π(t) 式にも入ります（$\Pi(t) = K(t) \cdot (1 - C(t))$，
hr-lifecycle 変種については [`org_performance`](org-performance.ja.md) を，
silence パックの注については §3 を参照），そのため古い $C(t)$ は記録された $\Pi(t)$ に伝播します．
同梱シナリオではこのリスクは緩和されています．`org_performance`（Reward）は記録の前に
`recompute_macro_aggregates` を自身で呼び出し，`climate_silence` は PostStep メカニズムが落ち着いた後に再公開するからです — 
2点はカスケードと $\psi$ 更新を挟んでいます．

## 3. データフロー

すべてのエージェントの `Employee.expression` と `Employee.private_concern` を
（`recompute_macro_aggregates` 経由で）読み取ります．
`SilenceWorld.climate_of_silence` と `SilenceWorld.voice_volume` を書き込みます．イベントは記録しません．

## 4. 6フェーズループにおける位置

6番目のフェーズである **PostStep** で実行されます．配置により公開される $C(t)$ が
すべての Reward フェーズと PostStep の `Expression` 変化を反映することを保証します
（Interaction のカスケードはその時点までに既に落ち着いており，カウントに影響しうる
PostStep メカニズムは — 既に走り終えた — カスケード自身だけです）．
PostStep 内では，同梱シナリオはエージェントごとの状態が先に落ち着くように
`climate_silence` を `psafety_update` の後に宣言します．

`climate_silence` と `org_learning` の間に厳密な順序要件はありません．
後者は `Team.knowledge_stock` を読み，風土集約は読みません．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `Employee.expression` | ✓ | | `BTreeMap`（ソート）順でカウント． |
| `Employee.private_concern` | ✓ | | 分子を dissenter に制限． |
| `SilenceWorld.climate_of_silence` | | ✓ | $C(t)$ — `Silence` ∧ `private_concern < 0` の割合． |
| `SilenceWorld.voice_volume` | | ✓ | `recompute_macro_aggregates` の副作用として再計算． |

## 6. 依存関係と順序制約

- **上流（同ステップ）：** `Expression` を反転させる可能性のあるすべてのメカニズム — 
  `voice_decision_rule`（Decision）と `prefalse_cascade`（Interaction） — が実行済みである必要があります．
  PostStep ではこれは自動的に成立します．
- **下流（同ステップ）：** なし．`climate_silence` はティックの最終的な記帳ステップの1つです．
- **下流（次ステップ）：** `voice_decision_rule` は `SilenceWorld.climate_of_silence` を読み*ません*
  （代わりに $\rho_i$ をエージェントごとのスナップショットから読みます），
  そのため風土集約は純粋に観測チャネルでありフィードバックチャネルではありません．
  JSONL ログを消費する研究者は `climate_of_silence` メトリクス系列を介して読み取ります．

## 7. パラメータ

なし．`climate_silence` はチューナブルなパラメータを持たない純粋な集約メカニズムです．
`from_params` はすべての入力を無視します．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "climate_silence"
phase = "post_step"
```

`[mechanism.params]` ブロックは不要です．

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_packs::organizational_silence::{OrganizationalSilencePack, SilenceWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<SilenceWorld> = Registry::new();
OrganizationalSilencePack.register(&mut reg);

let m = reg.build("climate_silence", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(m)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

乱数を**一切**使用しません．再計算は `BTreeMap`（`AgentId` でソート）の順で `employees.values()` を走査し，
2つのスカラを数え，書き込みます．同じワールド状態に対する2つの実行は同一のワールド集約を生成します．

## 10. 期待される動作

`climate_silence` 自身は可視の軌跡を生成しません — `org_performance` で既に使われた集約を再公開するだけです．
PostStep での再計算が意味を持つのは，カスケードが Reward フェーズの再計算の*後*にエージェントを反転させた場合です．
このときの JSONL ログの `climate_of_silence` 系列は*Reward フェーズの*値（PostStep の集約再計算前のスナップショット）を記録しますが，
実行終了時に最終ワールド状態を読む外部消費者は PostStep の値を見ます．
したがってこのメカニズムは snapshot/resume と，ティック間で `sim.world()` を確認するライブラリモード呼び出し元に対する
*整合性*保証です．

## 11. 参考文献

- Morrison, E. W., & Milliken, F. J. (2000). Organizational silence: A
  barrier to change and development in a pluralistic world. *Academy of
  Management Review*, 25(4), 706–725.
