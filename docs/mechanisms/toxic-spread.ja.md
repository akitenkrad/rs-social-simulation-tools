[English](toxic-spread.md) | **日本語**

# 有害伝播 (`toxic_spread`)

> 有害な従業員が，経験的に較正された感染確率に従い，ネットワークのエッジを通じて非有害な隣接者を感染させる．
> **フェーズ:** Interaction．**出典:** Housman & Minor (2015)．**種別:** 経験的（`p_spread`）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`toxic_spread` は，職場の毒性を社会的伝染のプロセスとしてモデル化する．有害な従業員は，否定的な交流を繰り返すことで，隣接する非有害な同僚を有害な従業員へと変えていく．このメカニズムは従業員間を結ぶ Watts–Strogatz ソーシャルネットワーク上で毒性を伝播させ，その際に，Housman & Minor (2015) が報告した経験的比率へと較正したエッジごとの感染確率を適用する．

毒性は組織に間接的な影響を及ぼす．有害な従業員は（fit および満足度ダイナミクスを通じて）周囲の満足度を下げ，離職確率を高めるため，`toxic_spread` は労働力の不安定性を増幅する重要な要因となる．採用時に設定されるベースライン有病率 `P_TOXIC = 0.04` は，抑制しなければ `toxic_spread` によって時間とともに上昇しうる．

## 2. 理論と出典

Housman & Minor (2015) は大規模サービス企業における有害労働者のコストを定量化し，直接的な生産性損失と強いピア感染効果の両方を示した．socsim はこの感染を，単純なネットワーク拡散モデルへと落とし込んでいる．すなわち，有害な従業員（`AgentId` でソート）それぞれについて，非有害な隣接者（`AgentId` でソート）が互いに独立に確率 $p_{\text{spread}}$ で有害になる．

$$P(\text{non-toxic neighbour becomes toxic}) = p_{\text{spread}}$$

感染の判定はいったんまとめて収集してから一括で適用するため，同じステップ内で新たに感染した従業員がその場で感染源になることはない．

- `p_spread`（$p_{\text{spread}} = 0.46$）— 経験的なエッジごとの月次感染確率（Housman & Minor 2015）．
- ネットワークのデフォルトは Watts–Strogatz（`k = 4`, $\beta = 0.1$）で，各従業員におよそ4人の隣接者を持たせる．

## 3. データフロー

![toxic_spread data flow](../assets/mech-toxic-spread.svg)

このメカニズムは `Employee.is_toxic` とネットワークの隣接リストを読み取り，感染が起こりうるエッジごとに `ctx.rng` から1回サンプリングして，新たに感染した従業員に `Employee.is_toxic = true` を書き込む．それ以外の状態は変更しない．

## 4. 6フェーズループ内での位置

4番目のフェーズである **Interaction** で，`peer_effect` や `ocb` と並んで実行される．Interaction 内の順序が問題になるのは，同じ Interaction フェーズ内に `is_toxic` を読み取るメカニズムがあり，それより `toxic_spread` を**先に**宣言する必要がある場合だけである．デフォルトパックでは Interaction フェーズのメカニズムは `is_toxic` を読み取らないため，宣言順序は柔軟に決められる．

毒性は直接的な社会的接触を通じて伝播し，ピア生産性のスピルオーバーと同じ概念的な枠組みを共有することから，Interaction に配置するのが妥当である．

## 5. 状態読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `Employee.is_toxic` | ✓ | ✓ | 感染源；新たに感染した場合は `true` に書き込まれる． |
| `HrWorld.network`（隣接） | ✓ | | Watts–Strogatz；有害な従業員ごとに隣接者を参照する． |

## 6. 依存関係と順序制約

- **上流** `hiring`（Decision）が採用時に，集団全体のベースラインである `P_TOXIC = 0.04` に基づいて `is_toxic` を設定する．ネットワークさえ初期化されていれば，同一ステップ内の依存関係はない．
- **下流** 同じステップの残りのフェーズで `is_toxic` を読み取るメカニズムはない．毒性の影響は，`fit`（`satisfaction` を更新する）と `turnover`（`satisfaction` と `embeddedness` を参照する）を介して，**次の**ステップの **Decision** フェーズで現れる．つまり，このステップで伝播した毒性が効いてくるのは次のステップ以降である．

