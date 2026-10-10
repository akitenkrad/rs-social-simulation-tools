[English](game-decide.md) | **日本語**

# ゲームの決定（`game_decide`）

> 席に着いた各エージェントの決定規則が行動を選び，対戦の後に規則が学習します．
> **フェーズ：** Decision ＋ Reward．**出典：** ゲーム理論のシミュレーション（決定規則）．**種別：** rule-driven．

[← Mechanism カタログに戻る](../mechanisms.ja.md)

## 1. 概要

`game_decide`（型は `RuleDriver`）は，[`socsim-games`](../games.ja.md) の 3 つの Mechanism の 2 つ目です．
すべてのエージェントの `DecisionRule` を動かします．

- **Decision** では，席に着いた各エージェントに行動を尋ねます（規則を通して．`Held` のエージェントは保持行動から）．
  行動を `StageGame::check_action` で検査し，エージェントの席に書き，保持行動を更新します．
- **Reward** では，規則で動く全エージェントについて，そのステップの記録と対戦の結果を渡して `learn_step` を呼びます．

規則のオブジェクトそのものは共有されるコードです．各エージェントの規則の状態は World の `PolicyTable` にあるので，
この Mechanism は再開後の実行に影響する状態を持ちません．

`game_decide` が動かすのは自分の `RuleBook` の種類だけです．どの Driver が種類を担当するかは `KindCatalog` で決まります．
feature `llm` では，`LlmRuleBook` に登録した種類は [`game_llm_decide`](game-llm-decide.ja.md) の担当で，
`game_decide` はそれらのエージェントを Decision でも Reward でも飛ばします．その席は，`assemble_with_llm` が `game_decide` のすぐ後に登録する `game_llm_decide` が埋めます．

## 2. 理論と出典

決定規則は，繰り返しゲームにおける行動戦略です．エージェントが知っていることから行動への写像と，内部状態の更新からなります．

$$a_i^t = \text{decide}_{k(i)}\big(s_i,\ \text{view}_i^t;\ \text{rng}\big), \qquad s_i \leftarrow \text{learn}_{k(i)}\big(s_i,\ \text{outcome}_i^t\big)$$

- $k(i)$ — エージェント $i$ に割り当てた規則の種類（`PolicyTable.kind`）．
- $s_i$ — エージェント $i$ の規則の状態（`X::PolicyState`）．
- $\text{view}_i^t$ — `DecisionContext`：自分の役割と対戦，現在の相手，自分の履歴，相手の行動の履歴，`X::view_for(i)`．

組込みの規則は `Fixed`（一定の行動），`UniformRandom`（役割の行動から一様），
`TitForTat`（Axelrod, 1984：初回は協力し，以後は相手の直前の行動をまねる）です．
規則は `socsim_marl::Policy` とは別物です．`DecisionRule` は `G::Action` 上のゲーム層の戦略で状態を World に置くものであり，
観測ベクトル上のニューラルな方策ではありません．

## 3. データフロー

![game_decide data flow](../assets/mech-game-decide.svg)

Decision は，`game_match` が書いた席，各エージェントの規則の種類と状態，保持行動，履歴，`X` を読み，
選んだ行動を席に，保持行動と規則の状態を World に書きます（`decide` は状態を変更してよい）．
Reward は，`game_play` が書いたそのステップの記録と対戦の結果を読み，規則の状態を更新します．

## 4. 6フェーズループにおける位置

**Decision** と **Reward** に登録されます．

- **Decision** — 同じフェーズで `game_match` の後（登録順）に動くので，対戦ができています．
  エージェントはスケジューラーの活性化の順（`ctx.agent_order`）に訪れ，席のないエージェントと LLM の種類のエージェントは飛ばします．
  `assemble_with_llm` では，同じフェーズで次に `game_llm_decide` が動き，LLM のエージェントの席を埋めます．
- **Reward** — `game_play`（Interaction）が利得とステップ履歴を書いた後なので，`learn_step` はステップ全体を見ます．
  エージェントは `AgentId` の昇順に訪れ，`Held` のエージェントと LLM の種類のエージェントは飛ばします（`game_llm_decide` の Reward はこの後に動く）．

## 5. 状態の読み書きコントラクト

