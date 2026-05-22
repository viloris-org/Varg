# Aster

[English](README.md) | [简体中文](README.zh-CN.md) | 日本語

Aster は、Rust 製ゲームエンジンの初期段階のワークスペースです。小さなネイティブランタイム、明確なサブシステム境界、エディタ向けデータ形式、feature によるエンジン構成を重視しています。

このプロジェクトは、まだ本番向けエンジンではありません。ランタイム、アセットパイプライン、レンダリング抽象、エディタシェル、パッケージングフローを構築して検証するためのワークスペースです。

## 目標

- 最小ランタイムを小さく保ち、測定可能にする。
- エンジンサブシステムを明示的でテスト可能にし、feature ごとに独立して制御できるようにする。
- エディタワークフロー、自動化、将来のマイグレーションに適したデータ形式を使う。
- 一時的なスクリプトではなく `xtask` でリポジトリ自動化を提供する。
- サンプルプロジェクト設定を汎用的かつ再現可能に保つ。

## ワークスペース

主要なエンジンコードは `crates/` にあります。

- `engine-core`: 共有 ID、ハンドル、エラー、ログ、数学、時間、ランタイム設定。
- `engine-ecs`: シーン、エンティティ、トランスフォーム、ワールド、schema、物理、オーディオの基礎。
- `engine-platform`: ウィンドウ、入力、ファイルシステム、動的ライブラリ、コールバックのプラットフォーム境界。
- `engine-assets`: アセットデータベース、リソースレジストリ、manifest、依存グラフ、インポートキュー、ホットリロード追跡、リソースデータ形式。
- `engine-render`: レンダラー向け抽象、レンダーグラフ、ターゲット、リソース、パイプライン、ヘッドレスレンダーデバイス。
- `engine-render-wgpu`: WGPU ベースのレンダリング統合。
- `engine-render-vulkan`: Vulkan 向けレンダリング統合の足場。
- `engine-physics`: 物理統合サーフェス。
- `engine-audio`: オーディオ統合サーフェス。
- `engine-editor`: エディタワークフロー、ネイティブエディタサービス、レンダーフック、物理フック、agent ツール。
- `engine-editor-ui`: egui ベースのエディタシェル、パネル、ウィジェット、フォント、UI 状態。
- `engine-i18n`: ローカライズ読み込みと同梱ロケールファイル。
- `engine-script-rhai`: Rhai スクリプト統合。
- `engine-cli` / package `aster`: エディタ優先のランチャーとコマンドラインツール。
- `runtime-min`: 最小ランタイム profile と feature 合成されたランタイム入口。
- `xtask`: リポジトリ自動化入口。

サンプルプロジェクトデータは `examples/project/` にあります。

- `aster.project.toml`: プロジェクト manifest。
- `build.runtime-min.toml`: サンプルのランタイムビルド設定。
- `editor.preferences.toml`: サンプルのエディタ設定。
- `assets/`: サンプルのマテリアルアセットとメタデータ。
- `scenes/`: サンプルのシーンデータ。
- `prefabs/`: サンプルの prefab データ。

設計メモや計画メモは `docs/` に置きます。

## ビルド Profile

ランタイム構成は Cargo features で制御します。

- `runtime-min`: エディタ、スクリプト、重いインポータ、物理、オーディオ、具体的なレンダリングバックエンドを含まない最小ネイティブランタイム。
- `runtime-game`: 最小 profile の上に構成されるゲームランタイムサーフェス。
- `wgpu`: WGPU が必要なランタイムビルド向けのレンダリングバックエンド。
- `physics`: 任意の物理サポート。
- `audio`: 任意のオーディオサポート。
- `editor`: エディタ向けワークフローとデータ。
- `agent-tools`: 自動化と agent 統合サーフェス。
- `script-python`: Python スクリプトバックエンド統合サーフェス。
- `dev-full`: ローカル開発用のフル profile。

重いアセットインポータは `engine-assets` の `fbx-importer`、`assimp-importer`、`heavy-importers` で feature-gated されています。これらを無効にすると、最小ランタイムビルドに関連依存を入れずに済みます。

## 要件

- Rust 1.78 以降。
- Rust 2021 edition に対応した Cargo ツールチェーン。
- エディタまたはレンダリング機能をビルドする場合は、`winit`、`egui`、`wgpu`、Vulkan に必要な各プラットフォームのグラフィックス依存関係。

## 開発

ワークスペース全体のテストを実行します。

```sh
cargo test --workspace
```

すべての feature を有効にして全 crate を型チェックします。

```sh
cargo check --workspace --all-features
```

最小ランタイム feature パスを確認します。

```sh
cargo test -p runtime-min --no-default-features --features runtime-min
```

同じ一般的なタスクをリポジトリ自動化経由で実行します。

```sh
cargo run -p xtask -- test
cargo run -p xtask -- check
```

最小ランタイム profile をビルドします。

```sh
cargo run -p xtask -- runtime-min
```

エディタ profile をビルドします。

```sh
cargo run -p xtask -- build-editor
```

agent ツールの smoke パスを実行します。

```sh
cargo run -p xtask -- agent-smoke
```

## CLI

`aster` package は、エディタ優先のランチャーとコマンドラインツールを提供します。

利用可能な CLI コマンドを表示します。

```sh
cargo run -p aster
```

よく使うコマンド:

```sh
cargo run -p aster -- profiles
cargo run -p aster -- smoke runtime-min
cargo run -p aster -- run examples/project
cargo run -p aster -- build examples/project
```

## パッケージング

エディタ profile でサンプルプロジェクトをパッケージングします。

```sh
cargo run -p xtask -- package --profile editor --project examples/project
```

出力先:

```text
target/aster-packages/<platform>/<profile>/
```

ネイティブパッケージングは現在 `runtime-game` と `editor` profile をサポートしています。profile を渡さない場合、`xtask package` はサンプルプロジェクトのランタイムビルド設定を読みます。

## テスト

Rust の組み込みテストフレームワークを使います。crate の統合テストは `crates/<crate>/tests/` に置き、ユニットテストは対象コードの近くに置きます。feature-gated なコードを変更した場合は、ワークスペース全体のテストに加えて、対象 feature のコマンドも実行します。

便利な対象別チェック:

```sh
cargo test -p runtime-min --no-default-features --features runtime-min
cargo test -p engine-editor --no-default-features --features agent-tools
cargo test -p engine-render-wgpu
```

## リポジトリ運用

- Rust コードは `cargo fmt --workspace` でフォーマットする。
- 依存関係はルート `Cargo.toml` の workspace dependencies を優先する。
- crate 名は kebab case、Rust のモジュール、ファイル、関数、変数は snake case にする。
- 生成された `target/` 出力やローカル環境の秘密情報をコミットしない。
- `examples/project/` のサンプル設定は汎用的に保つ。
- 変更が明示的に必要としていない限り、最小ランタイム関連の変更で重いインポータを有効にしない。

## ライセンス

Aster は Mozilla Public License 2.0 の下でライセンスされています。詳細は `LICENSE` を参照してください。
