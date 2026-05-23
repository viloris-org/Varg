# Repository Guidelines

## Project Structure & Module Organization

Aster is a Rust workspace. Core engine code lives under `crates/`, with each subsystem in its own crate: `engine-core`, `engine-ecs`, `engine-platform`, `engine-assets`, `engine-render`, `engine-render-vulkan`, `engine-physics`, `engine-audio`, `engine-editor`, `engine-editor-ui`, `engine-i18n`, `engine-cli`, and `runtime-min`. Repository automation is in `xtask/`. Integration tests currently live in crate-level `tests/` directories, for example `crates/runtime-min/tests/`. Example project assets and configs are in `examples/project/`, including scenes, prefabs, and `*.toml` runtime/editor configuration. Design and planning notes belong in `docs/`.

## Build, Test, and Development Commands

- `cargo test --workspace`: run all workspace tests.
- `cargo check --workspace --all-features`: type-check every crate with all feature gates enabled.
- `cargo test -p runtime-min --no-default-features --features runtime-min`: verify the minimal runtime path stays lean.
- `cargo run -p xtask -- test`: run the workspace test task through repository automation.
- `cargo run -p xtask -- check`: run the workspace check task.
- `cargo run -p xtask -- runtime-min`: build the minimal runtime profile.
- `cargo run -p xtask -- build-editor`: build the editor profile.
- `cargo run -p xtask -- package --profile editor --project examples/project`: build and package the example editor project into `target/aster-packages/`.

## Coding Style & Naming Conventions

Use Rust 2021 and keep code formatted with `cargo fmt --workspace`. Prefer explicit subsystem boundaries and workspace dependencies from the root `Cargo.toml`. Crate names use kebab case (`engine-editor-ui`); Rust modules, files, functions, and variables use snake case; public types and traits use `PascalCase`; constants use `SCREAMING_SNAKE_CASE`. Keep feature names aligned with workspace profiles such as `runtime-min`, `editor`, `agent-tools`, and `dev-full`.

## Testing Guidelines

Use Rust’s built-in test framework. Put crate integration tests in `crates/<crate>/tests/` and unit tests near the code they cover. Name tests after behavior, for example `loads_runtime_services` or `rejects_invalid_manifest`. When changing feature-gated code, run the targeted feature command as well as the full workspace tests.

Before finishing a task that changes Rust code, Cargo manifests, examples, or test-covered project configuration, run `cargo fmt --check` to verify formatting and `cargo test --workspace` to check for regressions. For docs-only, planning, or repository-instruction changes, skip the Cargo commands unless the change also touches buildable code.

## Commit & Pull Request Guidelines

Recent history uses short imperative commit subjects, such as `Add editor UI hub foundations` and `Improve project creation dialog`. Follow that style: one concise subject, capitalized, no trailing period. Pull requests should include a brief summary, the commands run, linked issues if applicable, and screenshots or recordings for editor UI changes.

## Security & Configuration Tips

Do not commit generated `target/` output or machine-local secrets. Keep example configuration generic under `examples/project/`. Heavy importers are feature-gated in `engine-assets`; avoid enabling them in minimal runtime changes unless the change explicitly requires it.
