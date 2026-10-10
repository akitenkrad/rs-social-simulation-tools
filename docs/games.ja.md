[English](games.md) | **日本語**

# 戦略形ゲーム（`socsim-games`）

`socsim-games` は socsim のエンジン上でゲーム理論のシミュレーションを走らせる crate です．
エージェントは**段階ゲーム**の対戦の席に着き，各自が**決定規則**で行動を選び，
**利得規則**が行動の組を利得に変え，規則は起きたことから学習します．
これを毎ステップ，通常の [6フェーズティックループ](architecture.ja.md#6フェーズティックループ) の中で行います．

この crate はライブラリ専用です（`ModulePack` を持たず，`socsim` バイナリには組み込まれていません）．
このページでは，部品，その状態の置き場所，規則が見てよい情報，実行可能なシミュレーションの組み立て方を説明します．
シミュレーションとは別に，古典的なゲームと有限ゲームのソルバー（Nash 均衡，支配，ミニマックス，ロジット QRE）も提供します（[有限ゲームを解く](#有限ゲームを解く) を参照）．

---

## Cargo feature

| feature | 追加されるもの | socsim への依存 |
|---|---|---|
| *（既定）* | 段階ゲームのトレイト（`StageGame`，`NormalFormGame`，`FiniteGame`），`MatrixGame`，組込みのゲーム（`catalog`），混合戦略（`mixed`），ソルバー（`solve`） | **なし** — 既定のビルドはどの `socsim-*` crate にも依存しません（ソルバーの線形計画のバックエンドは純 Rust の `microlp` crate） |
| `core` | World（`GameWorld`），規則，利得規則，Matching，ビュー，`build_world` | `socsim-core` |
| `engine` | `assemble`（`SimulationBuilder` を返す）．`core` を含意 | `socsim-core`，`socsim-engine` |
| `llm` | LLM の決定規則，応答ストア，再生の manifest（[LLM による決定](#llm-による決定) を参照）．`core` を含意．Mechanism `game_llm_decide` と `assemble_with_llm` には `engine` も要る | `socsim-core`，`socsim-llm`（既定の feature だけ．ネットワークなし） |
| `live` | `live_backend_factory()`（キャッシュなしの Ollama → OpenAI のバックエンド）．`llm` を含意 | `llm` と同じ＋`socsim-llm/live` |

`llm` を有効にしなければ，この crate は `socsim-llm` に依存しません．`socsim-config` と `socsim-runner` には常に依存しません．
シミュレーションを走らせるには `core` と `engine` が要ります．

```toml
socsim-games = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["engine"] }
socsim-core   = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-engine = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
```

---

## 最小の例：繰り返し囚人のジレンマ

`Tit-for-Tat` と無条件の裏切り者を 100 ラウンド対戦させます
（`crates/socsim-games/examples/ipd_pair.rs`）．

```rust
use std::sync::Arc;

use socsim_games::{
    assemble, build_world, AssignGroup, AssignKind, Assignment, Fixed, FixedPlayers,
    InitialActions, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};

const ROUNDS: usize = 100;

fn main() {
    // Row player's and column player's payoffs; action 0 = C, 1 = D.
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .expect("the prisoner's dilemma matrix is valid");

    let mut rules = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    rules.register("fixed_defect", Arc::new(Fixed::new(1))).unwrap();

    let seed = 1;
    let world = build_world(
        game,
        2,                          // n_agents
        (),                         // experiment state X
        InitialActions::Same(0),    // initial held action
        &rules,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".to_owned()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("fixed_defect".to_owned()), count: 1 },
        ]),
        ROUNDS,                     // history_len
        ROUNDS as u64,              // t_max
        seed,
    )
    .expect("the world configuration is valid");

    let mut simulation = assemble(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .expect("the simulation configuration is valid")
    .build();

    let mut scores = [0.0, 0.0];
    for _ in 0..ROUNDS {
        simulation.step().unwrap();
        for (score, payoff) in scores.iter_mut().zip(simulation.world().payoffs()) {
            *score += payoff;
        }
    }
    println!("TFT score: {}, Fixed(D) score: {}", scores[0], scores[1]);
}
```

```sh
cargo run -p socsim-games --example ipd_pair --features core,engine
# TFT score: 99, Fixed(D) score: 104
```

`GameWorld::payoffs()` が持つのは**そのステップの**利得です（毎ステップ空にされます）．
累積の得点は呼び出し側で足し合わせます．

---

## 段階ゲーム

段階ゲームは，1 回の対戦の役割と，各役割が取れる行動を定めます．3 つのトレイトが段をなします．

| トレイト | 追加されるもの | メソッド |
|---|---|---|
| `StageGame` | 役割と行動の妥当性 | `type Action`，`n_roles()`，`check_action(role, &action) -> Result<(), ActionError>` |
| `NormalFormGame: StageGame` | 全役割・全行動の組に対する利得 | `payoff(role, profile)`，`payoffs_into(profile, out)`（既定の実装は `payoff` を役割ごとに呼ぶ） |
| `FiniteGame: NormalFormGame<Action = usize>` | 番号付きの有限個の行動 | `n_actions(role)`，`action_label(role, action)`（既定は `"a{action}"`），`is_symmetric()`（既定は `false`） |

規則と利得規則は，必要な段をトレイト境界として宣言します．
`UniformRandom` と `TitForTat` は `FiniteGame` を，`NormalFormOutcome` は `NormalFormGame` を要求するので，
組み合わせの誤りは実行時ではなくコンパイル時のエラーになります．

### `MatrixGame`

`MatrixGame` は組込みの `FiniteGame` です．役割ごとに行優先の利得テンソルを 1 つ持ち，行動は 0 始まりの番号です．

| コンストラクタ | 用途 |
|---|---|
| `MatrixGame::new(n_actions, payoffs)` | 一般の n 役割のテンソル．`payoffs[role]` は行動の組ごとに 1 要素 |
| `MatrixGame::bimatrix(a, b)` | 2 役割．`a[i][j]` / `b[i][j]` が行プレイヤー / 列プレイヤーの利得 |
| `MatrixGame::zero_sum(a)` | 2 役割．列プレイヤーは `-a[i][j]` を受け取る |
| `.with_labels(labels)` | `action_label` 用の役割ごとの行動ラベル |

形の不一致（`Shape`），有限でない利得（`NonFinite`），形の合わないラベル表（`Labels`），
大きすぎるゲーム（`TooLarge`：行動の組が 10⁶ を超えるか，利得の要素が 10⁷ を超える）のとき，
構築は `MatrixError` で失敗します．`is_symmetric()` は構築時に一度だけ計算されます
（2 役割，行動の数が等しく，`payoff(0, (i, j)) == payoff(1, (j, i))`）．
`MatrixGame` は serde で直列化できるので，snapshot に含まれます．

シミュレーション層が現在組み立てるのは **2 役割**の対戦だけです．
それ以外の `n_roles()` は `assemble` が `GameError::UnsupportedRoles` で拒否します．

### 組込みのゲーム：`catalog`

`socsim_games::catalog` は古典的な 2 人ゲームを 5 つ提供します．どれも `FiniteGame` を実装しているので，
`MatrixGame` と同じように `build_world`，組込みの規則，[ソルバー](#有限ゲームを解く) に渡せます．
`to_matrix()` は同じラベルを持つ等価な `MatrixGame` に変換します．
行動のラベルは ASCII で，協力側を持つゲーム（`C`，`stag`）ではそれが行動 `0` です．

| 型 | 行動 `0, 1, …` | コンストラクタ | 構築時に確かめる条件 |
|---|---|---|---|
| `PrisonersDilemma` | `C`，`D` | `new(T, R, P, S)`；`weak(b)` ＝ `new(b, 1, 0, 0)` | `T > R > P ≥ S`（`P = S` を許すので，`weak(b)` には `b > 1` が要る） |
| `BattleOfSexes` | `B`，`V` | `new(a, b)`；`Default`：`a = [[2, -1], [-1, 1]]`，`b = [[1, -1], [-1, 2]]` | `(B, B)` と `(V, V)` が狭義の均衡で，役割 0 は `(B, B)` を，役割 1 は `(V, V)` を狭義に好む |
| `MatchingPennies` | `H`，`T` | `new(a, b)`；`Default`：`a = [[-1, 1], [1, -1]]`，`b = -a` | ゼロ和（要素ごとに `b == -a`）で，純粋戦略の均衡を持たない |
| `StagHunt` | `stag`，`hare` | `new(a, b)`；`Default`：`a = [[9, 0], [8, 7]]`，`b = [[9, 8], [0, 7]]` | `(stag, stag)` と `(hare, hare)` が狭義の均衡で，`(stag, stag)` はどちらの役割にも `(hare, hare)` より狭義に大きい利得を与える |
| `RockPaperScissors` | `R`，`P`，`S` | `new(win, loss)`；`Default`：`win = loss = 1` | `win > 0` かつ `loss > 0` |

`a[i][j]` / `b[i][j]` は，役割 0 が `i`，役割 1 が `j` を選んだときの役割 0 / 役割 1 の利得です（`MatrixGame::bimatrix` と同じ）．
囚人のジレンマは役割 0 に `[[R, S], [T, P]]` を，役割 1 にその転置を与えます．
標準の `T, R, P, S` は一通りには決まらないので，囚人のジレンマには意図して `Default` を用意していません．利得は必ず書き下します．
既定のマッチング・ペニーでは，硬貨の面が異なるときに役割 0 が勝ちます．
じゃんけんでは勝者が `win`，敗者が `-loss`，引き分けが `0` を受け取ります．
既定の `win = loss = 1` では標準の反対称なゼロ和の形になります（`win ≠ loss` のときはゼロ和ではありません）．

どのコンストラクタも `Result<_, CatalogError>` を返します．利得に NaN や無限大があれば `NonFinite`，
利得がそのゲームを定める形をしていなければ `Condition(_)` です．
これらの型は serde で直列化でき，逆直列化でも同じ検査が走るので，不正なゲームを snapshot やファイルから読み込むこともできません．

```rust
use socsim_games::catalog::{PrisonersDilemma, RockPaperScissors};
use socsim_games::{FiniteGame, NormalFormGame};

fn main() {
    // T > R > P >= S is checked when the game is built.
    let pd = PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap();
    assert!(PrisonersDilemma::new(3.0, 3.0, 1.0, 0.0).is_err()); // T = R

    println!("{} vs {}: {}", pd.action_label(0, 0), pd.action_label(1, 1), pd.payoff(0, &[0, 1]));
    // C vs D: 0

    let rps = RockPaperScissors::default(); // win = 1, loss = 1
    let matrix = rps.to_matrix();           // the same payoffs as a MatrixGame
    println!("{} beats {}: {}", matrix.action_label(0, 1), matrix.action_label(1, 0), matrix.payoff(0, &[1, 0]));
    // P beats R: 1
}
```

---

## 決定規則

`DecisionRule<G, X>` は 1 エージェントの行動を選び，必要ならそのステップの結果から学習します．

```rust
pub trait DecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;
    fn decide(
        &self,
        state: &mut X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
        rng: &mut SimRng,
    ) -> Result<G::Action, RuleError>;
    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> { Ok(()) }
}
```

`decide` と `learn_step` の受け手が `&self` であることに注意してください．規則のオブジェクトは共有されるコードであり，
エージェントについて覚えておくことはすべて，渡される `state: &mut X::PolicyState` に置きます
（[規則の状態と snapshot](#規則の状態と-snapshot) を参照）．

組込みの規則：

| 規則 | 境界 | 振る舞い | 状態のスロット |
|---|---|---|---|
| `Fixed::new(action)` | 任意の `StageGame` | 常に `action` を出す（初期化時に全役割で検証） | `Stateless` |
| `UniformRandom` | `FiniteGame` | 役割の行動から `rng` で一様に引く | `Stateless` |
| `TitForTat::new(coop)` | `FiniteGame`，2 役割，各役割 2 行動 | 初回は `coop`，以後は相手の直前の行動 | `TftState { last_opponent }` |

規則は `RuleBook` に**種類の名前**で登録します（`register(kind, Arc<dyn DecisionRule<G, X>>)`．
重複した種類は `RuleError::DuplicateKind`）．種類の番号は種類名の昇順で振られるので，登録の順序には依存しません．

### 規則をエージェントに割り当てる

`build_world` は `AssignGroup { kind, count }` の列である `Assignment` を受け取ります．

- `AssignKind::Rule(kind)` — そのエージェントは登録済みの規則 `kind` で動き，各自が `init_state` で自分の状態を得ます．
- `AssignKind::Held` — そのエージェントは規則も状態のスロットも**持たず**，保持している行動（`InitialActions` の値）を出し続けます．
- `Assignment::Ordered(groups)` はグループの順にエージェントの番号を割り当て，
  `Assignment::Shuffled(groups)` は World 初期化のストリームで割り当てを並べ替えます．

`count` の和は `n_agents` に一致しなければなりません（`GameError::AgentCount`）．

### `RuleInitContext`

`init_state` は `RuleInitContext { agent, kind, generation, init_seed }` を受け取ります．
`init_seed` は `derive_seed(seed, &[3, agent, generation])` で，状態の初期値を乱数で決めるためのエージェントごとのストリームです
（`build_world` が作るエージェントの `generation` は `0`）．
初期化の乱数は `SimRng::from_seed(init_seed)` から引いてください．

---

## 利得規則

`OutcomeRule<G, X>` は 1 つの対戦について役割ごとの利得を書き込みます．

```rust
pub trait OutcomeRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, G, X>,
        encounter: EncounterRef<'_>,
        actions: &[G::Action],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()>;
}
```

`NormalFormOutcome`（任意の `NormalFormGame` 用）は `game.payoffs_into(actions, payoffs)` を呼ぶだけです．
利得が行動の組以外にも依存するときは独自の利得規則を書きます．
`OutcomeContext` は `t()`，`game()`，`x()`，`held(agent)`，`topology()` を，
`EncounterRef` は対戦の `id`，`seats`（役割ごとのエージェント），`credit` を与えます．
例えば `X` に置いた評判で利得を割り引く場合：

```rust
impl OutcomeRule<MatrixGame, ReputationState> for ReputationDiscount {
    fn name(&self) -> &str { "reputation_discount" }
    fn outcome(
        &self,
        ctx: &OutcomeContext<'_, MatrixGame, ReputationState>,
        encounter: EncounterRef<'_>,
        actions: &[usize],
        payoffs: &mut [f64],
    ) -> socsim_core::Result<()> {
        ctx.game().payoffs_into(actions, payoffs);
        for (role, payoff) in payoffs.iter_mut().enumerate() {
            *payoff *= ctx.x().reputation[encounter.seats[role].0 as usize];
        }
        Ok(())
    }
}
```

対戦は `Credit::All`（全席がそれぞれの利得を受け取る）でなければなりません．
それ以外の credit の方式はまだ提供されておらず，ステップがエラーで止まります．

---

## 実験状態 `X`

`GameWorld<G, X>` は**実験状態** `X: ExperimentState` について総称的です．
`X` はモデル固有のもの — 評判，シグナル，予算，独自の Matching の記録 — をすべて置く場所です．
既定は `X = ()` で，組込みの規則の状態を使います．

```rust
pub trait ExperimentState: Send + Sync + 'static {
    type PolicyState: Clone + Send + Sync + Debug + 'static;
    type View<'a> where Self: 'a;
    fn view_for(&self, me: AgentId) -> Self::View<'_>;
}
```

- **`PolicyState`** — エージェントごとの規則の状態の型．規則で動くエージェント 1 人につき 1 つの値が World に保存されます．
- **`View<'a>` / `view_for(me)`** — エージェントが既定で `X` から見てよいもの（[情報構造](#情報構造) を参照）．

### `PolicyState` と `Slot`

組込みの規則は利用者の `PolicyState` の型を知りません．`Slot<T>` トレイト（`slot`，`slot_mut`，`from_slot`）を通して自分の状態に届きます．
`Fixed` と `UniformRandom` は `Slot<Stateless>` を，`TitForTat` は `Slot<TftState>` を要求します．
`BuiltinPolicyState`（`X = ()` の `PolicyState`）は両方を実装しています．
組込みの規則と独自の規則を混ぜるには，これを包みます．

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
enum MyPolicyState {
    Builtin(BuiltinPolicyState),
    Discriminator { threshold: f64, seen: u32 },
}

impl Slot<TftState> for MyPolicyState {
    fn slot(&self) -> Option<&TftState> {
        match self { Self::Builtin(s) => s.slot(), _ => None }
    }
    fn slot_mut(&mut self) -> Option<&mut TftState> {
        match self { Self::Builtin(s) => s.slot_mut(), _ => None }
    }
    fn from_slot(value: TftState) -> Self {
        Self::Builtin(<BuiltinPolicyState as Slot<TftState>>::from_slot(value))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReputationState { reputation: Vec<f64> }

impl ExperimentState for ReputationState {
    type PolicyState = MyPolicyState;
    type View<'a> = &'a [f64];
    fn view_for(&self, _me: AgentId) -> Self::View<'_> { &self.reputation }
}
```

`PolicyState` が実装していないスロットを要求する組込みの規則を使うと，コンパイルエラーになります．
型としてはスロットがあっても保存されている値の variant が違う場合，規則は `RuleError::MissingSlot` を返します．

実行中に `X` を変えるには，独自の `Mechanism<GameWorld<G, X>>` をビルダーに足し，`GameWorld::x_mut()` から書き込みます．

---

## 規則の状態と snapshot

再開に要る状態は，すべて **World の中**にあります．

| 状態 | フィールド（読み出し） |
|---|---|
| エージェントごとの規則の状態 | `PolicyTable`（`world.policies()`，`.state(agent)`，`.kind(agent)`） |
| 保持している行動 | `world.held()` |
| そのステップの対戦・行動・利得 | `PlayArena`（`world.plays()`） |
| そのステップのエージェントごとの利得と対戦数 | `world.payoffs()`，`world.n_played()` |
| エージェントごとのステップ履歴（長さ `history_len` の環） | `world.history(agent)` |
| 実験状態 | `world.x()` |

3 つの Mechanism（`game_match`，`game_decide`，`game_play`）と `RuleBook`，`Matching`，`OutcomeRule` は，
再開後の実行に影響する状態を**持ちません**．これらはコードであり，シミュレーションを組み直すときに改めて渡します．
このため，エンジンの [snapshot と再開](architecture.ja.md#スナップショット保存と再開) はゲームのシミュレーションでも正確です．

```rust
let snapshot = partial.snapshot();          // GameWorld + RNG position
let mut resumed = build_simulation(/* any seed */);
resumed.restore(snapshot);                  // continues bit-identically
```

`GameWorld` は `G`，`G::Action`，`X`，`X::PolicyState` が `Serialize`/`Deserialize` のときに直列化できます．
したがって `X` と `PolicyState` には復元できるデータだけを置いてください（ネットワークのクライアント，ファイルハンドル，キャッシュは置かない）．
また，実行ごとに変わるデータを規則・Matching・利得規則の構造体に持たせないでください．保存されないので，再開した実行が食い違います．

---

## 情報構造

規則は `GameWorld` を直接ではなく，コンテキストを通して見ます．既定のメソッドは標準的な繰り返しゲームの情報構造を与えます．

**`DecisionContext`**（`decide` に渡される）：

| メソッド | 返すもの |
|---|---|
| `t()`，`me()`，`unit()` | ステップ，自分の id，決定の単位（`DecisionUnit::Step`） |
| `role()`，`encounter()` | そのステップの自分の役割と対戦 |
| `opponents()` | **同じ対戦の**ほかのエージェント |
| `my_history()` | 自分の過去の `StepRecord`（行動，利得，`n_played`） |
| `opponent_history(id)` | `id` の行動の履歴．**`id` が現在の相手でなければ `None`** |
| `game()` | 段階ゲーム |
| `view()` | `X::view_for(me)` |
| `omniscient()` | 制限のないデータ（後述） |

**`LearnContext`**（`learn_step` に渡される）：`t()`，`me()`，`record()`（このステップの自分の記録），`my_history()`，
`outcomes()`（このステップの自分の対戦を `EncounterId` の順に．各 `EncounterOutcome` は `role`，`seats`，`actions`，`payoffs`，`credit` を持つ），
`game()`，`view()`，`omniscient()`．

`X` が何を見せるかは `view_for(me)` で決めます．不完備情報のゲームなら `me` の私的シグナルだけを，
間接互恵のモデルなら公開の評判のベクトルを返す，といった具合です．

**`omniscient()`** は `OmniscientContext` を返し，全エージェントの id・保持している行動・履歴と，`X` 全体を読めます．
これを呼ぶと**情報構造の保証がなくなります**．全知であることを意図した規則にだけ使ってください．
また Decision の間は，同じステップで先に行動したエージェントがすでに保持行動を更新しているため，
`held` が活性化の順序に依存することがあります．

---

## Matching

`Matching<G, X>` は Decision の先頭でエージェントを対戦の席に着けます．

```rust
pub trait Matching<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, ctx: &MatchContext<'_, G, X>) -> Result<(), GameError>;
    fn init_state(&self, ctx: &MatchContext<'_, G, X>) -> MatchingState { MatchingState::None }
    fn encounters_into(
        &self,
        ctx: &MatchContext<'_, G, X>,
        state: &mut MatchingState,
        rng: &mut SimRng,
        out: &mut ArenaWriter<'_, G::Action>,
    ) -> socsim_core::Result<()>;
}
```

`encounters_into` は `out.push(&seats, Credit::All)` ごとに対戦を 1 つ積みます（`seats` は役割ごとに 1 エージェント）．
`check` は `assemble` の中で一度だけ走り，Matching が World と合っているかを検査します．
組込みの `FixedPlayers::pairs(&[(0, 1), …])` は毎ステップ同じ組を対戦させ，
未知のエージェント，2 回現れるエージェント，役割がちょうど 2 でないゲームを拒否します．
あるステップで席に着かなかったエージェントは，そのステップでは決定も対戦もしません．

独自の Matching が要する状態は，Matching の構造体ではなく **`X` に**置きます（独自の Mechanism で更新します）．
`MatchingState` には現在 `None` しかなく，Matching の構造体の中の状態は snapshot に含まれないからです．

---

## 1 ステップの流れ

`assemble` は 3 つの Mechanism をこの順に登録します．

| Mechanism | フェーズ | 行うこと |
|---|---|---|
| [`game_match`](mechanisms/game-match.ja.md) | PreStep，Decision | PreStep：対戦の表，`payoff`，`n_played` を空にする．Decision：そのステップの対戦を `Matching` に組ませる． |
| [`game_decide`](mechanisms/game-decide.ja.md) | Decision，Reward | Decision：席に着いた各エージェントの規則が `decide` する（Held なら保持行動を使う）．行動は `check_action` で検査する．Reward：規則で動く全エージェントの `learn_step`． |
| [`game_play`](mechanisms/game-play.ja.md) | Interaction | 全席が埋まっていることを検査し，`OutcomeRule` で利得を計算し，`PayoffAggregation` でエージェントごとに集計し，全エージェントの `StepRecord` をステップ履歴に追記する． |

同じフェーズの Mechanism は登録順に動くので，`game_match` が席を決めてから `game_decide` が行動を尋ねます．1 ステップは次のとおりです．

```
PreStep      game_match   reset arena / payoff / n_played
Decision     game_match   Matching → encounters
             game_decide  decide + check_action → actions, held
Interaction  game_play    OutcomeRule → payoffs → aggregation → history
Reward       game_decide  learn_step (rule state)
```

feature `llm` では，`assemble_with_llm` が 4 つ目の Mechanism [`game_llm_decide`](mechanisms/game-llm-decide.ja.md)（Decision，Reward）を
`game_decide` と `game_play` の間に登録します．これは種類が LLM の規則であるエージェントの席を埋めます（[LLM による決定](#llm-による決定) を参照）．

`assemble` の後に足した Mechanism（返されたビルダーの `.add_mechanism(...)`）は，各フェーズでゲームの Mechanism の後に動きます．
したがって Reward フェーズの Mechanism はそのステップの利得を見られます．

---

## 組み立て：`build_world` と `assemble`

```rust
pub fn build_world<G: StageGame, X: ExperimentState>(
    game: G,
    n_agents: usize,
    x: X,
    initial_held: InitialActions<G::Action>,   // Same(a) | PerAgent(vec)
    rules: &RuleBook<G, X>,
    assign: &Assignment,
    history_len: usize,
    t_max: u64,
    seed: u64,
) -> Result<GameWorld<G, X>, GameError>;

pub fn assemble<G, X, M: Matching<G, X>, O: OutcomeRule<G, X>>(
    world: GameWorld<G, X>,
    matching: M,
    rules: RuleBook<G, X>,
    outcome: O,
    opts: PlayOptions<G::Action>,
    seed: u64,
) -> Result<socsim_engine::SimulationBuilder<GameWorld<G, X>>, GameError>;
```

`build_world` の検査：`history_len > 0`（`InvalidHistoryLength`），割り当ての `count` の和と `PerAgent` の長さが `n_agents` に一致（`AgentCount`），
初期行動がすべての役割で `check_action` を通る，割り当てた種類がすべて `RuleBook` にある（`RuleError::UnknownKind`），各 `init_state` が成功する．

`assemble` の検査：ゲームが 2 役割である（`UnsupportedRoles`），World にある規則の種類がすべて `RuleBook` にある，
`OnInvalidAction::Fallback` のとき使われている種類すべてに全役割で妥当な代替行動がある，`Matching::check`．
エンジンのシードを `derive_seed(seed, &[1])` に設定してビルダーを返すので，`build()` の前にスケジューラー，Recorder，追加の Mechanism を足せます．
2 つの関数には**同じ** `seed` を渡してください．返されたビルダーで `.seed(...)` を呼ぶと，エンジンのストリームが上書きされます．

### `PlayOptions`

```rust
PlayOptions::new()                                     // Sum + Stop
    .payoff_aggregation(PayoffAggregation::Mean)        // Sum (default) | Mean
    .on_invalid_action(OnInvalidAction::Stop)           // Stop (default) | Fallback(map)
```

- **`PayoffAggregation`** — 1 ステップに複数の対戦があるとき，エージェントの利得を `payoffs()[agent]` にどうまとめるか．
  `Sum`（既定）か `Mean`（`n_played` で割る．対戦しなかったエージェントは `0` のまま）．
- **`OnInvalidAction`** — 規則が自分の役割で `check_action` を通らない行動を返したときの扱い．
  - `Stop`（既定）：ステップがエラーで失敗します．
  - `Fallback(BTreeMap<kind, action>)`：行動を種類ごとの代替行動に差し替え，イベント `game.invalid_action` を
    payload `{ "agent", "kind", "role", "action" }`（拒否された行動を `Debug` 形式で）とともに記録します．

---

## LLM による決定

feature `llm` を有効にすると，エージェントの行動を言語モデルに決めさせられます．
LLM の規則は `DecisionRule` ではありません．別の Mechanism [`game_llm_decide`](mechanisms/game-llm-decide.ja.md)（型は `LlmDriver`）が動かします．
この Mechanism はそのステップの LLM の決定をすべて集めて 1 つのバッチとして送り，その後で行動を確定します．
即決の規則（`game_decide`）と LLM の規則（`game_llm_decide`）は同じ World に混ぜられます．各種類はどちらか一方の Driver だけが担当します．

```toml
socsim-games  = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-games", features = ["llm", "engine"] }
socsim-core   = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-core" }
socsim-engine = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-engine" }
socsim-llm    = { git = "https://github.com/akitenkrad/rs-social-simulation-tools", branch = "main", package = "socsim-llm" }
```

以下の型はすべて `socsim_games::llm` にあります．
組込みの Ollama → OpenAI のバックエンドを使うときだけ，`socsim-games` に feature `live` を足します．テストとオフラインの再生にネットワークは要りません．

### 二段階の決定：`prepare` → バッチ → `resolve`

```rust
pub trait LlmDecisionRule<G: StageGame, X: ExperimentState>: Send + Sync {
    fn name(&self) -> &str;
    fn init_state(&self, ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError>;
    /// 第 1 段：要求を描画する．純粋で，乱数を使わず，状態は読むだけ．
    fn prepare(
        &self,
        state: &X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
    ) -> Result<DecisionRequest, RuleError>;
    /// 第 2 段：応答を行動・再試行・フォールバックのいずれかにする．
    fn resolve(
        &self,
        state: &mut X::PolicyState,
        req: &DecisionRequest,
        reply: &LlmReply,
        rctx: &mut ResolveContext<'_, G>,
    ) -> Resolution<G::Action>;
    /// Reward フェーズ．モデルは呼ばない．
    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> { Ok(()) }
}
```

- `prepare` は `DecisionRequest { system, prompt, attempt }` を返します（`attempt` は 0 から）．
  生成の条件（temperature，seed，`max_tokens`，`think` など）は規則が決めるのではなく，
  Driver が `LlmSetup` の `LlmConfig` と決定単位から付けます．
- `resolve` は `LlmReply { text, thinking, usage, cache_hit }` と `ResolveContext { game, role, previous, fallback_rng, t }` を受け取ります．
  `previous` は前のステップのエージェントの保持行動，`fallback_rng` は決定単位から導いたストリームです．
  戻り値は `Resolution::Accept { action, flags }`，`Resolution::Retry(次の要求)`，`Resolution::GiveUp { action, fallback, violation }` のいずれかです．
- モデルを呼ぶのは Driver の Decision フェーズの中**だけ**です．`learn_step`（Reward）は規則の状態を更新するだけです．

### 2 つの Driver にまたがる種類：`KindCatalog` と `build_world_with`

`build_world` が知っているのは `RuleBook` だけです．LLM の規則の種類があるときは，すべての種類を `KindCatalog` で表し，
`build_world_with` で World を作ります（引数は `build_world` と同じで，`&rules` の代わりに `&catalog` を渡す）．

```rust
let catalog = KindCatalog::new()
    .with_rules(&rules)?      // game_decide が担当する種類
    .with_llm(&llm_rules)?;   // game_llm_decide が担当する種類
```

- 両方の表に登録した種類は `RuleError::DuplicateKind` です．
- 種類 ID は全種類名の和集合の辞書順で振るので，どの Driver が担当するかにも登録順にもよりません．
  結果は `catalog.kinds()`，`catalog.kind_id(kind)`，`catalog.driver_name(kind)`（`"game_decide"` か `"game_llm_decide"`）で見られます．
- `build_world(.., &rules, ..)` は `build_world_with(.., &KindCatalog::from_rules(&rules), ..)` と同じです．
- `assemble` は World の種類の一覧が自分の `RuleBook` と一致することを，`assemble_with_llm` は `rules` ＋ `llm_rules` の
  カタログと一致することを検査します．違えば `GameError::InvalidKinds { expected, actual }` です．
  したがって LLM の種類を含む World は `assemble_with_llm` で組み立てます．
- 各 Driver は担当でない種類を飛ばします．`game_decide` は LLM のエージェントの席を空けたままにし，`game_llm_decide` がそれを埋めます．

### 例：LLM のエージェント対 Tit-for-Tat

バックエンドは `ScriptedClient` なので，このプログラムはオフラインで動きます．
実際のモデルを呼ぶには，構築関数を `live_backend_factory()`（feature `live`）に替えます．

```rust
use std::sync::Arc;

use socsim_games::llm::{
    assemble_with_llm, BackendFactory, BackendIdentity, LabelParser, LlmPolicy,
    LlmReplayManifest, LlmRuleBook, LlmSetup, MarkdownTableRenderer, OwnHistoryObs, ReplayMode,
    ResponseStore, WithLlm,
};
use socsim_games::{
    build_world_with, AssignGroup, AssignKind, Assignment, FixedPlayers, InitialActions,
    KindCatalog, MatrixGame, NormalFormOutcome, PlayOptions, RuleBook, TitForTat,
};
use socsim_llm::mock::ScriptedClient;

type X = WithLlm<()>; // 再生の manifest は実験状態に置く

fn main() {
    // モデルが答えるラベルを付けた囚人のジレンマ．
    let game = MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![
        vec!["C".to_owned(), "D".to_owned()],
        vec!["C".to_owned(), "D".to_owned()],
    ])
    .unwrap();

    // 即決の規則 (game_decide) と LLM の規則 (game_llm_decide)．
    let mut rules: RuleBook<MatrixGame, X> = RuleBook::new();
    rules.register("tit_for_tat", Arc::new(TitForTat::new(0))).unwrap();
    let mut llm_rules: LlmRuleBook<MatrixGame, X> = LlmRuleBook::new();
    llm_rules
        .register(
            "llm",
            Arc::new(LlmPolicy::new(
                OwnHistoryObs::new(5),
                MarkdownTableRenderer::default(),
                LabelParser::strict(),
            )),
        )
        .unwrap();

    // 両方の Driver の全種類を 1 つのカタログに．
    let catalog = KindCatalog::new()
        .with_rules(&rules)
        .unwrap()
        .with_llm(&llm_rules)
        .unwrap();

    // 応答ストアとバックエンド．構築関数はワーカーごとに 1 回呼ばれる．
    let store = ResponseStore::in_memory("ipd-llm", ReplayMode::RecordMisses);
    let identity = BackendIdentity {
        model: "scripted".to_owned(),
        endpoint: "mock://scripted".to_owned(),
    };
    let backend: BackendFactory = Arc::new(|| {
        Ok(Box::new(ScriptedClient::constant(
            "scripted",
            r#"{"action": "D", "reason": "defect"}"#,
        )))
    });

    let seed = 7;
    let rounds = 5;
    let x = WithLlm {
        inner: (),
        llm: LlmReplayManifest::for_store(&store, identity.clone()),
    };
    let world = build_world_with(
        game,
        2,
        x,
        InitialActions::Same(0),
        &catalog,
        &Assignment::Ordered(vec![
            AssignGroup { kind: AssignKind::Rule("llm".to_owned()), count: 1 },
            AssignGroup { kind: AssignKind::Rule("tit_for_tat".to_owned()), count: 1 },
        ]),
        rounds,
        rounds as u64,
        seed,
    )
    .unwrap();

    let setup = LlmSetup::new(llm_rules, store, backend, identity);
    let mut simulation = assemble_with_llm(
        world,
        FixedPlayers::pairs(&[(0, 1)]),
        rules,
        setup,
        NormalFormOutcome,
        PlayOptions::new(),
        seed,
    )
    .unwrap()
    .build();

    for _ in 0..rounds {
        simulation.step().unwrap();
        println!("held = {:?}", simulation.world().held());
    }
}
```

```text
held = [1, 0]
held = [1, 1]
held = [1, 1]
held = [1, 1]
held = [1, 1]
```

`assemble_with_llm(world, matching, rules, llm, outcome, opts, seed)` は `assemble` と同じ引数に `LlmSetup` を足したもので，
`X: HasLlmManifest` を要求し（[manifest](#manifest-と再開の保証) を参照），`game_match` → `game_decide` → `game_llm_decide` → `game_play` の順に登録します．
`OnInvalidAction::Fallback` は `game_decide` の種類だけに効きます．その表には `RuleBook` の種類だけを書きます（LLM の種類を書くと `RuleError::UnknownKind`）．
LLM の種類のフォールバックは `RetryProtocol` が受け持ちます．

### `LlmPolicy` の部品

`LlmPolicy::new(obs, renderer, parser)` は組込みの `LlmDecisionRule` です．
エージェントが何を観測するか，それをどうプロンプトにするか，応答をどう行動にするか，の 3 つの部品を型引数にした 1 つの型です．

```rust
LlmPolicy::new(OwnHistoryObs::new(5), MarkdownTableRenderer::default(), LabelParser::strict())
    .with_retry(RetryProtocol { max_retries: 2, fallback: Fallback::Previous })  // 既定: 1，Previous
    .with_max_turns(32)                                                          // 既定: 16
```

状態は `X::PolicyState` の中の `LlmConversation { max_turns, turns }` なので，この規則は `X::PolicyState: Slot<LlmConversation>` を要求します
（`BuiltinPolicyState` は feature `llm` のとき `Llm` の変種を持つ）．
決定のたびに `ConversationTurn { t, action, attempts, gave_up, payoff }` を追記し（`action` は `Debug` の表記，`payoff` は Reward で埋まる），
`max_turns` を超えたら古いものから捨てます．この記録は解析用で，プロンプトには再生しません．

**観測**（`ObservationProvider<G, X>`．`DecisionContext` だけから作るので，[情報構造](#情報構造) の既定のビューが保たれる）：

| 提供者 | `Obs` | 中身 |
|---|---|---|
| `OwnHistoryObs::new(window)` | `OwnHistory<G::Action>` | `t`，`role`，自分の直近 `window` 件の `StepRecord`（古い順），現在の各相手の直前の行動（無ければ `None`） |
| `CountsObs`（`G: FiniteGame`） | `ActionCounts` | `role`，自分の直前の行動，現在の相手のうち直前に各行動を選んだ人数．`t` を持たないので，同じ状況のエージェントは同じプロンプトになり，呼出しを 1 回で共有する |

**レンダラ**（`GameRenderer<G, Obs>`：`rules` → system プロンプト，`turn` → プロンプト，`feedback` → 再試行で末尾に足す文）：

| レンダラ | 組み合わせる観測 | system プロンプト |
|---|---|---|
| `MarkdownTableRenderer`（`G: FiniteGame`） | `OwnHistoryObs` | `action_label` から作る利得表（2 役割なら双行列，それ以外は役割ごとの表）と，応答の形式 `{"action": "<action label>", "reason": "<reason>"}` |
| `CountsRenderer`（`G: FiniteGame`） | `CountsObs` | 役割，選べるラベル，同じ応答の形式 |

**パーサ**（`ActionParser<G>`）：

- `LabelParser::strict()` — 応答は JSON のオブジェクト（そのままか，Markdown の単一のフェンス ```` ``` ```` / ```` ```json ```` で囲んだもの）で，
  `"action"` の文字列が役割のラベルのどれかと完全に一致しなければなりません．それ以外は違反です．
- `LabelParser::lenient()` — まず厳密な照合を試し，失敗したら応答のどこかで最初に現れたラベルを探し
  （`socsim_llm::extract_first_choice`：大文字小文字を区別せず，語の境界で，最も早いものが勝つ），`ParseFlags::lenient_match` を立てます．
  `.with_synonyms(label, &[..])` でそのラベルとみなす語を足せます．
  **緩い照合は，全役割の全ラベルが ASCII の `[A-Za-z0-9_-]+` であることを要求します**．
  これは規則の状態を初期化するときに検査するので，満たさなければ `build_world_with` が `RuleError::Unsupported` で失敗します．厳密な照合はどんなラベルも受け付けます．
- `.require_reason(true)` は文字列の `"reason"` も要求します．理由が無ければ，緩い照合でも違反です．
- `NumberParser::new(field).bounds(lo, hi).out_of_range(OutOfRange::Clamp)` は数値の応答のための単体の補助です（`parse_number(raw) -> ParseOutcome<f64>`）．
  JSON の数値か数字の文字列（`value_was_string`）で，有限の値だけを受け付けます．範囲外は `Violation`（既定）か切り詰め（`clamped`）です．
  `MatrixGame` 用の `ActionParser` ではありません．

受理した応答は `ParseFlags { fenced, value_was_string, clamped, lenient_match }` を，拒否した応答は
`Violation { kind, message }`（`ViolationKind::{Format, Label, Range, InvalidAction}`）を伴います．

**再試行とフォールバック** — `RetryProtocol { max_retries, fallback }`．`attempt < max_retries` の間は，違反のたびに
前のプロンプトにレンダラの `feedback` を足した再試行を作ります．そのステップの再試行はまとめて次のバッチとして送ります．
それを過ぎるとフォールバックで諦めます．

| `Fallback` | 行動 |
|---|---|
| `Previous`（既定） | 前のステップのエージェントの保持行動 |
| `UniformRandom` | パーサが `fallback_rng`（決定単位から導く．エンジンのストリームは使わない）で一様に引く |
| `Specified(action)` | `action` |

Driver は確定した行動をすべて `check_action` で検査し，不正なら `RuleError::InvalidAction` でステップを止めます．

### `game_llm_decide` の Decision の流れ

1. **準備** — 種類が LLM の規則である席に着いたエージェントについて，スケジューラーの活性化の順に `prepare` を呼ぶ．
2. **重複除去** — キャッシュのキー（後述）が同じ要求は 1 回だけ送る．
3. **照会** — 主スレッドで各要求を `ResponseStore` から引く（`Live` を除く）．`ReplayOnly` で見つからなければステップを止める．
4. **実行** — 見つからなかった要求だけを `LlmExecutor` に渡す．
5. **確定** — 決定の順に `resolve` を呼ぶ．`Retry` の要求を集め，もう 1 つのバッチとして 2〜5 を繰り返す．`GiveUp` はフォールバックで確定する．
6. **戻し** — 確定した行動を席と `held` に書く．

**シード．** `temperature == 0` のとき，すべての要求は定数のシード `derive_seed(decision_root, &[0])` を使います．
同じプロンプトはキー・呼出し・保存した記録を 1 つに共有します．
それ以外の temperature では，シードは `derive_seed(decision_root, &[t, agent, 0, 0, attempt])`（決定と試行ごとに 1 つ）なので，
エージェントが違えば要求も普通は違います．フォールバックのストリームは `derive_seed(decision_root, &[t, agent, 0, 0, u64::MAX])` です．
Driver はエンジンのステップの乱数を引かないので，活性化の順やワーカーの数で結果は変わりません．

**構成．**

```rust
let setup = LlmSetup::new(llm_rules, store, backend, identity)
    .executor(LlmExecutor::Threaded { workers: 4 })   // 既定: Sequential
    .options(options)                                 // 既定: ExecutorOptions::default()
    .config(LlmConfig::deterministic());              // 既定．seed は常に上書きされる
```

`config` は各要求の `LlmConfig` の基になります．Driver は `seed` を上書きし，要求が system を持つとき（組込みの規則は常に持つ）は `system` も上書きします．

### 応答ストア

ゲームのシミュレーションでは `socsim_llm::PromptCache` の代わりに `ResponseStore` を使います．各要求は版付きのキーで識別します．

```rust
pub struct CacheKeyV1 {
    pub version: u32,                 // 1
    pub backend: BackendIdentity,     // { model, endpoint }
    pub system: Option<String>,
    pub prompt: String,
    pub temperature_bits: u32,        // f32::to_bits．0.2 と 0.8 は別のキー (0.0 と -0.0 も別)
    pub seed: u64,
    pub omit_seed: bool,
    pub max_tokens: Option<u32>,
    pub think: Option<bool>,
    pub allow_blank: bool,
}
```

引くときのキーは `canonical_json()` の 64 bit FNV-1a です．キー全体を各 `ResponseRecord { key, text, thinking, usage, backend_metadata }` と一緒に保存し，
当たるたびに比べるので，ダイジェストの衝突は応答として返さず検出します（`StoreError::KeyCollision`）．
当たったときも思考とトークン数を復元するので，再生した呼出しは `cache_hit` 以外は実際の呼出しと同じ形で記録されます．

| 構築 | 置き場所 |
|---|---|
| `ResponseStore::in_memory(store_id, mode)` | メモリだけ（テスト用） |
| `ResponseStore::open(dir, mode, run_tag, seed)` | `dir/store_id`（最初に開いたときに作り，以後はそれを使う），`dir/base.jsonl`（読取り専用．無くてもよい），この実行のシャード `dir/shards/<run_tag>-<seed>.jsonl`（読み，追記する） |

| `ReplayMode` | ストアの照会 | 見つからないとき | 書込み |
|---|---|---|---|
| `ReplayOnly` | する | ステップが失敗する | なし |
| `RecordMisses` | する | バックエンドを呼ぶ | 新しい記録をこの実行のシャードへ |
| `Live` | しない | （すべての要求がバックエンドへ行く） | なし |

**シャードは頼んだときだけ統合されます．** `open` が読むのは基底と自分のシャードだけで，他の実行のシャードは読みません．
記録した実行が終わった後（たとえば複数シードの掃引の後）に `ResponseStore::merge_shards(dir)` を呼ぶと，
すべてのシャードを `base.jsonl` に取り込み（一意な一時ファイルに書いて rename する），シャードを消して，
`MergeReport { records_added, duplicate_records, shards_merged }` を返します．
同じキーで中身の違う記録があれば `StoreError::Conflict` です．統合すると基底のダイジェストが変わるので，統合の前に書いた manifest はストアと一致しなくなります．

### 実行器とバックエンド

```rust
pub type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn LlmClient>, LlmError> + Send + Sync>;
```

実行器はバックエンドを構築関数から**それを使うスレッドの中で**作ります（`LlmClient` は `Send` ではないため）．
`Sequential` ではバッチごとに 1 回，`Threaded` ではワーカーごとに 1 回です．ストアに当たらなかった要求が 1 件もないバッチでは作りません．
バックエンドはキャッシュを持ってはいけません（キャッシュは応答ストアです）．
`live_backend_factory()`（feature `live`）は `socsim_llm::build_live_backend` を包みます．テストでは `socsim_llm::mock::ScriptedClient` を返します．

| `LlmExecutor` | 呼出しを実行する場所 |
|---|---|
| `Sequential`（既定） | 主スレッドで 1 つずつ |
| `Threaded { workers }` | 有界のキューで配った `workers` 本のスレッド．ストアへ書くのは主スレッドだけ |

| `ExecutorOptions` のフィールド | 既定値 | 意味 |
|---|---|---|
| `timeout` | 120 秒 | 呼出しごと．**`Threaded` だけ**に効く（`Sequential` の呼出しは中断できない）．時間切れの呼出しは再送せず，ステップが失敗する |
| `max_backend_retries` | 3 | バックエンドのエラーの後の再送の回数．使い切るとステップが `RuleError::Backend` で失敗する |
| `backoff_base` | 1 秒 | k 回目の再送の前に `backoff_base × 2^(k-1)` 待つ |
| `queue_per_worker` | 2 | キューの容量 = `workers × queue_per_worker` |

`ExecutorOptions` は `#[non_exhaustive]` です．`ExecutorOptions::default()` から始めてフィールドに代入します．
`workers == 0` や `queue_per_worker == 0` はバックエンドのエラーです．
シードが決定単位で決まり，確定が決定の順に行われるので，`Sequential` と `Threaded` は同じ World を作ります．

### manifest と再開の保証

実際のモデルは同じ要求に違う応答を返しうるので，snapshot から正確に再開できるのは，再開後の実行が同じ応答を見る場合だけです．
そこで World は，どの応答に対して実行したかを記録します．`WithLlm<X> { inner, llm }` は実験状態を包み（`PolicyState` と `view_for` は `inner` に委ねる），
`LlmReplayManifest::for_store(&store, identity)` で作る `LlmReplayManifest { store_id, base_digest, mode, backend }` を持ちます．
`assemble_with_llm` は `X: HasLlmManifest` を要求し，`WithLlm<X>` がそれを実装します．

- Driver は毎回の Decision で，World の manifest を自分のストア（`store_id`，`base_digest`，`mode`）と `BackendIdentity` に照らします．
  違いがあればステップを止めます．走っている途中のシミュレーションに restore した場合も，次の Decision で照合されます．
- `verify_snapshot(&snapshot, &store, &identity)` は同じ照合を `restore` の前に行い，`GameError::ManifestMismatch` を返します．
- 再開した実行が通しの実行と一致するのは，`ReplayOnly` の場合，または `RecordMisses` で再開後の全要求がストアに当たった場合
  （`game.llm_call` の `cache_hit`，`llm.cache_hit_rate == 1`）**だけ**です．`Live` では何も保証しません．
  モードは manifest の一部なので，snapshot を取ったときと同じモードで再開します．

```rust
// 記録し，シャードを基底に取り込む．
let store = ResponseStore::open(dir, ReplayMode::RecordMisses, "record", seed)?;
// ... このストアで組み立てて走らせる ...
ResponseStore::merge_shards(dir)?;

// オフラインで再生し，snapshot を取り，同じストアに対して再開する．
let store = ResponseStore::open(dir, ReplayMode::ReplayOnly, "replay", seed)?;
// ... 組み立て，走らせ，let snapshot = simulation.snapshot(); ...
let store = ResponseStore::open(dir, ReplayMode::ReplayOnly, "resume", seed)?;
verify_snapshot(&snapshot, &store, &identity)?;
// ... このストアで組み立て，resumed.restore(snapshot) ...
```

### 記録されるもの

LLM の決定が 1 つ以上あった Decision の最後に，Driver は表 `"game"` に 1 行を書きます（`record_row`）．

| 列 | 分母 |
|---|---|
| `llm.parse_fail_rate` | 試行の数（論理的な呼出し．再試行を含む） |
| `llm.clamp_rate`，`llm.value_was_string_rate`，`llm.lenient_rate` | 試行の数 |
| `llm.cache_hit_rate` | 試行の数 |
| `llm.retry_rate` | 決定の数（1 回以上再試行したエージェント） |
| `llm.giveup_rate` | 決定の数（フォールバックしたエージェント） |

試行はすべて（重複除去で呼出しを共有したものも）イベント `game.llm_call` として記録します．payload は次のとおりです．

| キー | 値 |
|---|---|
| `agent`，`kind` | エージェントの添字と規則の種類 |
| `unit`，`unit_index` | 決定単位（`"step"`）とその添字（`0`） |
| `attempt`，`resolution` | 0 起点の試行番号．`"accept"`，`"retry"`，`"giveup"` |
| `system`，`prompt` | 要求 |
| `reply`，`thinking`，`usage` | 応答，思考，トークン数（ストアに当たったときも） |
| `seed` | 生成のシード |
| `cache_hit` | ストアから返したなら `true` |
| `deduplicated` | 同じバッチの他のエージェントの呼出しを使い回したなら `true` |
| `key` | 要求のキーのダイジェスト（小文字 16 桁の 16 進） |
| `flags` | `ParseFlags`（`retry` / `giveup` ではすべて `false`） |

LLM の決定が無いステップでは何も記録しません．

---

## 決定論性と乱数ストリーム

すべてのストリームは，`build_world` と `assemble` に渡す 1 つの `seed` から派生します．

| ラベル | ストリーム |
|---|---|
| `derive_seed(seed, &[0])` | World の初期化（`Assignment::Shuffled`） |
| `derive_seed(seed, &[1])` | エンジン（スケジューラー，およびステップの RNG を通した `Matching` と `decide` の乱数） |
| `derive_seed(seed, &[2])` | `decision_root`．World に保存される．`game_llm_decide` は生成のシードとフォールバックのストリームをここから導く（[シード](#game_llm_decide-の-decision-の流れ) を参照） |
| `derive_seed(seed, &[3, agent, generation])` | 規則の状態の初期化（`RuleInitContext::init_seed`） |

したがってゲームのシミュレーションではラベル `[2]` と `[3]` が予約済みです．独自のストリームには `[4]` 以上を使ってください
（[RNG ストリームのラベル付け規約](library.ja.md#rngストリームのラベル付け規約) を参照）．
学習は `AgentId` の昇順で，決定はスケジューラーの活性化の順で行われます．同じ seed と同じコードなら，実行はステップごとに再現されます．

---

## 有限ゲームを解く

シミュレーションとは別に，この crate は任意の `FiniteGame`（`MatrixGame`，[組込みのゲーム](#組込みのゲームcatalog)，自作の型）の
均衡とそれに関連する解概念を直接計算します．混合戦略は `socsim_games::mixed` に，ソルバーは `socsim_games::solve` にあります．
どちらも既定のビルドに含まれ，feature も `socsim-*` crate も要りません（線形計画は純 Rust の `microlp` crate で解きます）．

### 混合戦略

`MixedStrategy` は 1 つの役割の行動上の確率ベクトルで，検査済みのものです．並びは行動の番号順です．

| コンストラクタ | 結果 |
|---|---|
| `MixedStrategy::new(probs)` | 与えたベクトルそのもの（正規化し直さない）．空のとき，NaN・無限大・負の要素があるとき，和と 1 の差が `1e-9` を超えるときは `MixedError::NotADistribution` |
| `MixedStrategy::uniform(n)` | `n` 個の行動それぞれに `1/n` |
| `MixedStrategy::pure(n, action)` | `action` に確率 1 |

混合戦略の組は，役割ごとに 1 つの `MixedStrategy` を並べたスライスです．次の関数は役割の数を問わずこれを評価します（行動の組をすべて列挙します）．

| 関数 | 返すもの |
|---|---|
| `mixed::expected_payoff(&game, role, &profile)` | その役割の期待利得 `F_i(σ)` |
| `mixed::action_payoffs(&game, role, &profile)` | その役割の各純粋行動 `s_i` に対する `F_i(s_i, σ_-i)` |
| `mixed::regrets(&game, &profile)` | 役割ごとの後悔（最良の純粋行動の利得 − 期待利得） |
| `mixed::regret(&game, &profile)` | `regrets` の最大値 |

役割の数が合わない組や長さの合わないベクトルは `MixedError::{RoleCount, Dimension}` になります．

混合戦略はソルバー側だけの概念です．シミュレーションでは決定規則は常に 1 つの `G::Action`（`FiniteGame` なら行動の番号）を返し，
`MixedStrategy` が行動になることはありません．混合を打つ規則は，自分の RNG でそこから行動を 1 つ引きます．

### 許容誤差と検証

ソルバー内の比較はすべて，ゲームごとに利得の幅で尺度を合わせた 1 つの許容誤差を使います．

```text
tol = 1e-10 × max(1, 利得の最大値 − 利得の最小値)    （全役割・全行動の組にわたって）
```

`solve::tolerance(&game)` がこれを返します．ソルバーは手法そのものを信用しません．
均衡を返すソルバーはどれも，見つけた組の後悔を元のゲームで計算し直して添え（`regret` フィールド），
後悔が `tol` 以下のときに限って結果を返します．
解く前には，どのソルバーもゲームを検査します．役割が 1 つ以上あること，行動のない役割がないこと，
行動の組が 10⁶ 以下であること（`SolveError::TooLarge`），利得がすべて有限であること（`SolveError::NonFinitePayoff`）です．

混合戦略の均衡は `Equilibrium { strategies, payoffs, regret }` で返ります．役割ごとの `MixedStrategy` と期待利得，
そして検証済みの最大の後悔です．`is_pure()` は，すべての戦略が 1 つの行動に確率 1 を置くとき真です．

### ソルバーの一覧

| 関数 | プレイヤー | 返すもの | 制限 |
|---|---|---|---|
| `pure_nash(&game)` | n 人 | `Vec<PureNash>`：純粋戦略の均衡すべて | 行動の組をすべて列挙（10⁶ 以下） |
| `iterated_strict_dominance(&game)` | n 人 | `DominanceResult`：残った行動と除去の記録 | 純粋戦略による強支配だけ |
| `support_enumeration(&game)` | 2 人 | `EquilibriumSet`：非退化なゲームの均衡すべてと `findings` | 台の組が 2²⁰ 以下（10 × 10 まで） |
| `lemke_howson(&game, &options)` | 2 人 | `LemkeHowsonResult`：均衡 1 つ | どれが返るかは落とすラベルで決まる |
| `zero_sum_minimax(&game)` | 2 人，ゼロ和 | `MinimaxSolution`：ゲームの値と最適戦略 | 線形計画．主問題と双対問題を両方解いて検査する |
| `maximin_security_level(&game, role)` | 2 人 | `SecurityLevel`：1 つの役割のマキシミン保証水準 | 線形計画．主問題と双対問題を両方解いて検査する |
| `logit_qre(&game, &options)` | n 人 | `QreSolution`：与えた λ でのロジット QRE 1 つ | 局所的な不動点反復であり，分枝の追跡ではない |

エラーは `SolveError` です．上のゲームの検査のほか，ソルバーが受け付けない役割の数の `UnsupportedRoles`，
`NotZeroSum`，`InvalidLabel`，`MaxPivots`，`Lp(LpFailure)`，`VerificationFailed { duality_gap, regret }`，
`NotConverged { residual, iterations }`，`InvalidOptions`，`Mixed(MixedError)` があります．

### `pure_nash`

行動の組を辞書式の順（役割 0 の行動がもっとも上位の桁，最後の役割の行動がもっとも速く変わる）にすべて訪れ，
どの役割も一方的な逸脱で `tol` を超えて得をしない組を残します．
`PureNash { profile, payoffs, regret }` をこの順で返します．列挙は網羅的なので，この一覧はゲームの純粋戦略の均衡のすべてです（`tol` の範囲で）．
費用は行動の組の数×行動の総数で，行動の組が 10⁶ を超えるゲームは拒否します．

### `iterated_strict_dominance`

各ラウンドで，すべての役割について同時に，同じ役割の別の残っている**純粋**行動に強支配される行動
（残っている他の役割の行動のどの組み合わせに対しても `tol` を超えて劣る行動）をすべて取り除き，
何も取り除かれなくなるまで繰り返します．`DominanceResult { surviving, eliminated }` を返します．
`surviving` は役割ごとの残った行動の番号（昇順），`eliminated` は取り除いた行動ごとの `Elimination { round, role, action }`（ラウンドは 1 から数える）です．

強支配による除去は Nash 均衡を取り除かず，最終結果は取り除く順序に依存しません．よく使われる 2 つの変種は意図して入れていません．

- **弱支配**は使いません．結果が取り除く順序に依存しうるうえ，均衡を取り除くことがあるからです．
- **混合戦略による支配**は調べません．他の行動の混合にだけ支配される行動は，ここでは残ります．
  これを検出するには行動ごとに線形計画を解く必要があります．そのため，混合戦略による支配を認める教科書の定義より，残る集合が大きくなることがあります．

### `support_enumeration`

2 人ゲームだけを扱います．空でない台の組（役割 0 の台を外側，役割 1 の台を内側のキーとし，それぞれ昇順の行動番号の辞書式順）をすべて調べ，
無差別条件の方程式を解き，負の確率が出たときや台の外の行動のほうが良いときはその台を退け，候補の後悔を検証します．
`EquilibriumSet { equilibria, findings }` を返します．`tol` の範囲で同じ均衡がもう一度見つかったときは，最初のものを残します．

**すべての均衡が見つかる条件．** 非退化なゲームでは，各均衡はその台での方程式のただ 1 つの解なので，均衡はすべて見つかります（その数は奇数です）．
退化したゲームでは，台の方程式の階数が落ちたり条件が悪かったりします．そうした台は推測で埋めず，`findings` に報告します．

| `SupportFinding` | 意味 |
|---|---|
| `Degenerate { support, reason }` | 階数が落ちていて，検証済みの点がない |
| `Component { support, base, directions }` | 正の次元を持つ均衡の集合の上の検証済みの点 `base` と，零空間の方向 `directions`（それぞれ役割 0 の確率，続けて役割 1 の確率と平坦に並べたもの）．`base` は `equilibria` にも加える |
| `Inconclusive { support, reason }` | 方程式の条件が悪い（条件数の推定が 10¹² を超える）か有限でない，または最後の検証に失敗した |

後悔の検査を通らない点を均衡として報告することはありません．`is_complete()` は `findings` が空のとき真です．
このときすべての台に決着がついており，非退化なゲームなら `equilibria` が均衡の全体です．

**制限．** 役割はちょうど 2 つです（それ以外は `UnsupportedRoles`）．台の組の数 `(2^m − 1)(2^n − 1)` は 2²⁰ 以下でなければならず，
10 × 10 は受け付け，11 × 10 は受け付けません（`TooLarge`．このとき `profiles` フィールドには台の組の数が入ります）．
計算は浮動小数点で行い，厳密な有理数演算はしないので，退化に近いゲームの判定は `tol` で決まります．

### `lemke_howson`

2 人ゲームだけを扱います．相補ピボットの経路を 1 本たどり，均衡をちょうど 1 つ返します（`LemkeHowsonResult { equilibrium, pivots }`）．

| `LemkeHowsonOptions` | 既定値 | 意味 |
|---|---|---|
| `dropped_label` | `0` | 最初に落とすラベル．`m × n` のゲームでは，ラベル `0..m` が役割 0 の行動，`m..m+n` が役割 1 の行動 |
| `max_pivots` | `100_000` | ピボットがこの回数に達したら `SolveError::MaxPivots { limit }` で止める |

`LemkeHowsonOptions::default().with_dropped_label(k).with_max_pivots(n)` で設定します．`m + n` 以上のラベルは `InvalidLabel` です．
ピボットの前に，各プレイヤーの利得を定数だけずらしてすべての要素を正にします（均衡は変わりません）．結果は，ずらす前の元のゲームで検証します．
退化したゲームで起きる最小比テストの同点は，その時点の基底の逆行列による辞書式の規則で解消します．その規則でもなお同点なら，`Lp(LpFailure::Internal)` で止まります．

**何が返るか．** 非退化なゲームでは，どのラベルを落としても均衡に着きます．どの均衡に着くかはラベルで決まり，どのラベルからも着かない均衡もあります．
混合戦略の均衡があっても純粋戦略の均衡が返ることがあります（下の男女の争いでは，どのラベルでも純粋戦略の均衡に着きます）．
均衡をすべて列挙するには `support_enumeration` を使ってください．

### `zero_sum_minimax` と `maximin_security_level`

どちらも `microlp` で線形計画の組を解きます．主問題（最大化する役割の混合戦略と，それで保証できる利得）と，
双対問題（相手の混合戦略と，それで最初の役割を抑え込める利得）です．

`zero_sum_minimax(&game)` は 2 つの役割とゼロ和（すべての行動の組で `|u_0 + u_1| ≤ tol`．そうでなければ `NotZeroSum`）を要求します．
ミニマックス定理により有限の 2 人ゼロ和ゲームはどれも値を持ち，主問題と双対問題の最適値はどちらもそれに等しくなります．
返す前に，双対ギャップ `|主問題 − 双対問題|`，戦略の組の後悔，役割 0 の期待利得と値の差，
各役割の最良の純粋行動の利得と値の差（役割 0 は `+value`，役割 1 は `-value`）がすべて `tol` 以下であることを確かめ，
そうでなければ `VerificationFailed { duality_gap, regret }` を返します．
返すのは `MinimaxSolution { value, row, col, duality_gap, regret }` で，`value` は役割 0 から見た値です．
最適戦略が一意でないとき，どの最適な組が返るかは線形計画ソルバー次第です．

`maximin_security_level(&game, role)` はゼロ和かどうかを問わず任意の 2 人ゲームを受け付けます．
`SecurityLevel { level, strategy, duality_gap }` を返します．`level` は `role` が相手のどの純粋行動に対しても保証できる最大の利得で，
`strategy` はそれを達成する混合戦略です．双対ギャップと，その戦略が相手のどの行動に対しても実際に `level` を保証すること（`tol` の範囲で）を確かめます．
0 と 1 以外の役割は `InvalidOptions` です．一般和ゲームでは，これは保証であって均衡戦略ではありません．

線形計画のバックエンドが実行不能・非有界を報告したとき，中断されたとき，最適でない実行可能解で止まったとき，内部で失敗したときは，
結果ではなく `SolveError::Lp(LpFailure::{Infeasible, Unbounded, Interrupted, NotOptimal, Internal})` を返します．

### `logit_qre`

1 つの精度 λ でのロジット量的応答均衡（QRE）を，プレイヤーの数を問わず求めます．
各役割の戦略が，他の役割に対する `λ × action_payoffs` のソフトマックスに一致する組です．
`init`（なければ一様な組）から始めて，減衰つきの更新

```text
σ_i ← (1 − damping) σ_i + damping × softmax(λ F_i(·, σ_-i))
```

を，不動点の残差（`σ` とそのロジット応答の差の絶対値の最大）が `tol` 以下になるまで繰り返します．

| `QreOptions` | 既定値 | 意味 |
|---|---|---|
| `lambda` | （`QreOptions::new(lambda)` の引数） | 精度．有限で `0` 以上．`0` なら `init` によらず一様な組になる |
| `init` | `None`（一様） | 初期の組．`.with_init(profile)` で設定 |
| `damping` | `0.1` | `(0, 1]` の範囲．`.with_damping(d)` で設定 |
| `tol` | `1e-12` | 有限で正．`.with_tol(t)` で設定 |
| `max_iter` | `100_000` | 正．`.with_max_iter(n)` で設定 |

不正なオプションは `InvalidOptions`，形の合わない `init` は `MixedError` です．残差は返す組で計算し直し，
それが `tol` 以下のときに限って `QreSolution { lambda, strategies, residual, regrets, iterations }` を返します．
そうでなければ結果は `NotConverged { residual, iterations }` です．最後の反復値で代用することはありません．

**これは局所的な探索であって，主分枝の追跡ではありません．** 同じ λ でもゲームが複数のロジット QRE を持つことがあり，
反復は `init` からたどり着いた不動点に収束します．初期値が違えば別の QRE の分枝に着くことがあります（下の鹿狩りを参照）．
λ が大きいと減衰つきの反復が落ち着かないことがあります．そのときは減衰を小さくするか，λ を段階的に上げ，各解を次の `init` に渡してください．
こうすると多くの場合 1 本の分枝をたどれますが，QRE の対応の折り返しや分岐を越えられる保証はありません．

有限の λ ではロジット QRE は Nash 均衡ではありません．`regrets` は返した組での各役割の通常の（正則化しない）後悔です．
厳密なロジット QRE ではこれは `ln |S_i| / λ` 以下（`|S_i|` はその役割の行動の数）なので，λ が大きくなるほど小さくなります．
ソルバーはこの後悔を報告しますが，受け入れの判定には使いません．

### 例

**男女の争い：すべての均衡と 1 本の経路．** 台の列挙は 2 つの純粋戦略の均衡と混合戦略の均衡を見つけます．
Lemke–Howson は，どのラベルを落としても純粋戦略の均衡にしか着きません．

```rust
use socsim_games::catalog::BattleOfSexes;
use socsim_games::solve::{
    lemke_howson, pure_nash, support_enumeration, tolerance, LemkeHowsonOptions,
};

fn main() {
    // Role 0 prefers (B, B) with 2, role 1 prefers (V, V) with 2;
    // miscoordination gives both -1.
    let game = BattleOfSexes::default();
    println!("tol = {:e}", tolerance(&game).unwrap());

    let pure: Vec<_> = pure_nash(&game).unwrap().into_iter().map(|e| e.profile).collect();
    println!("pure: {pure:?}");

    let all = support_enumeration(&game).unwrap();
    println!("complete: {}", all.is_complete());
    for eq in &all.equilibria {
        println!(
            "  {:.4?} {:.4?} payoffs {:.4?}",
            eq.strategies[0].probs(),
            eq.strategies[1].probs(),
            eq.payoffs
        );
    }

    // Labels 0..2 are role 0's actions, 2..4 are role 1's.
    for label in 0..4 {
        let found = lemke_howson(&game, &LemkeHowsonOptions::default().with_dropped_label(label))
            .unwrap();
        println!(
            "  label {label}: {:?} {:?} ({} pivots)",
            found.equilibrium.strategies[0].probs(),
            found.equilibrium.strategies[1].probs(),
            found.pivots
        );
    }
}
```

```text
tol = 3e-10
pure: [[0, 0], [1, 1]]
complete: true
  [1.0000, 0.0000] [1.0000, 0.0000] payoffs [2.0000, 1.0000]
  [0.6000, 0.4000] [0.4000, 0.6000] payoffs [0.2000, 0.2000]
  [0.0000, 1.0000] [0.0000, 1.0000] payoffs [1.0000, 2.0000]
  label 0: [1.0, 0.0] [1.0, 0.0] (2 pivots)
  label 1: [0.0, 1.0] [0.0, 1.0] (2 pivots)
  label 2: [1.0, 0.0] [1.0, 0.0] (2 pivots)
  label 3: [0.0, 1.0] [0.0, 1.0] (2 pivots)
```

利得の幅は 2 − (−1) = 3 なので `tol = 3e-10` です．混合戦略の均衡では，各役割が自分の好む行動を確率 0.6 で選び，期待利得は 0.2 です．

**じゃんけん：ミニマックス．** 値は 0 で，最適戦略はどちらも一様です．男女の争いはゼロ和ではありませんが，各役割は保証水準を持ちます．

```rust
use socsim_games::catalog::{BattleOfSexes, RockPaperScissors};
use socsim_games::solve::{maximin_security_level, zero_sum_minimax};

fn main() {
    let rps = RockPaperScissors::default();
    let solution = zero_sum_minimax(&rps).unwrap();
    println!("value {:.6}", solution.value);
    println!("row {:.6?}", solution.row.probs());
    println!("col {:.6?}", solution.col.probs());
    println!("gap {:e}, regret {:e}", solution.duality_gap, solution.regret);

    // A general-sum game has no minimax value, but each role still has a
    // maximin guarantee.
    let bos = BattleOfSexes::default();
    assert!(zero_sum_minimax(&bos).is_err()); // NotZeroSum
    let guard = maximin_security_level(&bos, 0).unwrap();
    println!("BoS role 0 guarantees {:.6} with {:.6?}", guard.level, guard.strategy.probs());
}
```

```text
value 0.000000
row [0.333333, 0.333333, 0.333333]
col [0.333333, 0.333333, 0.333333]
gap 0e0, regret 5.551115123125783e-17
BoS role 0 guarantees 0.200000 with [0.400000, 0.600000]
```

`regret` の下位の桁は浮動小数点の丸めによるものです．ソルバーが保証するのは `regret ≤ tol` です．

**鹿狩り：QRE の分枝．** 一様な初期値から λ を上げていくと `(hare, hare)` へ向かう分枝をたどります．
λ = 5 で `(stag, stag)` から始めると，`(stag, stag)` に近い別の QRE に着きます．

```rust
use socsim_games::catalog::StagHunt;
use socsim_games::solve::{logit_qre, QreOptions};
use socsim_games::MixedStrategy;

fn main() {
    let game = StagHunt::default();

    // Raise lambda step by step, starting each solve from the previous one.
    let mut previous = None;
    for lambda in [0.5, 1.0, 2.0, 5.0] {
        let mut options = QreOptions::new(lambda);
        if let Some(init) = previous.take() {
            options = options.with_init(init);
        }
        let qre = logit_qre(&game, &options).unwrap();
        println!(
            "lambda {lambda}: P(stag) = {:.4}, regrets {:.4?}",
            qre.strategies[0].probs()[0],
            qre.regrets
        );
        previous = Some(qre.strategies);
    }

    // The same lambda from a different start lands on a different fixed point.
    let stag = MixedStrategy::pure(2, 0).unwrap();
    let qre = logit_qre(&game, &QreOptions::new(5.0).with_init(vec![stag.clone(), stag])).unwrap();
    println!("lambda 5 from (stag, stag): P(stag) = {:.4}", qre.strategies[0].probs()[0]);
}
```

```text
lambda 0.5: P(stag) = 0.0334, regrets [0.2246, 0.2246]
lambda 1: P(stag) = 0.0009, regrets [0.0064, 0.0064]
lambda 2: P(stag) = 0.0000, regrets [0.0000, 0.0000]
lambda 5: P(stag) = 0.0000, regrets [0.0000, 0.0000]
lambda 5 from (stag, stag): P(stag) = 0.9901
```

どの後悔も `ln 2 / λ`（λ = 0.5 で約 1.39）を下回っています．
