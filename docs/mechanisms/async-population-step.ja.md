[English](async-population-step.md) | **日本語**

# 非同期の集団更新（`async_population_step`）

> Held のエージェントが 1 人ずつ更新します．活性化したエージェントは比べる利得を計算し直し，模倣の規則を当てて新しい行動をただちに書くので，次のエージェントはもうその行動を相手に対戦します．
> **フェーズ：** PreStep，Reward．**出典：** Huberman & Glance（1993）．**種別：** asynchronous imitation．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`async_population_step` は [`socsim-games`](../games.ja.md#非同期更新) の集団動学の非同期版です．
`assemble_async` と，`.async_dynamics(..)` を呼んだ `GameSimFactory` が，シミュレーションの**唯一の**ゲームの Mechanism として登録します．
`game_match`，`game_decide`，`game_play` と同期の動学の代わりになります．
全エージェントが **Held** で，Matching が対戦を実体化せずに評価するもの（`GridNeighborhood`，`NetworkEdges`，`WholePopulation`）である World で動きます．
写しの規則は [`imitate_best`](imitate-best.ja.md)，[`fermi_update`](fermi-update.ja.md)，[`replicator_update`](replicator-update.ja.md) のもので，`AsyncPopulationStep` で包んで渡します．
各ステップで次を行います．

1. $N$ 回，エージェントを 1 人ずつ活性化します．順はスケジューラーの順か，復元抽出の一様な抽選です（`AsyncActivation`）．
2. 活性化したエージェント $i$ ごとに，$i$ と，規則が比べる近傍の利得をその時点の `held` から計算し直し，規則を当てて，$i$ の新しい行動をただちに `held` に書きます．
3. 最後の活性化の後，最終的な `held` からステップ全体を 1 回対戦させ，利得，対戦数，ステップ履歴を記録します．

## 2. 理論と出典

Nowak & May（1992）は空間囚人のジレンマの全セルを同じ瞬間に更新し，協力者の塊が残り続け，混沌と形を変えることを示しました．
Huberman & Glance（1993）は，同時の更新には現実の集団に無い大域的な時計が要ると論じます．
彼らはそれをミクロステップの列に置き換えます．各ミクロステップでは無作為に選んだ 1 つのセルが，その時点の配置から計算した利得を比べて更新し，1 世代は $N$ ミクロステップからなります．したがって各セルは平均して 1 世代に 1 回更新します．
協力者の格子に裏切り者を 1 人置いて始めると，同期のモデルは多くの協力者を含む対称な模様を保ち，非同期のモデルは全員の裏切りで終わります（論文の Fig. 1）．

活性化したエージェント $i$ について，その時点の保持行動を $\mathbf a$ とすると，規則は次の局所利得を読みます．

$$\pi_k(\mathbf a) = \bigoplus_{e \ni k} u_k\big(\mathbf a_e\big), \qquad k \in \{i\} \cup N(i)$$

- $e$ — $k$ が利得を受け取る対戦（格子では $k$ が中心の対戦，ネットワークでは $k$ に接する辺，全員対戦では 1 つの対戦）．
- $u_k$ — その対戦での $k$ の利得．ゲームの利得関数から求めます．
- $\bigoplus$ — 和．`PayoffAggregation::Mean` では平均．
- $N(i)$ — トポロジーでの $i$ の近傍（`WholePopulation` では全エージェント）．

続いて動学の規則（最も稼いだ者，フェルミの確率，比例模倣）で $a_i \leftarrow a_{\text{source}}$ を決め，次の活性化の前に書きます．
局所利得はステップ全体の評価と同じ対戦を，同じ順，同じ席の向きで足すので，全体を計算し直した値とビット単位で等しく，`imitate_best` の同点もそこと同じように解けます．

## 3. データフロー

![async_population_step data flow](../assets/mech-async-population-step.svg)

保持行動，ゲームの利得，トポロジーの近傍の一覧を読みます．
`held` を 1 人ずつ書き，ステップに 1 回，最終的な行動から計算し直した利得，対戦数，ステップ履歴を書きます．

## 4. 6フェーズループにおける位置

**PreStep** と **Reward** に登録されます．
PreStep では `game_match` と同じようにそのステップの対戦の状態を消します．対戦の表を空にし，`payoff`，`n_played`，ステップごとの打った行動の数え上げを初期化するので，Decision や Interaction に足した Mechanism は同期のシミュレーションと同じく 0 の利得を読みます．
Reward では活性化を回し，ステップを記録します．Decision と Interaction には参加しません．
PostStep では，`payoffs()`，`n_played()`，ステップ履歴，`held()` のどれもが，そのステップの全更新の後に保持している行動を表します（[`PostStep` で見えるもの](../games.ja.md#poststep-で見えるもの) を参照）．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `GameWorld.held` | ✓ | ✓ | 活性化したエージェント 1 人ずつ，その場で書く． |
| `GameWorld.topology` | ✓ | | 規則が利得を比べる近傍．有向ネットワークでは出近傍． |
| `GameWorld.payoff`，`n_played` | | ✓ | PreStep で 0 にし，Reward の最後に最終的な `held` から 1 回だけ書く．局所利得は Mechanism の作業領域に置き，World には書かない． |
| `GameWorld.plays` | | ✓ | PreStep で空にし，空のままにする． |
| ステップ履歴 | | ✓ | ステップごとに 1 件．最終的な `held` から． |
| 集計値の表 | | ✓ | 対戦の評価経路が集計値を持つとき，ステップごとに 1 行． |
| `PolicyTable.slot` | ✓ | | 組み立て時に 1 回検査する．全エージェントが Held でなければならない． |

ステップの記録の値は，同じ `held` で `game_play` の対戦を実体化しない経路が書く値と同じです．同期の記録とは形ではなく意味が違います．

## 6. 依存関係と順序制約

- **組み立て：** `assemble_async(world, matching, play, step, opts, seed)` がこの Mechanism だけを登録します．
  `game_match`，`game_decide`，`game_play`，ほかの動学を足さないでください．スケジューラー，Recorder，`game_metrics` は利用者が足します．
- **Matching：** `GridNeighborhood`，`NetworkEdges`，`WholePopulation`．毎ステップ相手が変わる Matching（`RandomPairs`，`RandomGroups`）と，対戦を実体化しないパターンを持たない Matching は拒否されます．
- **対戦の評価経路：** `Pairwise::new(NormalFormOutcome)` か `counting_play()`（集計値の有無は問わない）．利用者定義の `OutcomeRule` は拒否されます．
- **オプション：** `DecisionScope::PerStep`，`materialize(false)`，`report_mixed(false)`，`OnInvalidAction::Stop` が必要です．
- **動学：** `Update::Asynchronous` で作り，`assemble_with` と同じ検査を受けます（Held のエージェント，共通の行動の集合，`imitate_best` の格子かネットワーク，`replicator_update` の $\beta \le 1/\Delta\pi$）．
  `MoranUpdate` は包めません．モランの 1 ステップは全員の適応度から引く 1 回の出生と死亡で，«活性化したエージェントの新しい行動» という単位を持たないからです．
- **下流：** [`game_metrics`](game-metrics.ja.md) は PostStep で計算し直した記録を読みます．
  `describe()` がその意味を述べるよう，`MetricsOptions::payoff_timing(PayoffTiming::AfterUpdate)` で作ってください．

## 7. パラメータ

| パラメータ | 既定 | 種別 | 備考 |
|---|---|---|---|
| `AsyncPopulationStep::new(dynamics)` | —（必須） | structural | `Update::Asynchronous` で作った `ImitateBest`，`FermiUpdate`，`ProportionalImitation`．それぞれのパラメータ（同点の破り方，$K$，$\beta$）はそのまま効く． |
| `activation(AsyncActivation)` | `AgentOrder` | structural | `AgentOrder`：全エージェントを 1 回ずつ，スケジューラーの順に（順次のスケジューラーでは `AgentId` の昇順，`RandomActivationScheduler` では毎ステップ新しい置換）．`UniformWithReplacement`：スケジューラーを使わず，復元抽出で $N$ 回一様に引く．Huberman & Glance（1993）の手続き．`activation_mode()` で読み出す． |
| `log_updates(bool)` | `false` | observation | エージェントの行動を変えた活性化ごとに，payload `{"agent", "order", "from", "to"}` のイベント `game.async_update` を記録する（`order` はそのステップでの活性化の通し番号．0 始まり）．`updates_logged()` で読み出す． |

## 8. 適用方法

ライブラリ専用です．Scenario ファイルと `normal-form` パックは非同期更新を受け付けません．

### ライブラリモード

```rust
use socsim_engine::SequentialScheduler;
use socsim_games::{
    assemble_async, AsyncActivation, AsyncPopulationStep, GridBoundary, GridNeighborhood,
    GridNeighbors, GridSpec, ImitateBest, ImitationTie, NormalFormOutcome, Pairwise, PlayOptions,
    Topology, Update,
};

let world = build_world(game, side * side, (), initial, &rules, &held, 1, steps, seed)?
    .with_topology(Topology::grid(GridSpec {
        rows: side,
        cols: side,
        boundary: GridBoundary::Fixed,
        neighborhood: GridNeighbors::Moore,
    })?)?;
let mut sim = assemble_async(
    world,
    GridNeighborhood::new().include_self(true),
    Pairwise::new(NormalFormOutcome),
    AsyncPopulationStep::new(
        ImitateBest::new(Update::Asynchronous).tie_break(ImitationTie::KeepOwn),
    )
    .activation(AsyncActivation::UniformWithReplacement),
    PlayOptions::new(),
    seed,
)?
.scheduler(Box::new(SequentialScheduler)) // the order is not used by UniformWithReplacement
.build();
```

`GameSimFactory` では同じ step を `.async_dynamics(..)` に渡します．factory は指標にも `PayoffTiming::AfterUpdate` を付けます（[`GameSimFactory` による組み立て](../games.ja.md#型状態) を参照）．
完全なプログラムは `crates/socsim-games/examples/huberman_glance.rs`（`--features core,engine,grid`）です．

## 9. 決定論性と RNG

エンジンのストリーム（`derive_seed(seed, &[1])`）から，Reward で活性化を 1 回ずつ順に行いながら引きます．
`UniformWithReplacement` では，活性化のたびにエージェントを引き（`gen_range(0..N)`），続いて規則の乱数を引きます．`fermi_update` と `replicator_update` は相手を 1 人と一様な `f64` を 1 つ（近傍を持たないエージェントは引かない），`imitate_best` は何も引きません．$N$ 人を先にまとめて引くことはしません．
`AgentOrder` では規則の乱数だけを引き，スケジューラーが引いた順を使います．
作業領域の利得は活性化のたびに計算し直すので，ステップの間に状態を持たず，snapshot から正確に再開できます．同じ seed なら同じ実行になります．

## 10. 期待される動作

例 `huberman_glance`：固定境界の 99 × 99 の格子，Moore 近傍と自己対戦，`PrisonersDilemma::weak(1.85)`，中央に裏切り者が 1 人，`KeepOwn` の `ImitateBest` と `UniformWithReplacement` です．

```sh
cargo run -q -p socsim-games --features core,engine,grid --example huberman_glance -- --update async --steps 217 --seed 1
```

```text
generation,cooperator_share
1,0.9969390878481788
2,0.987756351392715
3,0.9732680338740944
...
119,0.00010203040506070809
120,0
...
217,0
```

- seed 0〜4 では，それぞれ第 119，120，79，129，134 世代から全員が裏切ります．同じ構成を同期の `imitate_best`（`--update sync`）で走らせると，第 217 世代にも 3 920 人の協力者（割合 0.39995918783797574）が残ります．
- **この結果は $b > 1.8$ に限ります．** $b \le 1.8$ では，非同期に更新しても協力が残ります．
- 順次のスケジューラーでの `AgentOrder` は `AgentId` の順の決定的な掃引です．対称な裏切り者 1 人の初期配置から，最初の世代のうちに格子の対称性を崩します（その後，行 48 は列 0〜47 が協力者，列 48〜98 が裏切り者になります）．同期の更新はこの対称性を保ちます．
- `WholePopulation`，トーラス，正則なネットワークでは全エージェントの席の数が同じです．そこでは `game.played_share` と `game.population_share` はどのステップでも等しくなります．どちらも最終的な `held` から数えるからです．
  固定境界の格子では縁のエージェントの席が少ないので，2 つはわずかに違います．更新の効果ではありません．

## 11. 参考文献

- Huberman, B. A., & Glance, N. S. (1993). Evolutionary games and computer simulations. *Proceedings of the National Academy of Sciences*, 90(16), 7716–7718.
- Nowak, M. A., & May, R. M. (1992). Evolutionary games and spatial chaos. *Nature*, 359(6398), 826–829.
