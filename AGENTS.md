# Repository Guidelines

## Project Structure & Module Organization

Aster is a Rust 2021 workspace. Engine subsystems live in `crates/`, generally one responsibility per crate, such as `engine-ecs`, `engine-assets`, `engine-render-*`, `engine-physics`, and `engine-ai`. `crates/runtime-min` is the headless composition root. The desktop editor is in `editor/`: React/TypeScript UI code is under `editor/src/renderer`, while the Tauri Rust backend is under `editor/src-tauri`. Build automation lives in `xtask/` and `scripts/`. Put design notes in `docs/`, schemas in `schema/`, and sample scenes, assets, and behavior files in `examples/`.

## Build, Test, and Development Commands

- `cargo fmt --check`: verify Rust formatting used by CI.
- `cargo clippy --workspace`: lint all workspace crates.
- `cargo test --workspace`: run the complete Rust test suite.
- `cargo xtask check`: check the workspace with all features.
- `cargo xtask build-editor`: build the editor-enabled runtime profile.
- `cargo xtask agent-smoke`: test editor agent tooling with its feature gate.
- `cd editor && bun install`: install frontend and Tauri CLI dependencies.
- `cd editor && bun run build`: type-check and build the Vite frontend.
- `cd editor && bun run tauri dev`: launch the desktop editor in development mode.

Rust 1.78+, Bun 1.0+, and the platform-specific Tauri prerequisites are required.

## Coding Style & Naming Conventions

Format Rust with `cargo fmt`; use four-space indentation and keep crates free of unsafe code. Rust modules, functions, and tests use `snake_case`; types and traits use `PascalCase`; constants use `SCREAMING_SNAKE_CASE`; crate and feature names use kebab-case. TypeScript is strict, uses two-space indentation, single quotes, semicolons, `PascalCase` React components, and `camelCase` functions. Prefer existing workspace dependencies and subsystem boundaries over new cross-crate coupling.

## Testing Guidelines

Use Rust's built-in test framework. Keep unit tests beside implementation code and integration tests in `crates/<crate>/tests/` or `editor/src-tauri/tests/`. Name tests by observable behavior, for example `scene_with_all_components_round_trip`. Run targeted crate tests during development, then `cargo test --workspace` before submission. Rendering tests may skip when no display or GPU surface is available.

## Commit & Pull Request Guidelines

Recent commits primarily use Conventional Commits: `feat(editor): ...`, `fix(i18n): ...`, and `refactor(scope): ...`. Keep subjects concise, imperative, and scoped. Pull requests should explain behavior changes, list verification commands, link relevant issues, and include screenshots or recordings for editor UI changes. Do not commit `target/`, `editor/dist/`, secrets, or machine-local configuration.
