# Varg

[![CI](https://github.com/viloris-org/Varg/actions/workflows/core.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/core.yml)
[![Nightly](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.96+-orange.svg)

English | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-Hant.md) | [日本語](README.ja.md) | [한국어](README.ko.md) | [Español](README.es.md)

Varg is an experimental game engine and editor built around a Rust runtime, a
Tauri/React desktop editor, and AI-assisted authoring workflows. The current codebase
is focused on a safe ECS/runtime foundation, a native editor shell, the Varg authoring
language, project packaging, and Quest/Copilot style editor automation.

The project is still pre-1.0. Some docs describe target designs, while this README
tracks what is represented in the current repository.

![Varg Editor](docs/screenshots/editor.png)

## Getting Started

Prerequisites:

- [Rust](https://rustup.rs/) 1.96 or newer
- [Bun](https://bun.sh/) for the editor frontend
- [Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/)

On Debian/Ubuntu-like Linux distributions, the Tauri dependencies usually include:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

Clone and run the editor:

```sh
git clone https://github.com/viloris-org/Varg
cd Varg

cd editor
bun install
bun run dev:tauri
```

Build the Rust workspace:

```sh
cargo build --workspace
```

## Current Capabilities

- **Rust runtime foundation**: ECS, project manifests, assets, platform input,
  rendering traits, WGPU integration, physics, audio, UI, animation, skeleton,
  shader, policy, AI, and packaging crates.
- **Tauri editor**: a React/TypeScript desktop app backed by Rust commands for
  hub/project workflows, viewport hosting, Copilot, Quest, packaging, dialogs,
  and native windows/panels.
- **Varg authoring language**: `.varg`, `.vscene`, and `.vasset` parsing,
  diagnostics, an MVP script runtime, behavior declarations, and a `varg-lsp`
  binary.
- **Declarative scripting experiments**: JSON behavior, scene, UI, system,
  project, and asset structures under `engine-script-declarative`.
- **Packaging pipeline**: `cargo xtask package` builds a runtime folder for
  desktop projects and validates several future target/format combinations.
- **Safe Rust policy**: engine crates use `#![forbid(unsafe_code)]`.

## Project Structure

```text
Varg/
├── crates/
│   ├── engine-core/               # IDs, errors, math, config
│   ├── engine-ecs/                # Scenes, entities, transforms, components
│   ├── engine-assets/             # Asset database, importers, manifests
│   ├── engine-render/             # Renderer traits and shared render model
│   ├── engine-render-wgpu/        # WGPU backend and viewport experiments
│   ├── engine-platform/           # Window/input/filesystem abstractions
│   ├── engine-physics/            # Physics integration
│   ├── engine-audio/              # Audio integration
│   ├── engine-script-varg/        # Varg parser, diagnostics, runtime, LSP
│   ├── engine-script-declarative/ # Declarative JSON authoring experiments
│   ├── engine-editor/             # Editor services and AI/tooling support
│   ├── engine-agent-cluster/      # Agent orchestration primitives
│   ├── engine-ai/                 # AI planner and prompt support
│   ├── engine-quest/              # Quest validation/review primitives
│   ├── engine-packager/           # Project package pipeline
│   ├── runtime-min/               # Runtime composition root
│   └── ...                        # i18n, shader, render-2d, UI, animation, etc.
├── editor/
│   ├── src/renderer/              # React/TypeScript renderer
│   └── src-tauri/                 # Tauri Rust backend
├── examples/
│   ├── project/                   # Example Varg projects
│   ├── behaviors/                 # Declarative behavior JSON examples
│   └── scripts/                   # `.varg` script examples
├── docs/                          # Design notes, PRDs, and ADRs
├── schema/                        # JSON schemas
├── scripts/                       # Utility scripts and tests
└── xtask/                         # Workspace automation commands
```

## Editor Development

```sh
cd editor
bun install

# Vite frontend + Tauri backend
bun run dev:tauri

# Frontend production build
bun run build

# Tauri bundle
bun run tauri build
```

Useful editor paths:

- Renderer UI: `editor/src/renderer/`
- Tauri commands and host services: `editor/src-tauri/src/`
- Tauri permissions: `editor/src-tauri/capabilities/`

## Runtime Profiles

`runtime-min` is the composition crate. Feature sets are also listed in the root
`Cargo.toml` workspace metadata.

| Feature | Purpose |
|---|---|
| `runtime-min` | Minimal headless runtime path |
| `runtime-game` | Runtime path with asset import and windowing support |
| `wgpu` | WGPU renderer backend |
| `physics` | Physics subsystem |
| `audio` | Audio subsystem |
| `editor` | Editor-facing services |
| `agent-tools` | AI/editor tooling support |
| `dev-full` | Broad development build with runtime, editor, agent, physics, audio, shader, 2D/UI, animation, and skeleton features |

Examples:

```sh
cargo build -p runtime-min --no-default-features --features runtime-min
cargo build -p runtime-min --no-default-features --features runtime-game,wgpu,physics,audio
cargo xtask runtime-min
cargo xtask build-editor
```

## Varg Language Tools

Run the language server:

```sh
cargo xtask varg-lsp
```

Compile a `.vscene` file to scene JSON:

```sh
cargo xtask vscene compile path/to/input.vscene --out path/to/output.scene.json
```

The target language direction is documented in
[`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md). That
document intentionally includes planned syntax beyond the MVP runtime.

## Packaging a Project

Package the default example project for the current desktop host:

```sh
cargo xtask package --project examples/project/fps_arena --target native --format folder --debug
```

The folder package is written under the project, for example:

```text
examples/project/fps_arena/exports/<project>/<target>/<channel>/
```

It contains a runtime binary, launcher script, copied project payload, asset
manifest, and `package-manifest.json`.

Current packaging status:

| Target | Current support |
|---|---|
| `native`, `linux-x64`, `windows-x64`, `macos-universal` | `folder` packages on matching desktop hosts |
| `android-arm64` | Toolchain validation exists; signed APK/AAB generation is not yet implemented |
| `ios-universal` | Toolchain validation exists; signed IPA generation is not yet implemented |
| Desktop installers (`appimage`, `deb`, `rpm`, `exe`, `msi`, `nsis`, `dmg`) | Recognized by the CLI, but Varg project package generation currently returns unsupported capability errors |

## Testing and Checks

```sh
# Full Rust test suite
cargo test --workspace

# Workspace check with all features
cargo xtask check

# Formatting and linting
cargo fmt --check
cargo clippy --workspace

# Focused examples
cargo test -p runtime-min --no-default-features --features runtime-min
cargo test -p engine-render-wgpu
cargo test -p engine-editor --no-default-features --features agent-tools

# Python utility tests, when changing scripts/
pytest scripts/tests
```

## Documentation

- [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md): Varg
  authoring language direction and MVP subset notes.
- [`docs/ai-agent-unified-spec.md`](docs/ai-agent-unified-spec.md): AI agent
  workflow direction.
- [`docs/quest-workflow-ui-reference.md`](docs/quest-workflow-ui-reference.md):
  Quest workflow UI reference.
- [`docs/adr/`](docs/adr/): Architecture decision records.

## License

Mozilla Public License 2.0. See [LICENSE](LICENSE).
