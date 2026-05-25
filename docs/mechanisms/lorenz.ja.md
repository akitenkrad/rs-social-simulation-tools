[English](lorenz.md) | **日本語**

# Lorenz（`lorenz`）

> 各エージェントは近傍の意見の平均を同化し，その後自分の立場を強化することで，極端な意見を増幅し分極を駆動します．
> **フェーズ：** Interaction．**出典：** Lorenz et al. (2021)．**種別：** opinion dynamics（assimilation + reinforcement）．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`lorenz` は，汎用の `socsim-mechanisms` クレートにおける意見ダイナミクスファミリーの
**同化＋分極**メンバーです．各エージェントは `[-1, 1]` のスカラー意見を持ちます．
1ステップに1回，**同期的**な更新を行います．まず全エージェントの意見をスナップショットし，
各エージェント `i` について2つの項を組み合わせます．

- **同化** — 受容領域 ε 内にある近傍の意見へ向かう平均ギャップに α を掛けたもの（有界信頼の引き）；
- **強化／分極** — `repulsion · sign(x_i) · |x_i|` という項で，エージェントの意見を*自分の現在の方向へ*
  さらに押し出し，すでにどれだけ極端かに応じてスケールします．

組み合わされた差分が `x_i` に加えられ，`[-1, 1]` にクランプされ，一括書き込みされます．
分極項は自己強化的です — 意見が極端なほど外側へ強く押されるため，集団は穏健なコンセンサスに落ち着くのではなく
`±1` の両極へ分裂する傾向があります．

このメカニズムは**ライブラリ専用**です．`socsim-core` の `ScalarOpinions` および `Neighbors`
能力トレイトを実装する任意のワールド上で動作します．これには**`ModulePack` がありません**
（シナリオ TOML 登録なし）．直接構築して `SimulationBuilder` に追加してください．

## 2. 理論と出典

Lorenz et al. (2021) は意見変化を3つの力の相互作用として捉えます．**同化**（同じ考えの相手への引き寄せ），
**強化**（エージェント自身の態度が現在の方向へ強まる），および**分極**（集団が両極へ引き離される）です．
強化が，このファミリーを純粋な有界信頼モデルと区別する鍵となる要素です — 反対の社会的圧力がなくても態度を急進化させます．

socsim はこれをステップ単位の意見更新として表現します．意見 $x_i$ と近傍メッセージ $\{m_j\}$ を持つ
エージェント `i` について，差分は領域内の近傍にわたる同化項と強化／分極項を足し合わせたものです．

$$
\Delta_i \;=\; \underbrace{\frac{\alpha}{|A_i|}\sum_{j \in A_i}(m_j - x_i)}_{\text{同化}}
\;+\; \underbrace{\rho\,\operatorname{sign}(x_i)\,|x_i|}_{\text{強化／分極}},
\qquad A_i = \{\, j : |m_j - x_i| < \varepsilon \,\}
$$

ここで $\varepsilon$ は受容の半幅，$\alpha$ は同化率，$\rho$ は分極強度（`repulsion` フィールド）です．
新しい意見は $x_i' = \operatorname{clamp}_{[-1, 1]}(x_i + \Delta_i)$ です．
領域内の近傍がいないとき同化項はゼロになりますが，強化項は依然として適用されます．
この数式は `mou2024` 再現実装の `lorenz_update` から逐語的に移植されています．

## 3. データフロー

![lorenz data flow](../assets/mech-lorenz.svg)

このメカニズムはステップ開始時のスナップショットから `opinion(i)` と近傍の意見
（`neighbors_of(i)` → `opinion(j)`，メッセージ `m_j` として使用）を読み取り，
領域内の同化ギャップを平均し，`|x_i|` でスケールした強化項を加え，クランプした新しい意見を
`set_opinion` で一括書き込みします．他の状態には触れません．

## 4. 6フェーズループにおける位置

エージェントが互いに影響を及ぼし合う **Interaction** フェーズで実行されます．
ここでは意見の変化そのものが相互作用です．

- `apply` 呼び出しの開始時に取得した全意見のスナップショットを読み取り，
  各エージェントの新しい意見を単一バッチで書き込みます — これにより更新は同期的（同時）になり，
  スケジューラの活性化順序に依存しません．
- 自分自身はメッセージ集合から除外されます（近傍 `j == i` はスキップ）；強化項はエージェント*自身*の
  スナップショット意見 `x_i` を用います．

