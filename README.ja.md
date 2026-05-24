<p align="center"><img src="docs/assets/hero.svg" width="100%"></p>

[English](README.md) | **日本語**

# rs-social-simulation-tools

![Rust 2021](https://img.shields.io/badge/Rust-2021-orange)
![License: MIT](https://img.shields.io/badge/License-MIT-blue)
![tests: 130 passing](https://img.shields.io/badge/tests-130%20passing-brightgreen)

`socsim` はRustで書かれた，コンポーザブルなエージェントベース社会シミュレーションプラットフォームです．トレイトベースのメカニズムシステム，シードされたChaCha20 RNGによる決定論的再現性，ソーシャルネットワーク層，空間グリッドのプリミティブ，保存・再開のためのWorld状態スナップショット，オプションの学習ポリシー（MARL），そしてシナリオの実行・パラメータスイープ・集計のためのCLIを，11クレートのワークスペースとして提供します．参考実装として，文献に基づくキャリブレーションパラメータを持つ10メカニズムのHRライフサイクルモジュールが同梱されています．

## インストール

ソースからビルド（Rustツールチェーンが必要）：

```sh
git clone https://github.com/akitenkrad/rs-social-simulation-tools.git
cd rs-social-simulation-tools
cargo build --release
```

バイナリは `target/release/socsim` に生成されます．

テストスイートの実行：

```sh
cargo test --workspace
```

## クイックスタート

```sh
socsim run scenarios/hr_lifecycle_baseline.toml
```

出力例：

```
Running 'hr_lifecycle_baseline' (pack=hr-lifecycle, t_max=60, seeds=[42], parallel=false)

Seed 42 — 82 events recorded

t             avg_tenure   knowledge_stock   org_performance     turnover_rate
10                9.1000           53.9517           32.1462            0.0000
20               14.6000           62.4468           35.7133            0.0000
30               21.5500           72.5042           40.4270            0.0250
40               25.9000           78.4727           40.2186            0.0000
50               30.0750           85.3493           40.8007            0.0000
60               35.6250           92.3841           41.8100            0.0000
```

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [設計概要](docs/design.ja.md) | コンセプトと設計思想，主要トレイト/構造体，6フェーズ実行モデル |
| [CLIリファレンス](docs/cli.ja.md) | 全サブコマンド，フラグ，JSONL出力形式 |
| [ユースケース＆レシピ](docs/usecases.ja.md) | 代表的な研究ワークフローのランブック |
| [ライブラリAPI](docs/library.ja.md) | カスタムメカニズムの実装とライブラリとしての利用 |
| [Mechanismカタログ](docs/mechanisms.ja.md) | 全11メカニズム：理論，出典，図解，フェーズ上の位置付け，各メカニズムの適用方法 |
| [アーキテクチャ](docs/architecture.ja.md) | クレート依存グラフ，6フェーズティックループ，キャリブレーション哲学 |

## ライセンス

MIT — [LICENSE](LICENSE) を参照してください．
