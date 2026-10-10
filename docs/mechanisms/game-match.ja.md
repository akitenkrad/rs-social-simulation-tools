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

行動の選択や利得の計算はしません．それは `game_decide` と `game_play` が，ここで書かれた対戦から行います．

## 2. 理論と出典

繰り返しゲームや進化ゲームのシミュレーションでは，*対戦の組み方*が各ラウンドで誰と誰が出会うかを決めます．
固定の相手（2 人の繰り返しゲーム），無作為の組，総当たり，ネットワーク上の隣人などです．
組み方を段階ゲームや決定規則から切り離しておくと，同じゲームと規則を異なる相互作用の構造の下で走らせられます．

`Matching<G, X>` トレイトが 1 つの組み方を表します．

$$\text{encounters}_t = \mathcal{M}\big(t,\ \text{ids},\ \text{game},\ \text{held},\ \text{history},\ X;\ \text{rng}\big)$$

各対戦は，役割ごとに 1 エージェントを並べた組と credit の方式です．
組込みの `FixedPlayers` は毎ステップ同じ組を返し，標準的な 2 人の繰り返しゲームになります．

## 3. データフロー

![game_match data flow](../assets/mech-game-match.svg)

PreStep はステップごとのフィールドを空にします．Decision は対戦組みのコンテキスト
（`t`，エージェントの id，ゲーム，トポロジー，保持行動，ステップ履歴，`X`）と `MatchingState` を読み，
対戦（`seats`，`credit`，空の行動と利得の欄）を `PlayArena` に書き込みます．

## 4. 6フェーズループにおける位置

**PreStep** と **Decision** に登録されます．

- **PreStep** はステップの最初に動くので，以降のすべてのフェーズは空の対戦の表と 0 の計数を見ます．
- **Decision**：`assemble` は `game_match` を `game_decide` より先に登録し，同じフェーズの Mechanism は登録順に動きます．
  したがって，エージェントに行動を尋ねる前に対戦ができています．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `GameWorld.plays`（`PlayArena`） | | ✓ | PreStep：空にする．Decision：`ArenaWriter::push` で対戦を追加する． |
| `GameWorld.payoff` | | ✓ | PreStep：`0.0` で埋める． |
| `GameWorld.n_played` | | ✓ | PreStep：`0` で埋める． |
| `GameWorld.matching_state` | ✓ | ✓ | `encounters_into` に可変で渡す（現在は `MatchingState::None`）． |
| `clock.t`，`ids`，`game`，`topology`，`held`，ステップ履歴，`x` | ✓ | | `MatchContext` を通して読み取りのみ． |

## 6. 依存関係と順序制約

- **上流：** ステップ内にはありません．保持行動と履歴は前のステップ（`game_decide`，`game_play`）から来ます．
- **下流：** `game_decide`（Decision）は席を読んで，どのエージェントがどの役割で行動するかを決めます．
  `game_play`（Interaction）はすべての対戦を評価します．席に着かなかったエージェントはそのステップでは行動も対戦もしません．
- `game_match` を最初に登録しておきます．`assemble` がそうします．

## 7. パラメータ

| パラメータ | 既定値 | 種別 | 備考 |
|---|---|---|---|
| `matching: M`（`M: Matching<G, X>`） | — | structural | `assemble` に渡す．組込み：`FixedPlayers::pairs(&[(a, b), …])` / `FixedPlayers::new(vec![[AgentId; 2], …])`． |

`FixedPlayers::check`（`assemble` の中で一度だけ走る）は，未知のエージェント，複数の組に現れるエージェント，
役割がちょうど 2 でないゲームを拒否します（`GameError::InvalidMatching`）．

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

## 9. 決定論性と RNG

PreStep は乱数を引きません．Decision では `encounters_into` がエンジンのステップの RNG（`ctx.rng`，`derive_seed(seed, &[1])` から）を受け取ります．
`FixedPlayers` はこれを引きません．無作為に組む独自の Matching は，この RNG だけから引いてください．
そうすれば実行も，再開した実行も再現できます．

## 10. 期待される動作

`FixedPlayers::pairs(&[(0, 1)])` では，各ステップの後に `world().plays().n_encounters()` は `1` で，両エージェントの `n_played == 1` です．
組がすべてのエージェントを覆っていないとき，覆われていないエージェントはそのステップで `n_played == 0` となり，`StepRecord` の行動は `None` になります．

## 11. 参考文献

- Axelrod, R. (1984). *The Evolution of Cooperation*. Basic Books.