スカラー意見のみを読み書きするため，同一の Interaction フェーズに意見を変更するメカニズムが2つあれば逐次的に合成されます．

## 5. 状態の読み書きコントラクト

| フィールド | 読み取り | 書き込み | 備考 |
|---|:--:|:--:|---|
| `opinion(i)`（`ScalarOpinions`） | ✓ | ✓ | ステップ開始時にスナップショット；`clamp(x_i + Δ)` で上書き．`sign(x_i)·|x_i|` を介して強化項も駆動． |
| `neighbors_of(i)`（`Neighbors`） | ✓ | | 同化項のためのメッセージ `m_j = x_j` の供給源（自分自身は除外）． |

## 6. 依存関係と順序制約

- **上流：** なし．`ScalarOpinions + Neighbors` を実装するワールドのみを必要とします．
  トポロジー（完全グラフ・リング・ネットワーク・格子）は `neighbors_of` を介したワールド側の関心事です．
- **下流：** オプションの [`ConvergenceMechanism`]（PostStep）と `max_abs_delta` ヘルパは利用できますが，
  強化項は意見を `±1` のクランプ境界へ駆動しそこに留める傾向があるため，
  単一の固定点ではなく分極した配置が通常の終状態です．ステップ予算がより明確な停止です．

## 7. パラメータ

| パラメータ | 型 | デフォルト | 意味 |
|---|---|---|---|
| `epsilon`（ε） | `f64` | `0.4` | 同化項の受容半幅：`|diff| < ε` ⇒ 同化． |
| `alpha`（α） | `f64` | `0.5` | 領域内の平均ギャップに適用する同化率． |
| `repulsion`（ρ） | `f64` | `0.2` | 強化項 `ρ·sign(x_i)·|x_i|` の分極強度． |

これらは経験的相関ではなく，調整可能な行動スケールです．ModulePack がないため，
シナリオ TOML のパラメータブロックもありません．3つのフィールドはすべてコンストラクタ引数です．

## 8. 適用方法

このメカニズムは**ライブラリモード専用**です — シナリオ TOML 登録はありません．
`ScalarOpinions + Neighbors` を実装するワールドを用意し，メカニズムを構築して
`SimulationBuilder` に追加します．（ワールドのボイラープレートは
[Hegselmann–Krause の例](hegselmann-krause.ja.md#8-適用方法)と同一です．）

```rust
use socsim_mechanisms::LorenzMechanism;
use socsim_engine::{SequentialScheduler, SimulationBuilder};

// ε = 0.4 受容，α = 0.5 同化，repulsion = 0.2 分極．
let lorenz = LorenzMechanism::new(0.4, 0.5, 0.2);

let mut sim = SimulationBuilder::new(world) // world: ScalarOpinions + Neighbors
    .scheduler(Box::new(SequentialScheduler))
    .seed(42)
    .add_mechanism(lorenz)
    .build();
sim.run()?;
```

`repulsion` を上げると強化が支配的になり（より速く鋭い分極），0 にすると純粋な同化
（有界信頼に似た）ダイナミクスに戻ります．

## 9. 決定論性と RNG

**決定論的**です．更新は固定スナップショットを読み取り，固定バッチを書き込むため，
結果は順序非依存で，同じワールド状態に対して再現可能です — `ctx.rng` には触れません．
（ランダムな初期意見などの確率性は，メカニズムではなくワールドに存在します．）

## 10. 期待される動作

レジームは，同化（α, ε）と強化（`repulsion`）の釣り合いによって決まります．

- **同化支配**（大きな ε，小さな `repulsion`）：集団は有界信頼モデルのようにコンセンサスへ収束します．
- **強化支配**（小さな ε，大きな `repulsion`）：自己強化項が同化が引き寄せるよりも速く意見を外側へ押すため，
  集団は **`±1` の両極へ分極**し，そこでクランプが固定します．

強化は `|x_i|` でスケールするため，中央付近から始まるエージェントは最初はゆっくり漂い，
外側へ移動するにつれて加速します — このモデルが捉える急進化ダイナミクスの特徴です．

## 11. 参考文献

- Lorenz, J., Neumann, M., & Schröder, T. (2021). Individual attitude change and
  societal dynamics: Computational experiments with psychological theories.
  *Psychological Review*, 128(4), 623–642.
- Mou, X., et al. (2024). Opinion-dynamics agent-based models with assimilation,
  reinforcement, and polarisation mechanisms (the `mou2024` reference port).
