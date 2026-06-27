# Varg

[![CI](https://github.com/viloris-org/Varg/actions/workflows/core.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/core.yml)
[![Nightly](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.96+-orange.svg)

[English](README.md) | [简体中文](README.zh-CN.md) | 繁體中文 | [日本語](README.ja.md) | [한국어](README.ko.md) | [Español](README.es.md)

Varg 是一個實驗性的遊戲引擎與編輯器，圍繞 Rust 執行時、Tauri/React 桌面編輯器，以及 AI 輔助創作工作流構建。目前程式庫聚焦於安全的 ECS/執行時基礎、原生編輯器外殼、Varg 創作語言、專案打包，以及 Quest/Copilot 風格的編輯器自動化。

專案仍處於 pre-1.0 階段。部分文件描述的是目標設計，而本 README 追蹤的是目前倉庫中已體現的內容。

![Varg 編輯器](docs/screenshots/editor.png)

## 快速開始

前置需求：

- [Rust](https://rustup.rs/) 1.96 或更新版本
- 用於編輯器前端的 [Bun](https://bun.sh/)
- [Tauri v2 系統依賴](https://v2.tauri.app/start/prerequisites/)

在 Debian/Ubuntu 類 Linux 發行版上，Tauri 依賴通常包括：

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

克隆並執行編輯器：

```sh
git clone https://github.com/viloris-org/Varg
cd Varg

cd editor
bun install
bun run dev:tauri
```

構建 Rust workspace：

```sh
cargo build --workspace
```

## 目前能力

- **Rust 執行時基礎**：ECS、專案清單、資源、平台輸入、渲染 trait、WGPU 整合、物理、音訊、UI、動畫、骨架、shader、policy、AI 與打包 crate。
- **Tauri 編輯器**：React/TypeScript 桌面應用，由 Rust 命令支援 Hub/專案工作流、視埠宿主、Copilot、Quest、打包、對話框與原生視窗/面板。
- **Varg 創作語言**：`.varg`、`.vscene`、`.vasset` 解析、診斷、MVP 腳本執行時、行為宣告與 `varg-lsp` 二進位。
- **宣告式腳本實驗**：`engine-script-declarative` 下的 JSON 行為、場景、UI、系統、專案與資源結構。
- **打包管線**：`cargo xtask package` 可為桌面專案構建執行時資料夾，並驗證若干未來 target/format 組合。
- **安全 Rust 策略**：引擎 crate 使用 `#![forbid(unsafe_code)]`。

## 專案結構

```text
Varg/
├── crates/                         # 引擎與執行時 crate
│   ├── engine-core/                # ID、錯誤、數學、設定
│   ├── engine-ecs/                 # 場景、實體、transform、元件
│   ├── engine-assets/              # 資源資料庫、匯入器、清單
│   ├── engine-render/              # 渲染 trait 與共享渲染模型
│   ├── engine-render-wgpu/         # WGPU 後端與視埠實驗
│   ├── engine-platform/            # 視窗、輸入、檔案系統抽象
│   ├── engine-script-varg/         # Varg 解析器、診斷、執行時、LSP
│   ├── engine-script-declarative/  # 宣告式 JSON 創作實驗
│   ├── engine-editor/              # 編輯器服務與 AI/工具支援
│   ├── engine-packager/            # 專案打包管線
│   └── runtime-min/                # 執行時組合根
├── editor/                         # Tauri/React 桌面編輯器
├── examples/                       # 範例專案、行為與腳本
├── docs/                           # 設計筆記、PRD 與 ADR
├── schema/                         # JSON schema
├── scripts/                        # 工具腳本與測試
└── xtask/                          # workspace 自動化命令
```

## 編輯器開發

```sh
cd editor
bun install

bun run dev:tauri
bun run build
bun run tauri build
```

常用路徑：

- Renderer UI：`editor/src/renderer/`
- Tauri 命令與宿主服務：`editor/src-tauri/src/`
- Tauri 權限：`editor/src-tauri/capabilities/`

## 執行時 Feature

`runtime-min` 是組合 crate。Feature 集也列於根 `Cargo.toml` 的 workspace metadata。

| Feature | 用途 |
|---|---|
| `runtime-min` | 最小無頭執行時路徑 |
| `runtime-game` | 含資源匯入與視窗支援的執行時路徑 |
| `wgpu` | WGPU 渲染後端 |
| `physics` | 物理子系統 |
| `audio` | 音訊子系統 |
| `editor` | 編輯器服務 |
| `agent-tools` | AI/編輯器工具支援 |
| `dev-full` | 較完整的開發構建，包含執行時、編輯器、Agent、物理、音訊、shader、2D/UI、動畫與骨架功能 |

範例：

```sh
cargo build -p runtime-min --no-default-features --features runtime-min
cargo build -p runtime-min --no-default-features --features runtime-game,wgpu,physics,audio
cargo xtask runtime-min
cargo xtask build-editor
```

## Varg 語言工具

執行語言伺服器：

```sh
cargo xtask varg-lsp
```

將 `.vscene` 編譯為場景 JSON：

```sh
cargo xtask vscene compile path/to/input.vscene --out path/to/output.scene.json
```

目標語言方向見 [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md)。該文件包含一些超出 MVP 執行時的計畫語法。

## 打包專案

為目前桌面宿主打包預設範例專案：

```sh
cargo xtask package --project examples/project/fps_arena --target native --format folder --debug
```

資料夾包會寫入專案目錄下，例如：

```text
examples/project/fps_arena/exports/<project>/<target>/<channel>/
```

其中包含執行時二進位、啟動腳本、複製後的專案內容、資源清單與 `package-manifest.json`。

目前打包狀態：

| Target | 目前支援 |
|---|---|
| `native`、`linux-x64`、`windows-x64`、`macos-universal` | 在匹配桌面宿主上生成 `folder` 包 |
| `android-arm64` | 已有工具鏈驗證；尚未實作簽名 APK/AAB 生成 |
| `ios-universal` | 已有工具鏈驗證；尚未實作簽名 IPA 生成 |
| 桌面安裝包（`appimage`、`deb`、`rpm`、`exe`、`msi`、`nsis`、`dmg`） | CLI 能識別，但 Varg 專案打包目前會回傳 unsupported capability |

## 測試與檢查

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

## 文件

- [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md)：Varg 創作語言方向與 MVP 子集說明。
- [`docs/ai-agent-unified-spec.md`](docs/ai-agent-unified-spec.md)：AI Agent 工作流方向。
- [`docs/quest-workflow-ui-reference.md`](docs/quest-workflow-ui-reference.md)：Quest 工作流 UI 參考。
- [`docs/adr/`](docs/adr/)：架構決策記錄。

## 授權

Mozilla Public License 2.0。詳見 [LICENSE](LICENSE)。