## 7. パラメータ

| パラメータキー | デフォルト | 種別 | 出典 |
|---|---|---|---|
| `p_spread` | `0.46` | 経験的（エッジごとの月次感染率） | Housman & Minor (2015) |

採用時のベースライン有病率 `P_TOXIC = 0.04` を設定するのは，このメカニズムではなく `hiring` である．調整したい場合は `hiring` メカニズムの `p_toxic` パラメータを使う．

## 8. 適用方法

### シナリオ TOML

```toml
[[mechanism]]
name  = "toxic_spread"
phase = "interaction"
[mechanism.params]
p_spread = 0.46
```

### ライブラリモード

```rust
use socsim_config::{Registry, Params, ModulePack};
use socsim_hr_lifecycle::{HrLifecyclePack, HrWorld};
use socsim_engine::{RandomActivationScheduler, SimulationBuilder};

let mut reg: Registry<HrWorld> = Registry::new();
HrLifecyclePack.register(&mut reg);

let ts = reg.build("toxic_spread", &Params::empty())?;
let mut sim = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(42)
    .add_mechanism(ts)
    .build();
sim.run()?;
```

## 9. 決定論性と RNG

`ctx.rng` から乱数を引く — 感染が起こりうる（有害な感染源と非有害な隣接者を結ぶ）エッジごとに `gen::<f64>()` を1回呼び出す．ビット単位の再現性は，次の手順で保証している．

1. 有害な感染源の従業員を，走査に先立って **AgentId でソート**して収集する．
2. 各感染源について，隣接者を RNG を参照する前に **AgentId でソート**する．

この辞書順の整列により，RNG の消費シーケンスは内部マップの走査順序に左右されず一定になり，同一シードでの再現可能な実行が保証される．

### 汎用 `si_contagion` カーネルとの関係

`toxic_spread` は SI の変種であり，`socsim-mechanisms` の汎用
[`si_contagion`](si-contagion.ja.md) と概念的に重なるが，RNG の引き方が両者で
非互換であり，統合するとこの実証的にキャリブレーションされたメカニズムの
シード付き軌道が変わってしまうため，**あえて共有カーネル上には構築していない**．

- **走査の起点．** `toxic_spread` は *感染源起点*（各有害従業員 → その隣接者）で
  走査するのに対し，`si_contagion` は *対象起点*（各非アクティブ → そのアクティブ
  隣接者）で走査する．
- **break の有無．** `toxic_spread` は（感染源, 非有害隣接者）エッジごとに
  Bernoulli を引き，**break しない**（*k* 個の有害源に隣接する者は *k* 回引く）．
  一方 `si_contagion` は **最初の成功で break** する．
- **順序の基準．** `toxic_spread` はソート済み `AgentId`，`si_contagion` は
  スケジューラの `ctx.agent_order` を用いる．

これらの違いにより，忠実な委譲は RNG ドローの回数と順序を変え，決定論的な
シード付きテストを壊してしまう．したがって `toxic_spread` は HR ローカルの
メカニズムのまま据え置き，カーネルが将来「感染源起点・break なし」の変種を
持った場合の *将来候補* として記録する．同じ理由から `HrWorld` は
`BinaryState` / `Neighbors` 能力トレイトを実装しない．

## 10. 期待される動作

`P_TOXIC = 0.04` および `p_spread = 0.46` の条件では，有害有病率は初期の 4% から上昇し，ネットワーク構造と有害従業員の離職率に応じて決まるより高い均衡へと向かう．`k = 4` の Watts–Strogatz ネットワークでは，通常 24〜48 ステップにわたって毒性が緩やかに上昇したのち安定するか，あるいは有害なノードを吐き出す離職カスケードを引き起こす．`hiring` による補充を無効にしたまま `toxic_spread` を実行するとネットワークの大部分が感染するが，`p_toxic = 0.04` で採用を再び有効にすると有害比率は次第に薄まっていく．

## 11. 参考文献

- Housman, M., & Minor, D. (2015). Toxic workers. *Harvard Business School
  Working Paper* 16-057.
