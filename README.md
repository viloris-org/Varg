# Aster

English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

Aster is an early-stage Rust game engine workspace focused on a small native
runtime, explicit subsystem boundaries, editor-ready data formats, and
feature-gated engine composition.

The project is not a production engine yet. It is a workspace for building and
testing the runtime, asset pipeline, rendering abstractions, editor shell, and
packaging flow that will make up the engine.

## Goals

- Keep the minimal runtime small and measurable.
- Make engine subsystems explicit, testable, and independently feature-gated.
- Use data formats that are suitable for editor workflows, automation, and
  future migration.
- Provide repository automation through `xtask` instead of ad-hoc scripts.
- Keep example project configuration generic and reproducible.

## Workspace

Core engine code lives in `crates/`:

- `engine-core`: shared IDs, handles, errors, logging, math, time, and runtime
  configuration.
- `engine-ecs`: scene, entity, transform, world, schema, physics, and audio
  primitives.
- `engine-platform`: platform boundaries for windows, input, filesystem,
  dynamic libraries, and callbacks.
- `engine-assets`: asset database, resource registry, manifests, dependency
  graph, import queues, hot reload tracking, and resource data formats.
- `engine-render`: renderer-facing abstractions, render graph, targets,
  resources, pipelines, and the headless render device.
- `engine-render-wgpu`: WGPU-backed rendering integration.
- `engine-render-vulkan`: Vulkan-facing rendering integration scaffolding.
- `engine-physics`: physics integration surface.
- `engine-audio`: audio integration surface.
- `engine-editor`: editor workflows, native editor services, render hooks,
  physics hooks, and agent tooling.
- `engine-editor-ui`: egui-based editor shell, panels, widgets, fonts, and UI
  state.
- `engine-i18n`: localization loading and bundled locale files.
- `engine-script-rhai`: Rhai scripting integration.
- `engine-cli` / package `aster`: editor-first launcher and command-line tool.
- `runtime-min`: minimal runtime profile and feature-composed runtime entry
  point.
- `xtask`: repository automation entry points.

Example project data lives in `examples/project/`, including:

- `aster.project.toml`: project manifest.
- `build.runtime-min.toml`: sample runtime build configuration.
- `editor.preferences.toml`: sample editor preferences.
- `assets/`: example material assets and metadata.
- `scenes/`: example scene data.
- `prefabs/`: example prefab data.

Design and planning notes belong in `docs/`.

## Build Profiles

Runtime composition is driven through Cargo features:

- `runtime-min`: minimal native runtime without editor, scripting, heavy
  importers, physics, audio, or concrete rendering.
- `runtime-game`: game runtime surface on top of the minimal profile.
- `wgpu`: WGPU rendering backend for runtime builds that need it.
- `physics`: optional physics support.
- `audio`: optional audio support.
- `editor`: editor-facing workflows and data.
- `agent-tools`: automation and agent integration surface.
- `script-python`: Python scripting backend integration surface.
- `dev-full`: full local development profile.

Heavy asset importers are feature-gated in `engine-assets` with
`fbx-importer`, `assimp-importer`, and `heavy-importers`, so disabling them
keeps their dependencies out of minimal runtime builds.

## Requirements

- Rust 1.78 or newer.
- Cargo with the Rust 2021 edition toolchain.
- Platform graphics dependencies required by `winit`, `egui`, `wgpu`, or
  Vulkan when building editor or rendering features.

## Development

Run the full workspace tests:

```sh
cargo test --workspace
```

Type-check every crate with all feature gates enabled:

```sh
cargo check --workspace --all-features
```

Check the minimal runtime feature path:

```sh
cargo test -p runtime-min --no-default-features --features runtime-min
```

Run the same common tasks through repository automation:

```sh
cargo run -p xtask -- test
cargo run -p xtask -- check
```

Build the minimal runtime profile:

```sh
cargo run -p xtask -- runtime-min
```

Build the editor profile:

```sh
cargo run -p xtask -- build-editor
```

Run the agent tooling smoke path:

```sh
cargo run -p xtask -- agent-smoke
```

## CLI

The `aster` package provides the editor-first launcher and command-line tool.

Show available CLI commands:

```sh
cargo run -p aster
```

Common commands:

```sh
cargo run -p aster -- profiles
cargo run -p aster -- smoke runtime-min
cargo run -p aster -- run examples/project
cargo run -p aster -- build examples/project
```

## Packaging

Package the example project with the editor profile:

```sh
cargo run -p xtask -- package --profile editor --project examples/project
```

Package output is written under:

```text
target/aster-packages/<platform>/<profile>/
```

Native packaging currently supports the `runtime-game` and `editor` profiles.
If no profile is passed, `xtask package` reads the example project's runtime
build configuration.

## Testing

Use Rust's built-in test framework. Put crate integration tests in
`crates/<crate>/tests/` and unit tests near the code they cover. When changing
feature-gated code, run the targeted feature command as well as the full
workspace tests.

Useful targeted checks:

```sh
cargo test -p runtime-min --no-default-features --features runtime-min
cargo test -p engine-editor --no-default-features --features agent-tools
cargo test -p engine-render-wgpu
```

## Repository Practices

- Format Rust code with `cargo fmt --workspace`.
- Prefer workspace dependencies from the root `Cargo.toml`.
- Keep crate names in kebab case and Rust modules, files, functions, and
  variables in snake case.
- Do not commit generated `target/` output or machine-local secrets.
- Keep example configuration generic under `examples/project/`.
- Avoid enabling heavy importers in minimal runtime changes unless the change
  explicitly requires them.

## License

Aster is licensed under the Mozilla Public License 2.0. See `LICENSE`.
