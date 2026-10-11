[English](game-match.md) | **日本語**

# ゲームの対戦組み（`game_match`）

> ステップごとの対戦の状態を空にし，そのステップの対戦の席にエージェントを着けます．
> **フェーズ：** PreStep ＋ Decision．**出典：** ゲーム理論のシミュレーション（対戦の組み方）．**種別：** structural．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`game_match` は，[`socsim-games`](../games.ja.md) が戦略形ゲームのシミュレーションのために登録する 3 つの Mechanism の 1 つ目です．
ステップのうち「誰が誰と対戦するか」を受け持ちます．

- **PreStep** では，前のステップの対戦（`PlayArena`）と，エージェントごとの `payoff`・`n_played` を空にします．
- **Decision** の先頭では，設定された `Matching` に World を渡し，そのステップの対戦（役割ごとに 1 席）を対戦の表に書き込ませます．

組込みの Matching は 7 つあります．`FixedPlayers`（毎ステップ同じ組），`RoundRobin`（毎ステップすべての 2 人の組），
および集団用の `RandomPairs`，`RandomGroups`，`WholePopulation`，`NetworkEdges`（feature `net`），`GridNeighborhood`（feature `grid`）です．
集団用のものは [`docs/games.ja.md` の「集団用の Matching」](../games.ja.md#集団用の-matching) で説明します．

対戦の表を使わずにステップを評価できるとき — 全エージェントが Held，`DecisionScope::PerStep`，評価経路が `Pairwise::new(NormalFormOutcome)` か `counting_play()`，
Matching が `Matching::implicit()` で型を返す — は，`game_match` は PreStep でステップごとの状態を空にしますが，Decision では対戦を**書きません**．
代わりに `game_play` がその型をたどります（[対戦を実体化しない評価](../games.ja.md#対戦を実体化しない評価) を参照）．
`PlayOptions::materialize(true)` でこれを止められます．

行動の選択や利得の計算はしません．それは `game_decide` と `game_play` が，ここで書かれた対戦から行います．

## 2. 理論と出典

繰り返しゲームや進化ゲームのシミュレーションでは，*対戦の組み方*が各ラウンドで誰と誰が出会うかを決めます．
固定の相手（2 人の繰り返しゲーム），無作為の組，総当たり，ネットワーク上の隣人などです．
組み方を段階ゲームや決定規則から切り離しておくと，同じゲームと規則を異なる相互作用の構造の下で走らせられます．

`Matching<G, X>` トレイトが 1 つの組み方を表します．

$$\text{encounters}_t = \mathcal{M}\big(t,\ \text{ids},\ \text{game},\ \text{held},\ \text{history},\ X;\ \text{rng}\big)$$

各対戦は，役割ごとに 1 エージェントを並べた組と credit の方式です．
組込みの `FixedPlayers` は毎ステップ同じ組を返し，標準的な 2 人の繰り返しゲームになります．
組込みの `RoundRobin` は毎ステップすべての組 $\{i, j\}$（$i < j$）を返します．Axelrod (1984) の総当たり戦で，
必要なら $(i, j)$ と $(j, i)$ の両方の順で組みます．
集団用の Matching は，無作為な組と無作為な群れ（毎ステップ引き直す），集団全体での 1 つの対戦，ネットワークの辺ごとの対戦，
Nowak & May（1992）の格子の近傍を扱います．格子の近傍では，各エージェントは近傍それぞれと，利得を受け取る中心として対戦します（`Credit::Center`）．

総当たりでは，エージェントは 1 ステップに複数の対戦に，場合によっては違う役割で着きます．
そのときに何回決定するかが**決定の粒度**です（`PerStep`，`PerAgentRole`，`PerEncounter`．[決定の粒度](../games.ja.md#決定の粒度) を参照）．
Matching は `MatchContext::decision_scope()` としてこれを受け取り，その粒度では表せない席の着け方を `check` で拒否できます．

## 3. データフロー

![game_match data flow](../assets/mech-game-match.svg)

PreStep はステップごとのフィールドを空にします．Decision は対戦組みのコンテキスト
（`t`，決定の粒度，エージェントの id，ゲーム，トポロジー，保持行動，ステップ履歴，`X`）と `MatchingState` を読み，
対戦（`seats`，`credit`，空の行動と利得の欄）を `PlayArena` に書き込みます．

## 4. 6フェーズループにおける位置

**PreStep** と **Decision** に登録されます．

- **PreStep** はステップの最初に動くので，以降のすべてのフェーズは空の対戦の表と 0 の計数を見ます．
- **Decision**：`assemble` は `game_match` を `game_decide` より先に登録し，同じフェーズの Mechanism は登録順に動きます．
  したがって，エージェントに行動を尋ねる前に対戦ができています．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `GameWorld.plays`（`PlayArena`） | | ✓ | PreStep：空にする．Decision：`ArenaWriter::push` で対戦を追加する．実体化しない経路では何も追加しない． |
| 役割ごとの打った行動の数 | | ✓ | PreStep：空にする（実体化しない経路だけ．`game_play` が埋める）． |
| `GameWorld.payoff` | | ✓ | PreStep：`0.0` で埋める． |
| `GameWorld.n_played` | | ✓ | PreStep：`0` で埋める． |
| `GameWorld.matching_state` | ✓ | ✓ | `encounters_into` に可変で渡す．`FixedPlayers` では `MatchingState::None`，`RoundRobin` では `MatchingState::RoundRobin { cursor }`（毎ステップ `cursor` += 1）．World とともに直列化される． |
| `clock.t`，決定の粒度，`ids`，`game`，`topology`，`held`，ステップ履歴，`x` | ✓ | | `MatchContext` を通して読み取りのみ（`decision_scope()` は `PlayOptions::decision_scope` を返す）． |

## 6. 依存関係と順序制約

- **上流：** ステップ内にはありません．保持行動と履歴は前のステップ（`game_decide`，`game_play`）から来ます．
- **下流：** `game_decide`（Decision）は席を読んで，どのエージェントがどの役割で行動するかを決めます．
  `game_play`（Interaction）はすべての対戦を評価します．席に着かなかったエージェントはそのステップでは行動も対戦もしません．
- **決定の粒度：** `DecisionScope::PerStep` では，各エージェントが 1 ステップに持つ役割は 1 つでなければなりません．
  そのため `RoundRobin::check` は，`PerStep` のもとでエージェントが 3 人以上のとき，または `both_orders(true)` のときに拒否します．
  これを前もって判定できない独自の Matching では，ステップの実行時に `game_decide` が検出します（`RuleError::AmbiguousRole`）．
- `game_match` を最初に登録しておきます．`assemble` がそうします．

## 7. パラメータ

| パラメータ | 既定値 | 種別 | 備考 |
|---|---|---|---|
| `matching: M`（`M: Matching<G, X>`） | — | structural | `assemble` に渡す．組込み：`FixedPlayers::pairs(&[(a, b), …])` / `FixedPlayers::new(vec![[AgentId; 2], …])`，`RoundRobin::new()`． |
| `RoundRobin::both_orders(bool)` | `false` | structural | `true`：各組 `(i, j)` の後に `(j, i)` も積む．各エージェントはどの相手とも両方の役割で対戦する． |
| `RoundRobin::self_play(bool)` | `false` | structural | `true` は拒否される（自分自身との対戦には対応しない）． |
| `RandomPairs::new()`，`RandomGroups::new(k)?`，`WholePopulation::new()` | — | structural | 集団用の Matching（feature `core`）．`RandomGroups` は `k ≥ 2` と `k` 役割のゲームを要する． |
| `NetworkEdges::new()` | — | structural | feature `net`．ネットワークのトポロジーを要する． |
| `GridNeighborhood::new().include_self(bool)` | `false` | structural | feature `grid`．格子のトポロジーを要する．`true` ならセルごとに自己対戦を 1 つ足す． |

`FixedPlayers::check`（`assemble` の中で一度だけ走る）は，未知のエージェント，複数の組に現れるエージェント，
役割がちょうど 2 でないゲームを拒否します（`GameError::InvalidMatching`）．
`RoundRobin::check` は，役割がちょうど 2 でないゲーム，`self_play(true)`，そして `DecisionScope::PerStep` のもとでの
3 人以上のエージェントまたは `both_orders(true)` を拒否します（`GameError::InvalidMatching`）．

`RoundRobin` はエージェントを `AgentId` で並べ，組を辞書式の順に積みます．役割 0 には番号の小さい方が着きます．
4 人なら `(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)`，両方の順なら `(0, 1), (1, 0), (0, 2), (2, 0), …` です．
`GridNeighborhood` 以外の Matching はすべて `Credit::All` を使います．

Matching は `check` のほかに，席を組める役割の数（`supports_roles`．既定は 2），1 ステップにエージェントが利得を受け取る対戦の数（`encounter_bounds`．集団動学の利得の範囲を求めるのに使う），
`PerStep` の 1 ステップに 1 人が複数の役割を持ってよいか（`allows_multi_role_per_step`．`NetworkEdges` と `GridNeighborhood` は許す），
実体化しない型を持つか（`implicit`）を `assemble` に伝えます．

独自の `Matching` が要する状態は，その構造体ではなく実験状態 `X` に置きます．構造体はコードであり，snapshot に含まれません．

## 8. 適用方法

`game_match` はライブラリ専用です（シナリオ TOML には登録されません）．直接は構築せず，`assemble` が `matching` 引数から作ります．

### ライブラリモード

```rust
use socsim_games::{assemble, FixedPlayers, NormalFormOutcome, PlayOptions};

let mut sim = assemble(
    world,                              // from build_world(...)
    FixedPlayers::pairs(&[(0, 1), (2, 3)]),
    rules,
    NormalFormOutcome,
    PlayOptions::new(),
    seed,
)?
.build();
sim.step()?;
let arena = sim.world().plays();        // this step's encounters
```

総当たりにするには，`RoundRobin::new()`（必要なら `.both_orders(true)`）と，それに合う決定の粒度
（例えば `PlayOptions::new().decision_scope(DecisionScope::PerEncounter)`）を渡します．
[総当たりの例](../games.ja.md#例総当たりの繰り返し囚人のジレンマ)（`examples/ipd_tournament.rs`）を参照してください．

## 9. 決定論性と RNG

PreStep は乱数を引きません．Decision では `encounters_into` がエンジンのステップの RNG（`ctx.rng`，`derive_seed(seed, &[1])` から）を受け取ります．
`RandomPairs` と `RandomGroups` は，そこから 1 ステップに 1 回エージェントの ID の並べ替えを引きます．ほかの組込みの Matching は何も引きません．`RoundRobin` の cursor は World の一部なので，再開した実行は数え続けます．
無作為に組む独自の Matching は，この RNG だけから引いてください．
そうすれば実行も，再開した実行も再現できます．

## 10. 期待される動作

`FixedPlayers::pairs(&[(0, 1)])` では，各ステップの後に `world().plays().n_encounters()` は `1` で，両エージェントの `n_played == 1` です．
組がすべてのエージェントを覆っていないとき，覆われていないエージェントはそのステップで `n_played == 0` となり，`StepRecord` の行動は `None` になります．

`RoundRobin::new()` で `n` 人なら，各ステップの対戦は `n(n − 1)/2` 個（両方の順なら `n(n − 1)` 個）で，
各エージェントの `n_played` は `n − 1`（`2(n − 1)`）です．`k` ステップ後の World の `matching_state` は `RoundRobin { cursor: k }` です．
囚人のジレンマ `[[3, 0], [5, 1]]` で 3 人が `C`，`D`，`C` を出すと，ステップごとの利得は `PayoffAggregation::Sum` で `[3, 10, 3]`，`Mean` で `[1.5, 5, 1.5]` です．

## 11. 参考文献

- Axelrod, R. (1984). *The Evolution of Cooperation*. Basic Books.
- Nowak, M. A., & May, R. M. (1992). Evolutionary games and spatial chaos. *Nature*, 359(6398), 826–829.
