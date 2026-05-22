# Aster

[English](README.md) | 简体中文 | [日本語](README.ja.md)

Aster 是一个早期阶段的 Rust 游戏引擎工作区，重点是小型原生运行时、清晰的子系统边界、适合编辑器的数据格式，以及通过功能开关组合引擎能力。

项目目前还不是生产级引擎。它用于构建和验证运行时、资产管线、渲染抽象、编辑器外壳和打包流程，这些模块会逐步组成完整引擎。

## 目标

- 保持最小运行时足够小，并且可度量。
- 让引擎子系统边界明确、可测试，并可独立通过 feature 控制。
- 使用适合编辑器流程、自动化和未来迁移的数据格式。
- 通过 `xtask` 提供仓库自动化，减少临时脚本。
- 保持示例项目配置通用且可复现。

## 工作区

核心引擎代码位于 `crates/`：

- `engine-core`：共享 ID、句柄、错误、日志、数学、时间和运行时配置。
- `engine-ecs`：场景、实体、变换、世界、schema、物理和音频基础结构。
- `engine-platform`：窗口、输入、文件系统、动态库和回调的平台边界。
- `engine-assets`：资产数据库、资源注册表、manifest、依赖图、导入队列、热重载跟踪和资源数据格式。
- `engine-render`：面向渲染器的抽象、渲染图、目标、资源、管线和无头渲染设备。
- `engine-render-wgpu`：基于 WGPU 的渲染集成。
- `engine-render-vulkan`：面向 Vulkan 的渲染集成脚手架。
- `engine-physics`：物理集成接口。
- `engine-audio`：音频集成接口。
- `engine-editor`：编辑器工作流、原生编辑器服务、渲染钩子、物理钩子和 agent 工具。
- `engine-editor-ui`：基于 egui 的编辑器外壳、面板、控件、字体和 UI 状态。
- `engine-i18n`：本地化加载和内置语言文件。
- `engine-script-rhai`：Rhai 脚本集成。
- `engine-cli` / package `aster`：以编辑器为中心的启动器和命令行工具。
- `runtime-min`：最小运行时 profile 和通过 feature 组合的运行时入口。
- `xtask`：仓库自动化入口。

示例项目数据位于 `examples/project/`，包括：

- `aster.project.toml`：项目 manifest。
- `build.runtime-min.toml`：示例运行时构建配置。
- `editor.preferences.toml`：示例编辑器偏好设置。
- `assets/`：示例材质资产和元数据。
- `scenes/`：示例场景数据。
- `prefabs/`：示例 prefab 数据。

设计和规划笔记放在 `docs/`。

## 构建 Profile

运行时组合由 Cargo features 驱动：

- `runtime-min`：最小原生运行时，不包含编辑器、脚本、重型导入器、物理、音频或具体渲染后端。
- `runtime-game`：基于最小 profile 的游戏运行时表面。
- `wgpu`：用于需要 WGPU 的运行时构建的渲染后端。
- `physics`：可选物理支持。
- `audio`：可选音频支持。
- `editor`：编辑器相关工作流和数据。
- `agent-tools`：自动化和 agent 集成接口。
- `script-python`：Python 脚本后端集成接口。
- `dev-full`：完整本地开发 profile。

`engine-assets` 中的重型资产导入器通过 `fbx-importer`、`assimp-importer` 和 `heavy-importers` 控制。关闭它们可以让最小运行时构建不引入相关依赖。

## 环境要求

- Rust 1.78 或更新版本。
- 支持 Rust 2021 edition 的 Cargo 工具链。
- 构建编辑器或渲染功能时，需要平台上 `winit`、`egui`、`wgpu` 或 Vulkan 所需的图形依赖。

## 开发

运行完整工作区测试：

```sh
cargo test --workspace
```

使用所有 feature 对每个 crate 做类型检查：

```sh
cargo check --workspace --all-features
```

检查最小运行时 feature 路径：

```sh
cargo test -p runtime-min --no-default-features --features runtime-min
```

通过仓库自动化运行常用任务：

```sh
cargo run -p xtask -- test
cargo run -p xtask -- check
```

构建最小运行时 profile：

```sh
cargo run -p xtask -- runtime-min
```

构建编辑器 profile：

```sh
cargo run -p xtask -- build-editor
```

运行 agent 工具 smoke 路径：

```sh
cargo run -p xtask -- agent-smoke
```

## CLI

`aster` package 提供以编辑器为中心的启动器和命令行工具。

显示可用 CLI 命令：

```sh
cargo run -p aster
```

常用命令：

```sh
cargo run -p aster -- profiles
cargo run -p aster -- smoke runtime-min
cargo run -p aster -- run examples/project
cargo run -p aster -- build examples/project
```

## 打包

使用编辑器 profile 打包示例项目：

```sh
cargo run -p xtask -- package --profile editor --project examples/project
```

打包输出位于：

```text
target/aster-packages/<platform>/<profile>/
```

原生打包目前支持 `runtime-game` 和 `editor` profile。如果没有传入 profile，`xtask package` 会读取示例项目的运行时构建配置。

## 测试

使用 Rust 内置测试框架。crate 级集成测试放在 `crates/<crate>/tests/`，单元测试放在被测代码附近。修改 feature-gated 代码时，除了完整工作区测试，也要运行对应 feature 的目标命令。

常用定向检查：

```sh
cargo test -p runtime-min --no-default-features --features runtime-min
cargo test -p engine-editor --no-default-features --features agent-tools
cargo test -p engine-render-wgpu
```

## 仓库实践

- 使用 `cargo fmt --workspace` 格式化 Rust 代码。
- 优先使用根 `Cargo.toml` 中的 workspace 依赖。
- crate 名使用 kebab case，Rust 模块、文件、函数和变量使用 snake case。
- 不要提交生成的 `target/` 输出或本机私密配置。
- `examples/project/` 下的示例配置应保持通用。
- 除非变更明确需要，否则不要在最小运行时相关变更中启用重型导入器。

## 许可证

Aster 使用 Mozilla Public License 2.0 授权。详情见 `LICENSE`。
