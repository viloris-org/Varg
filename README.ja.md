# Varg

[![CI](https://github.com/viloris-org/Varg/actions/workflows/core.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/core.yml)
[![Nightly](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.96+-orange.svg)

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-Hant.md) | 日本語 | [한국어](README.ko.md) | [Español](README.es.md)

Varg は、Rust ランタイム、Tauri/React デスクトップエディタ、AI 支援のオーサリングワークフローを中心に構築されている実験的なゲームエンジン兼エディタです。現在のコードベースは、安全な ECS/ランタイム基盤、ネイティブエディタシェル、Varg オーサリング言語、プロジェクトパッケージング、Quest/Copilot 型のエディタ自動化に重点を置いています。

このプロジェクトはまだ pre-1.0 です。一部のドキュメントは目標設計を説明していますが、この README は現在のリポジトリに存在する内容を追跡します。

![Varg エディタ](docs/screenshots/editor.png)

## クイックスタート

前提条件：

- [Rust](https://rustup.rs/) 1.96 以降
- エディタフロントエンド用の [Bun](https://bun.sh/)
- [Tauri v2 システム依存関係](https://v2.tauri.app/start/prerequisites/)

Debian/Ubuntu 系 Linux では、Tauri の依存関係には通常次が含まれます：

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

クローンしてエディタを起動します：

```sh
git clone https://github.com/viloris-org/Varg
cd Varg

cd editor
bun install
bun run dev:tauri
```

Rust workspace をビルドします：

```sh
cargo build --workspace
```

## 現在の機能

- **Rust ランタイム基盤**：ECS、プロジェクトマニフェスト、アセット、プラットフォーム入力、レンダリング trait、WGPU 統合、物理、オーディオ、UI、アニメーション、スケルトン、shader、policy、AI、パッケージング crate。
- **Tauri エディタ**：Hub/プロジェクトワークフロー、ビューポートホスト、Copilot、Quest、パッケージング、ダイアログ、ネイティブウィンドウ/パネルを Rust コマンドで支える React/TypeScript デスクトップアプリ。
- **Varg オーサリング言語**：`.varg`、`.vscene`、`.vasset` の解析、診断、MVP スクリプトランタイム、behavior 宣言、`varg-lsp` バイナリ。
- **宣言型スクリプト実験**：`engine-script-declarative` 配下の JSON behavior、scene、UI、system、project、asset 構造。
- **パッケージングパイプライン**：`cargo xtask package` はデスクトッププロジェクト用のランタイムフォルダを作成し、将来の target/format 組み合わせをいくつか検証します。
- **安全な Rust 方針**：エンジン crate は `#![forbid(unsafe_code)]` を使用します。

## プロジェクト構造

```text
Varg/
├── crates/                         # エンジンとランタイム crate
│   ├── engine-core/                # ID、エラー、数学、設定
│   ├── engine-ecs/                 # シーン、エンティティ、transform、コンポーネント
│   ├── engine-assets/              # アセット DB、インポーター、マニフェスト
│   ├── engine-render/              # レンダリング trait と共有レンダーモデル
│   ├── engine-render-wgpu/         # WGPU バックエンドとビューポート実験
│   ├── engine-platform/            # ウィンドウ、入力、ファイルシステム抽象
│   ├── engine-script-varg/         # Varg パーサー、診断、ランタイム、LSP
│   ├── engine-script-declarative/  # 宣言型 JSON オーサリング実験
│   ├── engine-editor/              # エディタサービスと AI/ツール支援
│   ├── engine-packager/            # プロジェクトパッケージングパイプライン
│   └── runtime-min/                # ランタイム合成ルート
├── editor/                         # Tauri/React デスクトップエディタ
├── examples/                       # サンプルプロジェクト、behavior、script
├── docs/                           # 設計メモ、PRD、ADR
├── schema/                         # JSON schema
├── scripts/                        # ユーティリティスクリプトとテスト
└── xtask/                          # workspace 自動化コマンド
```

## エディタ開発

```sh
cd editor
bun install

bun run dev:tauri
bun run build
bun run tauri build
```

よく使うパス：

- Renderer UI：`editor/src/renderer/`
- Tauri コマンドとホストサービス：`editor/src-tauri/src/`
- Tauri 権限：`editor/src-tauri/capabilities/`

## ランタイム Feature

`runtime-min` は合成 crate です。Feature セットはルート `Cargo.toml` の workspace metadata にも記載されています。

| Feature | 用途 |
|---|---|
| `runtime-min` | 最小ヘッドレスランタイムパス |
| `runtime-game` | アセットインポートとウィンドウ対応を含むランタイムパス |
| `wgpu` | WGPU レンダリングバックエンド |
| `physics` | 物理サブシステム |
| `audio` | オーディオサブシステム |
| `editor` | エディタ向けサービス |
| `agent-tools` | AI/エディタツール支援 |
| `dev-full` | ランタイム、エディタ、Agent、物理、オーディオ、shader、2D/UI、アニメーション、スケルトンを含む広めの開発ビルド |

例：

```sh
cargo build -p runtime-min --no-default-features --features runtime-min
cargo build -p runtime-min --no-default-features --features runtime-game,wgpu,physics,audio
cargo xtask runtime-min
cargo xtask build-editor
```

## Varg 言語ツール

言語サーバーを起動します：

```sh
cargo xtask varg-lsp
```

`.vscene` を scene JSON にコンパイルします：

```sh
cargo xtask vscene compile path/to/input.vscene --out path/to/output.scene.json
```

目標言語の方向性は [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md) にあります。この文書には MVP ランタイムを超える予定構文も含まれています。

## プロジェクトのパッケージング

現在のデスクトップホスト向けに既定のサンプルプロジェクトをパッケージします：

```sh
cargo xtask package --project examples/project/fps_arena --target native --format folder --debug
```

フォルダパッケージはプロジェクト配下に書き込まれます。例：

```text
examples/project/fps_arena/exports/<project>/<target>/<channel>/
```

ランタイムバイナリ、ランチャースクリプト、コピー済みプロジェクトペイロード、アセットマニフェスト、`package-manifest.json` が含まれます。

現在のパッケージング状況：

| Target | 現在の対応 |
|---|---|
| `native`、`linux-x64`、`windows-x64`、`macos-universal` | 対応するデスクトップホストで `folder` パッケージを生成 |
| `android-arm64` | ツールチェーン検証あり。署名済み APK/AAB 生成は未実装 |
| `ios-universal` | ツールチェーン検証あり。署名済み IPA 生成は未実装 |
| デスクトップインストーラー（`appimage`、`deb`、`rpm`、`exe`、`msi`、`nsis`、`dmg`） | CLI は認識しますが、Varg プロジェクトパッケージ生成は現在 unsupported capability を返します |

## テストとチェック

```sh
cargo test --workspace
cargo xtask check
cargo fmt --check
cargo clippy --workspace

cargo test -p runtime-min --no-default-features --features runtime-min
cargo test -p engine-render-wgpu
cargo test -p engine-editor --no-default-features --features agent-tools

pytest scripts/tests
```

## ドキュメント

- [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md)：Varg オーサリング言語の方向性と MVP サブセットの説明。
- [`docs/ai-agent-unified-spec.md`](docs/ai-agent-unified-spec.md)：AI Agent ワークフローの方向性。
- [`docs/quest-workflow-ui-reference.md`](docs/quest-workflow-ui-reference.md)：Quest ワークフロー UI リファレンス。
- [`docs/adr/`](docs/adr/)：アーキテクチャ決定記録。

## ライセンス

Mozilla Public License 2.0。詳細は [LICENSE](LICENSE) を参照してください。
