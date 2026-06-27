# Varg

[![CI](https://github.com/viloris-org/Varg/actions/workflows/core.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/core.yml)
[![Nightly](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.96+-orange.svg)

[English](README.md) | 简体中文 | [繁體中文](README.zh-Hant.md) | [日本語](README.ja.md) | [한국어](README.ko.md) | [Español](README.es.md)

Varg 是一个实验性的游戏引擎和编辑器，围绕 Rust 运行时、Tauri/React 桌面编辑器以及 AI 辅助创作工作流构建。当前代码库重点在安全的 ECS/运行时基础、原生编辑器外壳、Varg 创作语言、项目打包，以及 Quest/Copilot 风格的编辑器自动化。

项目仍处于 pre-1.0 阶段。部分文档描述的是目标设计，而本 README 记录的是当前仓库中已经体现的内容。

![Varg 编辑器](docs/screenshots/editor.png)

## 快速上手

前提条件：

- [Rust](https://rustup.rs/) 1.96 或更新版本
- 用于编辑器前端的 [Bun](https://bun.sh/)
- [Tauri v2 系统依赖](https://v2.tauri.app/start/prerequisites/)

在 Debian/Ubuntu 类 Linux 发行版上，Tauri 依赖通常包括：

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

克隆并运行编辑器：

```sh
git clone https://github.com/viloris-org/Varg
cd Varg

cd editor
bun install
bun run dev:tauri
```

构建 Rust workspace：

```sh
cargo build --workspace
```

## 当前能力

- **Rust 运行时基础**：ECS、项目清单、资源、平台输入、渲染 trait、WGPU 集成、物理、音频、UI、动画、骨骼、shader、policy、AI 和打包 crate。
- **Tauri 编辑器**：React/TypeScript 桌面应用，由 Rust 命令支持 Hub/项目工作流、视口宿主、Copilot、Quest、打包、对话框和原生窗口/面板。
- **Varg 创作语言**：`.varg`、`.vscene`、`.vasset` 解析、诊断、MVP 脚本运行时、行为声明和 `varg-lsp` 二进制。
- **声明式脚本实验**：`engine-script-declarative` 下的 JSON 行为、场景、UI、系统、项目和资源结构。
- **打包管线**：`cargo xtask package` 可为桌面项目构建运行时文件夹，并校验若干未来 target/format 组合。
- **安全 Rust 策略**：引擎 crate 使用 `#![forbid(unsafe_code)]`。

## 项目结构

```text
Varg/
├── crates/                         # 引擎和运行时 crate
│   ├── engine-core/                # ID、错误、数学、配置
│   ├── engine-ecs/                 # 场景、实体、变换、组件
│   ├── engine-assets/              # 资源数据库、导入器、清单
│   ├── engine-render/              # 渲染 trait 和共享渲染模型
│   ├── engine-render-wgpu/         # WGPU 后端和视口实验
│   ├── engine-platform/            # 窗口、输入、文件系统抽象
│   ├── engine-script-varg/         # Varg 解析器、诊断、运行时、LSP
│   ├── engine-script-declarative/  # 声明式 JSON 创作实验
│   ├── engine-editor/              # 编辑器服务和 AI/工具支持
│   ├── engine-packager/            # 项目打包管线
│   └── runtime-min/                # 运行时组合根
├── editor/                         # Tauri/React 桌面编辑器
├── examples/                       # 示例项目、行为和脚本
├── docs/                           # 设计笔记、PRD 和 ADR
├── schema/                         # JSON schema
├── scripts/                        # 工具脚本和测试
└── xtask/                          # workspace 自动化命令
```

## 编辑器开发

```sh
cd editor
bun install

bun run dev:tauri
bun run build
bun run tauri build
```

常用路径：

- 渲染器 UI：`editor/src/renderer/`
- Tauri 命令和宿主服务：`editor/src-tauri/src/`
- Tauri 权限：`editor/src-tauri/capabilities/`

## 运行时 Feature

`runtime-min` 是组合 crate。Feature 集也列在根 `Cargo.toml` 的 workspace metadata 中。

| Feature | 用途 |
|---|---|
| `runtime-min` | 最小无头运行时路径 |
| `runtime-game` | 带资源导入和窗口支持的运行时路径 |
| `wgpu` | WGPU 渲染后端 |
| `physics` | 物理子系统 |
| `audio` | 音频子系统 |
| `editor` | 编辑器服务 |
| `agent-tools` | AI/编辑器工具支持 |
| `dev-full` | 较完整的开发构建，包含运行时、编辑器、Agent、物理、音频、shader、2D/UI、动画和骨骼功能 |

示例：

```sh
cargo build -p runtime-min --no-default-features --features runtime-min
cargo build -p runtime-min --no-default-features --features runtime-game,wgpu,physics,audio
cargo xtask runtime-min
cargo xtask build-editor
```

## Varg 语言工具

运行语言服务器：

```sh
cargo xtask varg-lsp
```

将 `.vscene` 编译为场景 JSON：

```sh
cargo xtask vscene compile path/to/input.vscene --out path/to/output.scene.json
```

目标语言方向见 [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md)。该文档包含一些超出 MVP 运行时的计划语法。

## 打包项目

为当前桌面宿主打包默认示例项目：

```sh
cargo xtask package --project examples/project/fps_arena --target native --format folder --debug
```

文件夹包会写入项目目录下，例如：

```text
examples/project/fps_arena/exports/<project>/<target>/<channel>/
```

其中包含运行时二进制、启动脚本、复制后的项目内容、资源清单和 `package-manifest.json`。

当前打包状态：

| Target | 当前支持 |
|---|---|
| `native`、`linux-x64`、`windows-x64`、`macos-universal` | 在匹配桌面宿主上生成 `folder` 包 |
| `android-arm64` | 已有工具链校验；尚未实现签名 APK/AAB 生成 |
| `ios-universal` | 已有工具链校验；尚未实现签名 IPA 生成 |
| 桌面安装包（`appimage`、`deb`、`rpm`、`exe`、`msi`、`nsis`、`dmg`） | CLI 能识别，但 Varg 项目打包目前会返回 unsupported capability |

## 测试与检查

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

## 文档

- [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md)：Varg 创作语言方向和 MVP 子集说明。
- [`docs/ai-agent-unified-spec.md`](docs/ai-agent-unified-spec.md)：AI Agent 工作流方向。
- [`docs/quest-workflow-ui-reference.md`](docs/quest-workflow-ui-reference.md)：Quest 工作流 UI 参考。
- [`docs/adr/`](docs/adr/)：架构决策记录。

## 许可证

Mozilla Public License 2.0。详见 [LICENSE](LICENSE)。
