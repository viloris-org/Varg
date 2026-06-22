# Aster

[![CI](https://github.com/viloris-org/Aster/actions/workflows/core.yml/badge.svg)](https://github.com/viloris-org/Aster/actions/workflows/core.yml)
[![Nightly](https://github.com/viloris-org/Aster/actions/workflows/nightly.yml/badge.svg)](https://github.com/viloris-org/Aster/actions/workflows/nightly.yml)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.78+-orange.svg)

English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

Aster is an AI-native game engine. Describe your game in natural language, and a cluster
of autonomous agents builds it — scene, logic, UI, and all. A full visual editor is
there for you to tweak, polish, and take control whenever you want.

![Aster Editor](docs/screenshots/editor.png)

> **Screenshot placeholder** — replace `docs/screenshots/editor.png` with an actual
> editor screenshot once the UI stabilises.

## Getting Started

```sh
git clone https://github.com/viloris-org/Aster
cd Aster

# Launch the editor
cd editor
bun install
bun run dev:tauri
```

> **Prerequisites:** [Rust ≥ 1.78](https://rustup.rs/), [Bun ≥ 1.0](https://bun.sh/),
> [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/).
> Linux users: `sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev
> libayatana-appindicator3-dev librsvg2-dev`

## Features

- **AI-native at its core** — not just an assistant bolted on. A multi-agent cluster
  plans, builds, and reviews your game autonomously. Natural language in, playable
  scene out. Sandboxed review keeps your project safe.
- **Declarative game description** — six complete declarative systems (behavior
  trees, scene graphs, UI layouts, system configs, asset manifests, project
  structure) let agents generate structured JSON instead of code. LLM success rate
  jumps from ~50% to ~90% compared to raw scripting.
- **Visual scene editor** — place objects, tweak transforms, add components through a
  polished interface. Best of both worlds: let AI do the heavy lifting, then
  hand-tune every detail.
- **Live play mode** — hit Play, see physics and scripts run; hit Stop with zero
  cleanup. Your edit scene is never touched.
- **Asset pipeline** — drop glTF/PNG into the project panel. File watcher triggers
  import, hot reload pushes updates live.
- **Pluggable rendering** — swap backends without touching engine code. Ships with
  WGPU.
- **Headless runtime** — the same engine runs in servers, CI pipelines, or automated
  builds. No window required.
- **Zero unsafe code** — every crate uses `#![forbid(unsafe_code)]`. Safe by
  default.

## Project Structure

```
Aster/
├── editor/                  # Tauri desktop app (React + Rust)
├── crates/
│   ├── engine-editor/       # Editor workflow, services, agent tooling
│   ├── engine-ecs/          # Scene, entity, transform, world
│   ├── engine-assets/       # Database, importers, hot reload
│   ├── engine-render/       # Render graph, device trait
│   ├── engine-render-wgpu/  # WGPU backend
│   ├── engine-physics/      # Physics (rapier3d)
│   ├── engine-audio/        # Audio pipeline
│   ├── engine-core/         # IDs, errors, math, config
│   ├── engine-platform/     # Window, input, filesystem
│   ├── engine-script-rhai/  # Rhai scripting
│   ├── engine-animation/    # Animation system
│   ├── engine-ai/           # AI planner & system prompts
│   ├── engine-agent-cluster/# Agent orchestration
│   ├── runtime-min/         # Composition root
│   └── …                    # i18n, shader, policy, skeleton, etc.
├── xtask/                   # Build & automation tasks
├── examples/                # Sample project & scenes
└── docs/                    # Design notes
```

## Editing a Scene

1. Launch the editor → **Hub** screen
2. Create or open a project
3. **Hierarchy** panel lists every object in the scene
4. **Inspector** shows the selected object's transform and components
5. **Scene View** renders the 3D viewport — orbit, pan, zoom
6. Click **Play** to run physics and scripts in **Game View**
7. Add components (Camera, Light, MeshRenderer, Rigidbody, Collider, …) or write a
   Rhai script

## Build Profiles

Profiles select which subsystems are linked at compile time:

| Profile | What you get |
|---|---|
| `editor` | Editor services, wgpu viewports, and agent tools for the Tauri frontend |
| `runtime-min` | Headless — CI smoke tests, servers, automated builds |
| `runtime-game` | Headless + windowing |
| `dev-full` | Everything: editor, physics, audio, script, agent, render |

```sh
cargo build -p runtime-min --no-default-features --features editor
cargo build -p runtime-min --no-default-features --features runtime-min
```

## Packaging a Game Project

```sh
# Native runnable folder for the example project
cargo xtask package --project examples/project --target native --format folder --debug

# Release folder
cargo xtask package --project examples/project --target native --format folder --release
```

The package is written to `exports/<project>/<target>/<channel>/` and contains
the runtime binary, launcher script, project manifest, default scene, copied
assets, `asset-manifest.json`, and `package-manifest.json`.

Current support:

| Target | Host support | Formats |
|---|---|---|
| `linux-x64` | Linux | `folder` |
| `windows-x64` | Windows | `folder` |
| `macos-universal` | macOS | `folder` |
| `android-arm64` | Linux, Windows | `apk`, `aab` planned; validates Android SDK/NDK and Rust target |
| `ios-universal` | macOS | `ipa` planned; validates Xcode and Rust iOS targets |

Android and iOS targets are wired into the shared packaging pipeline and
toolchain validation, but signed mobile artifacts require the mobile runtime
adapter and platform project templates before package generation is enabled.

## Building the Editor

```sh
cd editor
bun install

# Development (hot-reload frontend + Rust backend)
bun run dev:tauri

# Distribution bundle
bun run tauri build
# → editor/src-tauri/target/release/bundle/
```

## Testing

```sh
# Full engine test suite
cargo test --workspace

# Headless runtime only (fast)
cargo test -p runtime-min --no-default-features --features runtime-min

# Editor services
cargo test -p engine-editor --no-default-features --features agent-tools

# WGPU backend
cargo test -p engine-render-wgpu
```

## License

Mozilla Public License 2.0. See [LICENSE](LICENSE).
