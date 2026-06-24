# Repository Guidelines

## Project Structure & Module Organization

Varg is a Rust workspace with a Tauri/React editor. Core engine crates live under `crates/`, with one crate per subsystem such as `engine-ecs`, `engine-assets`, `engine-render`, `engine-render-wgpu`, `engine-ai`, and `runtime-min`. The desktop editor is in `editor/`: React/TypeScript UI under `editor/src/renderer/` and the Tauri Rust backend under `editor/src-tauri/`. Example projects, scenes, behaviors, and assets are in `examples/`. Automation lives in `xtask/` and `scripts/`; JSON schemas are in `schema/`.

## Build, Test, and Development Commands

- `cargo build --workspace`: build all Rust crates.
- `cargo test --workspace`: run the full Rust test suite.
- `cargo fmt --check`: verify Rust formatting, matching CI.
- `cargo clippy --workspace`: run Rust lint checks used by CI.
- `cd editor && bun install`: install editor frontend dependencies.
- `cd editor && bun run dev:tauri`: launch the hot-reload Tauri editor.
- `cd editor && bun run build`: build the Vite frontend for packaging.
- `cargo xtask package --project examples/project --target native --format folder --debug`: package the sample project.

## Coding Style & Naming Conventions

Use Rust 2024 edition conventions and standard `rustfmt`; keep modules and files in `snake_case`, crates in `kebab-case`, and public types in `PascalCase`. Existing crates use `#![forbid(unsafe_code)]`; do not add unsafe code. Prefer typed errors with `thiserror`, structured data with `serde`, and workspace dependencies from root `Cargo.toml`. For TypeScript/React, keep components in `PascalCase`, helpers in `camelCase`, and renderer code within `editor/src/renderer/`.

## Testing Guidelines

Place Rust integration tests in each crate's `tests/` directory and unit tests near the code they exercise. Use focused package commands while iterating, for example `cargo test -p engine-render-wgpu` or `cargo test -p runtime-min --no-default-features --features runtime-min`. Python utility tests live in `scripts/tests/`; run them with `pytest scripts/tests` when changing scripts.

## Commit & Pull Request Guidelines

Recent history follows Conventional Commits such as `feat(render-wgpu): ...`, `refactor(project): ...`, and `chore(lock): ...`. Use the same `type(scope): summary` format with concise, imperative summaries. Pull requests should describe the behavioral change, list validation commands run, link relevant issues, and include screenshots or recordings for editor UI changes.

## Security & Configuration Tips

Do not commit generated build output, local editor preferences, credentials, or platform-specific secrets. Keep Tauri capability changes in `editor/src-tauri/capabilities/` narrow and review any file-system or process permissions carefully.
