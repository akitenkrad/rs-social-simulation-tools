[English](moran-update.md) | **日本語**

# モラン過程（`moran_update`）

> 1 ステップに出生と死亡を 1 回行います．適応度に比例して選んだ親が，一様に選んだ 1 人に自分の行動を渡します．変異を加えることもできます．
> **フェーズ：** Reward．**出典：** Moran（1958）；Nowak, Sasaki, Taylor & Fudenberg（2004）．**種別：** selection (birth–death)．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`moran_update` は [`socsim-games`](../games.ja.md#集団動学) の 4 つの集団動学の 1 つです．
`assemble_with` の引数 `dynamics` に `MoranUpdate::new(fitness, mutation, update)?` を渡すと登録され，
全エージェントが **Held** で**トポロジーを持たない** World で動きます．
ステップの対戦が終わった後，出生と死亡をちょうど 1 回行います．

1. 設定した `Fitness` で，各エージェントの利得を適応度に写します．
2. **親**を，全エージェントから適応度に比例した確率で引きます．
3. **死ぬ**エージェントを，全エージェント（親を含む）から一様に引きます．
4. 死ぬエージェントの `held` に親の保持行動を書きます．ただし確率 `mutation` で，共通の行動の集合から一様に引いた行動を書きます．

ほかのエージェントは行動を保ちます．1 ステップで変わるのは高々 1 人なので，模倣の動学が 1 ステップで行うことに，モラン過程はおよそ $N$ ステップを要します．

## 2. 理論と出典

モラン過程（Moran, 1958）は，大きさ $N$ が一定の有限集団での選択の標準的なモデルです．
Nowak, Sasaki, Taylor & Fudenberg（2004）は，ゲームの利得から適応度を作り，これを進化ゲーム理論の頻度依存の過程にしました．
Traulsen, Shoresh & Nowak（2008）の定式化では，1 個体を適応度に比例して選んで同じ子を作り，その子が無作為に選んだ 1 個体と入れ替わります．
型 $A$ のエージェントが $j$ 人のとき，$A$ が 1 人増える確率は次のとおりです．

$$T^+(j) = \frac{j f_A}{j f_A + (N - j) f_B}\cdot\frac{N - j}{N}$$

因子 $(N - j)/N$ は，死ぬエージェントが親を含む $N$ 人全員から選ばれることを表します．適応度はステップの利得 $\pi$ の関数です．

$$\texttt{Linear}\ \{w\}:\ f = 1 - w + w\,\pi \quad (w \in [0, 1]), \qquad \texttt{Exponential}\ \{w\}:\ f = e^{w\pi}$$

$w = 0$ は中立な浮動で，`Linear` の $w = 1$ では適応度が利得に等しくなります．1 人の変異体が集団を占める確率は次のとおりです．

$$\rho_A = \frac{1}{1 + \sum_{k=1}^{N-1} \prod_{j=1}^{k} f_B(j)/f_A(j)}$$

適応度が一定（$f_A = r$，$f_B = 1$）なら $\rho = (1 - 1/r)/(1 - 1/r^N)$，中立なら $1/N$ です．

## 3. データフロー

![moran_update data flow](../assets/mech-moran-update.svg)

そのステップの利得と保持行動を読みます．ステップの RNG から 1 ステップに 4 回引きます．保持行動を 1 つ書きます．

## 4. 6フェーズループにおける位置

**Reward** に，`game_decide` の後（したがって学習の後）に登録されます．読む利得は Interaction で `game_play` が書いたものです．
PostStep では，`world.held()` はすでに入れ替えを含み，`world.payoffs()` は打ったばかりのステップを表します
（[`PostStep` で見えるもの](../games.ja.md#poststep-で見えるもの) を参照）．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `GameWorld.payoff` | ✓ | | 適応度に写す．有限でない利得，負か有限でない適応度，正でない適応度の合計はステップを止める． |
| `GameWorld.held` | ✓ | ✓ | 1 ステップにちょうど 1 つ書く．親と死ぬエージェントは更新前の状態から引く． |
| `GameWorld.topology` | | | `Topology::None` でなければならない（組み立て時に検査）． |
| `PolicyTable.slot` | ✓ | | 組み立て時に 1 回検査する．全エージェントが Held でなければならない． |

## 6. 依存関係と順序制約

- **上流：** `game_play`（Interaction）がそのステップの利得を書いていること．よく使う Matching は `RandomPairs`，`RandomGroups`，`WholePopulation` です．
- **トポロジーなし：** 格子やネットワークを持つ World は `assemble_with` が拒否します（`GameError::Population`）．
- **線形の適応度の正値性：** `Fitness::Linear { w }` では，1 ステップに取りうる最小の利得で $1 - w + w\,\pi_{\min}$ が正でなければなりません．
  そのために有界な利得の値域が要ります．評価経路は `Pairwise::new(NormalFormOutcome)` か `counting_play()` で，
  `PayoffAggregation::Sum` では `encounter_bounds` を報告する Matching が必要です（[組み立て時の検査](../games.ja.md#組み立て時の検査) を参照）．
  `Fitness::Exponential` にはこの検査は要りません．
- **Held のエージェントだけ**と**共通の行動の集合**は，ほかの動学と同じです（`GameError::Population`）．

## 7. パラメータ

| パラメータ | 既定 | 種別 | 備考 |
|---|---|---|---|
| `fitness: Fitness` | —（必須） | tunable | `Linear { w }`（`w` は `[0, 1]`）か `Exponential { w }`（`w` は有限）．そうでなければ `MoranUpdate::new` が `GameError::Population` を返す．`w` は選択の強さ． |
| `mutation: f64` | —（必須） | tunable | 子が親の行動の代わりに一様に引いた行動を採る確率．`[0, 1]`（引いた行動が親の行動と一致することもある）．`0` なら単型の状態が吸収状態になる． |
| `update: Update` | —（必須） | structural | `Update::Synchronous`．方式はこれだけで，既定値は無い． |

## 8. 適用方法

ライブラリ専用です．トポロジーの無い World で，`assemble_with` の引数 `dynamics` に設定を渡します．

### ライブラリモード

```rust
use socsim_games::{
    assemble_with, Fitness, MoranUpdate, NormalFormOutcome, Pairwise, PlayOptions, RandomPairs,
    Update,
};

// Action 1 is a mutant with constant fitness 2 against 1 for action 0.
let game = MatrixGame::bimatrix(
    &[vec![1.0, 1.0], vec![2.0, 2.0]],
    &[vec![1.0, 2.0], vec![1.0, 2.0]],
)?;
let world = build_world(game, 10, (), InitialActions::PerAgent(one_mutant), &rules, &held, 1, 2_000, seed)?;
let mut sim = assemble_with(
    world,
    RandomPairs::new(),                                    // 10 agents: one encounter each
    rules,                                                 // an empty RuleBook: every agent is held
    Pairwise::new(NormalFormOutcome),
    MoranUpdate::new(Fitness::Linear { w: 1.0 }, 0.0, Update::Synchronous)?,
    PlayOptions::new(),
    seed,
)?
.build();
// Step until held() is all 0 (extinction) or all 1 (fixation).
```

## 9. 決定論性と RNG

エンジンのストリーム（`derive_seed(seed, &[1])`，ステップの RNG）から，1 ステップに 4 回，この順に引きます．
親のための一様な `f64`（適応度の累積を `AgentId` の順にたどる），死ぬエージェントのための `gen_range`，変異するかを決める一様な `f64`，変異体のための一様な行動です．
変異体の行動は変異が起きないステップでも引くので，引く回数は結果に依りません．
ステップの間に状態を持たないので，snapshot から正確に再開できます．

## 10. 期待される動作

$N = 10$，`RandomPairs`（全員がちょうど 1 回対戦するので `Sum` と `Mean` が一致する），`Fitness::Linear { w: 1.0 }`，変異なし，初期の変異体 1 人では，
変異体が集団を占める頻度は $\rho$ に合います．

| ゲーム | $\rho$ | 観測された固定 |
|---|---|---|
| 中立，$A = \begin{pmatrix}1 & 1\\ 1 & 1\end{pmatrix}$ | $1/10$ | 40 000 回中 4 016 回 |
| 一定の適応度 $r = 2$，$A = \begin{pmatrix}1 & 1\\ 2 & 2\end{pmatrix}$ | $512/1023 \approx 0.5005$ | 8 000 回中 4 040 回 |

どちらの回数も，正確な二項検定の受理域（$[3793, 4210]$ と $[3848, 4160]$）に入ります．吸収までの平均のステップ数は約 28 と 35 です．

## 11. 参考文献

- Moran, P. A. P. (1958). Random processes in genetics. *Mathematical Proceedings of the Cambridge Philosophical Society*, 54(1), 60–71.
- Nowak, M. A., Sasaki, A., Taylor, C., & Fudenberg, D. (2004). Emergence of cooperation and evolutionary stability in finite populations. *Nature*, 428(6983), 646–650.
- Traulsen, A., Shoresh, N., & Nowak, M. A. (2008). Analytical results for individual and group selection of any intensity. *Bulletin of Mathematical Biology*, 70(5), 1410–1424.