| フィールド | 読み | 書き | 備考 |
|---|:--:|:--:|---|
| `PlayArena.seats` | ✓ | | Decision：行動するエージェントとその役割． |
| `PlayArena.actions` | | ✓ | Decision：各席の検査済みの行動． |
| `GameWorld.held` | ✓ | ✓ | Decision：`Held` のエージェントはこれを出す．規則で動くエージェントの行動はこれを置き換える． |
| `PolicyTable`（`kinds`，`slot`，`kind_of`，`states`） | ✓ | ✓ | `decide` と `learn_step` はエージェントの状態への `&mut` を受け取る． |
| ステップ履歴，`game`，`x`，`ids`，`clock.t` | ✓ | | `DecisionContext` / `LearnContext` を通して． |
| `PlayArena.payoffs` / `credit` | ✓ | | Reward：`LearnContext::outcomes()` を通して． |
| Recorder のイベント `game.invalid_action` | | ✓ | `OnInvalidAction::Fallback` のときだけ． |

## 6. 依存関係と順序制約

- **上流：** `game_match`（Decision）が対戦を書いていること．Reward の前に `game_play`（Interaction）が利得とステップ履歴を書いていること．
- **下流：** `game_play` はすべての席に行動があることを要求します．LLM のエージェントの席は，`game_decide` の後に登録される `game_llm_decide` が埋めます．
- **情報構造：** 既定のビューで見えるのは，自分の対戦の相手だけです（それ以外の相手に対して `opponent_history` は `None` を返す）．
  World 全体は `omniscient()` からだけ読めます．Decision の間に `omniscient()` から読む `held` は，活性化の順序に依存することがあります．

## 7. パラメータ

| パラメータ | 既定値 | 種別 | 備考 |
|---|---|---|---|
| `rules: RuleBook<G, X>` | — | structural | `assemble` に渡す．種類は名前で登録する． |
| `opts.on_invalid_action` | `OnInvalidAction::Stop` | policy | `Stop`：不正な行動でステップが失敗する．`Fallback(map)`：種類ごとの代替行動に差し替え，`game.invalid_action` を記録する． |

`Fallback` の表が対象にするのはこの Mechanism の種類だけです．LLM の種類は書きません（そのフォールバックは LLM の規則の `RetryProtocol` が持つ）．
`Fallback` のとき，`assemble` は使われているすべての規則の種類に代替行動があることを要求し，各代替行動を全役割で `check_action` により検査します．
イベントの payload は `{ "agent", "kind", "role", "action" }`（拒否された行動を `Debug` 形式で）です．

## 8. 適用方法

ライブラリ専用です．`assemble` が `rules` と `opts` から `game_decide` を構築します．

### ライブラリモード

```rust
use std::collections::BTreeMap;
use std::sync::Arc;
use socsim_games::{
    assemble, FixedPlayers, NormalFormOutcome, OnInvalidAction, PlayOptions, RuleBook,
    TitForTat, UniformRandom,
};

let mut rules = RuleBook::new();
rules.register("tit_for_tat", Arc::new(TitForTat::new(0)))?;
rules.register("uniform_random", Arc::new(UniformRandom))?;
// build_world(game, n, x, initial_held, &rules, &assignment, history_len, t_max, seed)?

let opts = PlayOptions::new().on_invalid_action(OnInvalidAction::Fallback(BTreeMap::from([
    ("tit_for_tat".to_owned(), 0),
    ("uniform_random".to_owned(), 0),
])));
let sim = assemble(world, FixedPlayers::pairs(&[(0, 1)]), rules, NormalFormOutcome, opts, seed)?
    .build();
```

## 9. 決定論性と RNG

`decide` はエンジンのステップの RNG（`ctx.rng`，`derive_seed(seed, &[1])` から）を受け取ります．
`UniformRandom` はこれを引き，`Fixed` と `TitForTat` は引きません．
規則の状態の初期化（`build_world` の中）は `derive_seed(seed, &[3, agent, generation])` を使い，
`derive_seed(seed, &[2])` は World の `decision_root` として保存されます．どちらのラベルも予約済みです．
学習は `AgentId` の昇順，決定はスケジューラーの順で行うので，同じ seed なら実行は再現されます．
規則の状態がすべて World にあるので，snapshot からの再開も正確です．

## 10. 期待される動作

繰り返し囚人のジレンマで `TitForTat` と `Fixed(D)` を対戦させると，`TitForTat` は最初のステップで協力し，2 ステップ目から裏切ります．
100 ラウンドの得点は 99 と 104 です（`examples/ipd_pair.rs`）．
範囲外の行動を返す規則は，`Stop` ではステップを止め，`Fallback` では（発生ごとに `game.invalid_action` を 1 件記録して）差し替えられます．

## 11. 参考文献

- Axelrod, R. (1984). *The Evolution of Cooperation*. Basic Books.
