# Aster

[![CI](https://github.com/viloris-org/Aster/actions/workflows/core.yml/badge.svg)](https://github.com/viloris-org/Aster/actions/workflows/core.yml)
[![Nightly](https://github.com/viloris-org/Aster/actions/workflows/nightly.yml/badge.svg)](https://github.com/viloris-org/Aster/actions/workflows/nightly.yml)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.78+-orange.svg)

[English](README.md) | [简体中文](README.zh-CN.md) | 日本語

Aster は AI ネイティブなゲームエンジンです。自然言語で作りたいゲームを説明すれば、自律エージェント群がシーン、ロジック、UI まですべて構築します。本格的なビジュアルエディタも搭載しており、細部の調整や仕上げも思いのままです。

![Aster Editor](docs/screenshots/editor.png)

> **スクリーンショットプレースホルダー** — UI が安定したら実際のエディタ画像に差し替えてください。

## クイックスタート

```sh
git clone https://github.com/viloris-org/Aster
cd Aster

# エディタを起動
cd editor
bun install
bun tauri dev
```

> **前提条件：** [Rust ≥ 1.78](https://rustup.rs/)、[Bun ≥ 1.0](https://bun.sh/)、
> [Tauri システム依存関係](https://v2.tauri.app/start/prerequisites/)。
> Linux ユーザー：`sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev
> libayatana-appindicator3-dev librsvg2-dev`

## 機能

- **AI ネイティブコア** — 単なる後付けの AI アシスタントではなく、複数のエージェントが自律的に計画・構築・レビューします。自然言語を入力し、プレイ可能なシーンを出力。サンドボックスレビューがプロジェクトを安全に保ちます。
- **宣言型ゲーム記述** — 6 つの宣言型システム（ビヘイビアツリー、シーングラフ、UI レイアウト、システム設定、アセットマニフェスト、プロジェクト構造）により、エージェントはコードの代わりに構造化 JSON を生成。LLM の成功率が約 50% から約 90% に向上します。
- **ビジュアルシーンエディタ** — 直感的なインターフェースでオブジェクトの配置、トランスフォーム調整、コンポーネント追加が可能。AI に重労働を任せ、細部は手作業で磨き上げる、両方のいいとこ取りです。
- **ライブプレイモード** — Play を押せば物理とスクリプトが実行され、Stop でゼロクリーンアップ。編集シーンは決して変更されません。
- **アセットパイプライン** — glTF/PNG をプロジェクトパネルにドロップ。ファイルウォッチャーが自動インポートをトリガーし、ホットリロードが即座に反映されます。
- **プラグ可能なレンダリング** — エンジンコードに触れずにバックエンドを切り替え可能。WGPU 搭載、Vulkan は開発中。
- **ヘッドレスランタイム** — 同じエンジンがサーバー、CI パイプライン、自動ビルドで動作。ウィンドウ不要。
- **ゼロ unsafe コード** — すべての crate が `#![forbid(unsafe_code)]` を使用。デフォルトで安全。

## プロジェクト構造

```
Aster/
├── editor/                  # Tauri デスクトップアプリ（React + Rust）
├── crates/
│   ├── engine-editor/       # エディタワークフロー、サービス、Agent ツール
│   ├── engine-ecs/          # シーン、エンティティ、トランスフォーム、ワールド
│   ├── engine-assets/       # データベース、インポーター、ホットリロード
│   ├── engine-render/       # レンダーグラフ、デバイストレイト
│   ├── engine-render-wgpu/  # WGPU バックエンド
│   ├── engine-render-vulkan/# Vulkan バックエンド（開発中）
│   ├── engine-physics/      # 物理（rapier3d）
│   ├── engine-audio/        # オーディオパイプライン
│   ├── engine-core/         # ID、エラー、数学、設定
│   ├── engine-platform/     # ウィンドウ、入力、ファイルシステム
│   ├── engine-script-rhai/  # Rhai スクリプト
│   ├── engine-animation/    # アニメーションシステム
│   ├── engine-ai/           # AI プランナーとシステムプロンプト
│   ├── engine-agent-cluster/# Agent オーケストレーション
│   ├── runtime-min/         # コンポジションルート
│   └── …                    # i18n、shader、policy、skeleton 等
├── xtask/                   # ビルドと自動化タスク
├── examples/                # サンプルプロジェクトとシーン
└── docs/                    # 設計メモ
```

## シーン編集

1. エディタ起動 → **Hub** 画面
2. プロジェクトを作成または開く
3. **Hierarchy** パネルにシーン内の全オブジェクトが表示
4. **Inspector** で選択オブジェクトのトランスフォームとコンポーネントを確認
5. **Scene View** で 3D ビューポートをレンダリング — 回転、パン、ズーム
6. **Play** をクリックして **Game View** で物理とスクリプトを実行
7. コンポーネント（Camera、Light、MeshRenderer、Rigidbody、Collider…）を追加、または Rhai スクリプトを作成

## ビルドプロファイル

プロファイルはコンパイル時にリンクするサブシステムを選択します：

| プロファイル | 内容 |
|---|---|
| `editor` | Tauri フロントエンド向けのエディタサービス、wgpu ビューポート、Agent ツール |
| `runtime-min` | ヘッドレス — CI テスト、サーバー、自動ビルド |
| `runtime-game` | ヘッドレス + ウィンドウ表示 |
| `dev-full` | すべて：エディタ、物理、オーディオ、スクリプト、Agent、レンダリング |

```sh
cargo build -p runtime-min --no-default-features --features editor
cargo build -p runtime-min --no-default-features --features runtime-min
```

## エディタのビルド

```sh
cd editor
bun install

# 開発モード（フロントエンド + Rust バックエンドのホットリロード）
bun tauri dev

# 配布用バンドル
bun tauri build
# → editor/src-tauri/target/release/bundle/
```

## テスト

```sh
# 完全なエンジンテストスイート
cargo test --workspace

# ヘッドレスランタイムのみ（高速）
cargo test -p runtime-min --no-default-features --features runtime-min

# エディタサービス
cargo test -p engine-editor --no-default-features --features agent-tools

# WGPU バックエンド
cargo test -p engine-render-wgpu
```

## ライセンス

Mozilla Public License 2.0。詳細は [LICENSE](LICENSE) を参照してください。
