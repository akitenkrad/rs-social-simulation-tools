[English](games.md) | **日本語**

# 戦略形ゲーム（`socsim-games`）

`socsim-games` は socsim のエンジン上でゲーム理論のシミュレーションを走らせる crate です．
エージェントは**段階ゲーム**の対戦の席に着き，各自が**決定規則**で行動を選び，
**利得規則**が行動の組を利得に変え，規則は起きたことから学習します．
これを毎ステップ，通常の [6フェーズティックループ](architecture.ja.md#6フェーズティックループ) の中で行います．

この crate はライブラリ専用です（`ModulePack` を持たず，`socsim` バイナリには組み込まれていません）．
このページでは，部品，その状態の置き場所，規則が見てよい情報，実行可能なシミュレーションの組み立て方を説明します．

---

## Cargo feature

| feature | 追加されるもの | socsim への依存 |
|---|---|---|
| *（既定）* | 段階ゲームのトレイト（`StageGame`，`NormalFormGame`，`FiniteGame`）と `MatrixGame` | **なし** — 既定のビルドはどの `socsim-*` crate にも依存しません |
| `core` | World（`GameWorld`），規則，利得規則，Matching，ビュー，`build_world` | `socsim-core` |
| `engine` | `assemble`（`SimulationBuilder` を返す）．`core` を含意 | `socsim-core`，`socsim-engine` |

この crate は `socsim-llm`，`socsim-config`，`socsim-runner` のいずれにも依存しません．
シミュレーションを走らせるには両方の feature が要ります．

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

## 決定論性と乱数ストリーム

すべてのストリームは，`build_world` と `assemble` に渡す 1 つの `seed` から派生します．

| ラベル | ストリーム |
|---|---|
| `derive_seed(seed, &[0])` | World の初期化（`Assignment::Shuffled`） |
| `derive_seed(seed, &[1])` | エンジン（スケジューラー，およびステップの RNG を通した `Matching` と `decide` の乱数） |
| `derive_seed(seed, &[2])` | `decision_root`．World に保存され，決定時のストリームのために予約されている |
| `derive_seed(seed, &[3, agent, generation])` | 規則の状態の初期化（`RuleInitContext::init_seed`） |

したがってゲームのシミュレーションではラベル `[2]` と `[3]` が予約済みです．独自のストリームには `[4]` 以上を使ってください
（[RNG ストリームのラベル付け規約](library.ja.md#rngストリームのラベル付け規約) を参照）．
学習は `AgentId` の昇順で，決定はスケジューラーの活性化の順で行われます．同じ seed と同じコードなら，実行はステップごとに再現されます．
