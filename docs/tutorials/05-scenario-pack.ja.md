[English](05-scenario-pack.md) | **日本語**

# T5 — シナリオパック

**作るもの：** フルスタックパス — メカニズムを `ModulePack` にまとめ，`Registry` に登録し，Rustとシナリオ `.toml` の両方から駆動し，パックが `socsim` CLIのサブコマンドになる仕組みを理解します．
**所要時間：** 50分．

## 前提

- [T1 — 最初のモデル](01-first-model.ja.md)（`Mechanism`，`SimulationBuilder`，シード）．
- T0（`socsim run` / `list` / `sweep` に慣れていること）．

裏付けの成果物（どちらもCIコンパイル済み）：ライブラリドライバ [`crates/socsim-packs/examples/hr_baseline.rs`](../../crates/socsim-packs/examples/hr_baseline.rs)，それが駆動する `HrLifecyclePack`（および [`crates/socsim-packs/src/opinion.rs`](../../crates/socsim-packs/src/opinion.rs) の `opinion-dynamics` パック）．

## 2つのパス，1つのエンジン

ここまでは `SimulationBuilder` にメカニズムを直接追加してきました（T1–T4 — *engine-only* モード）．**フルスタック** パスは，あなたとエンジンの間に2つの層を挟みます：`ModulePack`（名前付きのメカニズムコンストラクタの束）と `Registry`（パラメータから名前でメカニズムを構築）．これによりシナリオ `.toml` — またはCLI — が再コンパイルなしにモデルを構成できます．[2つの利用パス](../architecture.ja.md#2つの利用経路シナリオcli-vs-ライブラリモード) を参照してください．

## ステップ

### 1. `ModulePack` がメカニズムコンストラクタを登録する

`ModulePack<W>` は名前と，`Registry<W>` に名前付きコンストラクタを追加する `register` メソッドを持ちます．各コンストラクタは型付きパラメータを読み，ボックス化したメカニズムを返します．`opinion-dynamics` パックはコンパクトな実例です — 各意見メカニズムを名前で登録します：

```rust
reg.register("hegselmann_krause", |p: &Params| {
    let epsilon = p.get_f64("epsilon", 0.2);
    let p_fallback = p.get_f64("p", 1.0);
    let mean = parse_mean(p.get_str("mean", "A"), p_fallback)
        .map_err(socsim_core::SocsimError::Config)?;
    Ok(Box::new(HegselmannKrauseMechanism::new(epsilon, mean))
        as Box<dyn socsim_core::Mechanism<OpinionWorld>>)
});
```

クロージャは `Params`（TOMLテーブルに対する型付き・既定値付きのビュー）を受け取ります：`get_f64("epsilon", 0.2)` はシナリオの値を読むか，なければ `0.2` にフォールバックします．キーを省いたシナリオでも動くよう，必ず既定値を与えてください．リファレンスの `HrLifecyclePack` も10メカニズムに対して同じことをします．

### 2. レジストリからメカニズムを構築する（ライブラリのフルスタック）

同梱の `hr_baseline.rs` はフルスタックパスを *Rustで* 示します：パックを `Registry` に登録し，名前で各メカニズムを構築してビルダーに追加します：

```rust
// Register all mechanisms.
let mut reg = socsim_config::Registry::new();
HrLifecyclePack.register(&mut reg);

let p = Params::empty();
let mechanism_names = [
    "learning_curve", "peer_effect", "ocb", "fit", "turnover",
    "knowledge_loss", "toxic_spread", "hiring", "socialization", "org_performance",
];

let mut builder = SimulationBuilder::new(world)
    .scheduler(Box::new(RandomActivationScheduler))
    .seed(SEED)
    .recorder(Box::new(shared_rec));

for name in &mechanism_names {
    let m = reg.build(name, &p).expect("mechanism registered");
    builder = builder.add_mechanism(m);
}

let mut sim = builder.build();
sim.run().expect("simulation completed without error");
```

`reg.build(name, &params)` が橋渡しです：登録したコンストラクタを引き，それらのパラメータでメカニズムをインスタンス化します．CLIがシナリオの `[[mechanism]]` ブロックを読むとき内部で行っているのもこれです — ここでは名前をRustで明示しているだけです．

### ライブラリドライバを実行する

```sh
cargo run -p socsim-packs --example hr_baseline
```

```
=== HR Lifecycle ABM — Baseline Run ===
Teams: 5  |  Initial team size: 8  |  T_max: 60  |  Seed: 42

Initial employees: 40  |  Base mean θ: 0.9516

   t  org_performance    avg_tenure   turnover_rate   knowledge_stock
----------------------------------------------------------------------
   1          6.2045          1.00          0.0000             42.61
   ...
  60         41.8100         35.62          0.0000             92.38
```

### 3. 同じモデルをシナリオ `.toml` として

メカニズムをRustで列挙する代わりに，シナリオファイルはパックを指定し `[[mechanism]]` ブロックを並べます．これが `scenarios/hr_lifecycle_baseline.toml` です：

```toml
[simulation]
name        = "hr_lifecycle_baseline"
module_pack = "hr-lifecycle"
t_max       = 60
seed        = 42
scheduler   = "random_activation"

[[mechanism]]
name  = "learning_curve"
phase = "environment"
[mechanism.params]
lambda_learn = 0.15

# ... eight more [[mechanism]] blocks ...

[output]
log_path = "runs/{name}_{seed}.jsonl"
metrics  = ["org_performance", "avg_tenure", "turnover_rate", "knowledge_stock"]
```

`module_pack` がパックを選びます．各 `[mechanism.params]` テーブルがコンストラクタの読む `Params` です．配列は順序保存 — 構成順 = 宣言順です．CLIで実行します（シードとパラメータが一致するので，Rustドライバと同じ数値になります）：

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
socsim sweep scenarios/hr_lifecycle_baseline.toml --param "toxic_spread.p_spread=0.2,0.7" --seeds 0..2
```

シナリオの全スキーマと全フラグは [CLIリファレンス](../cli.ja.md) にあります．タスクのレシピ（マルチシード検証，スイープ，実行の再開）は [ユースケース＆レシピ](../usecases.ja.md) を参照してください．

### 4. （任意）パックを `CliPack` として `socsim` バイナリに公開する

同梱の3パックが `socsim list packs` に現れるのは，それぞれが **`CliPack`** — World多態のバイナリがディスパッチする，オブジェクト安全でWorldを消去したアダプタ — にも包まれているからです．自分のパックをCLIに追加するには：

1. `impl CliPack` する `struct FooCliPack;` を実装する（内部で具体的なワールドを所有し，`name`，`starter_toml`，`mechanism_names`，`run_seeds`，`run_sweep` を公開）；
2. Cargoフィーチャ `pack-foo = ["dep:socsim-foo"]` を追加する；
3. impl を `#[cfg(feature = "pack-foo")]` でゲートする；
4. `packs()` レジストリに push する．

このチェックリストは [`crates/socsim-cli/src/packs.rs`](../../crates/socsim-cli/src/packs.rs) の冒頭にあります．配線が済むと `socsim list packs` に現れ，`socsim init --module-pack foo` が雛形を作ります．それまでは，ライブラリのフルスタックパス（ステップ2）でCLIバイナリに一切触れずに任意のパックを実行できます．

## 実行する

```sh
cargo run -p socsim-packs --example hr_baseline      # library full-stack
socsim run scenarios/hr_lifecycle_baseline.toml      # same model, scenario TOML
socsim list packs                                    # packs exposed as CliPacks
```

## 学んだこと

- **`ModulePack`** は名前付きメカニズムコンストラクタを **`Registry`** に登録します．`reg.build(name, &params)` がそれらをインスタンス化します — TOML/CLIが再コンパイルなしにモデルを構成できる間接層です．
- **`Params`** はシナリオの `[mechanism.params]` テーブルを型付き・既定値付きで読みます．
- ライブラリのフルスタックパス（`hr_baseline.rs`）とシナリオTOMLパスは，*同じ* 登録済みメカニズムを同じエンジンで実行します．
- パックは `pack-*` フィーチャの背後で **`CliPack`** を実装することで `socsim` バイナリに届きます．それまではライブラリとして実行できます．

## 次へ

パスは完走です．ここからは：

- [ユースケース＆レシピ](../usecases.ja.md) — 実研究ワークフローのタスク指向ランブック．
- [Mechanismカタログ](../mechanisms.ja.md) — 構成できる全同梱メカニズム．
- [アーキテクチャ](../architecture.ja.md) — クレートグラフと設計の *なぜ*．
