# Project Decisions And Scope

Status: accepted P0 baseline.

## Independence Statement

Aster is an independent Rust-native engine project. Infernux is a reference only for capability boundaries, workflow lessons, and risk discovery. Aster must not inherit Infernux code, module structure, Python production layer, resource formats, naming, or release flow, and it is not a compatibility fork.

## Naming And Versioning

| Decision | Value |
|:---|:---|
| Project name | Aster |
| Repository | `https://github.com/rownix101/Aster` |
| Workspace package version | `0.1.0` |
| Crate prefix | `engine-*` for engine crates, profile crates named by product shape such as `runtime-min` |
| Version strategy | Single workspace version until public API pressure requires independent crate versioning |
| Release strategy | Source-first Rust workspace; package native binaries later through task 07 |

## Target Users And First Product

The first users are engine developers and technical game developers who need a small, auditable Rust-native runtime foundation before editor, importers, scripting, physics, audio, and concrete renderer work begins.

The first usable product shape is `runtime-min`: a native Rust library/profile that builds and ticks one frame with explicit runtime services, scene storage, stable handles, platform abstractions, asset IDs/paths, and a render abstraction backed by a headless renderer.

## Infernux Reference Matrix

| Area | Reference From Infernux | Aster Decision |
|:---|:---|:---|
| Capability boundary | Use as a reminder of engine/editor/tooling categories | Redesign crate boundaries around Rust workspace profiles |
| Workflow lessons | Keep task split, acceptance gates, and automation discipline | Use `xtask`, Cargo features, and CI as primary workflow |
| Python layer | Reference only as deferred scripting demand | No Python in first release; `script-python` is P2 optional |
| Resource formats | Do not inherit | Define native asset manifest and project formats in later tasks |
| Importers | Treat as optional compatibility utility | Infernux importer is P2/P3, not core |
| Release flow | Reference only for risk | Native Cargo/package flow is redesigned in task 07 |

## Reference Project Research

| Project | Useful Lesson | Boundary For Aster |
|:---|:---|:---|
| [Bevy](https://bevy.org/) | Rust-native ECS-first engines benefit from strong feature gating and data-driven APIs | Aster is lower-level initially and does not adopt Bevy module structure |
| [Fyrox](https://github.com/FyroxEngine/Fyrox) | A Rust engine can ship editor-first workflows and broad runtime features | Aster defers editor breadth until the atomic runtime is stable |
| [Godot](https://docs.godotengine.org/en/stable/about/list_of_features.html) | Mature engines need explicit platform, asset, editor, and scripting boundaries | Aster avoids compatibility goals and starts with narrower native scope |
| [wgpu](https://wgpu.rs/) | Portable graphics APIs reduce backend complexity | Aster keeps wgpu as a future comparison, but first backend target is lower-level Vulkan via `ash` |
| [ash](https://github.com/ash-rs/ash) | Raw Vulkan bindings fit an engine-owned render abstraction | Concrete Vulkan backend is outside `runtime-min` |

## Platform Matrix

| Platform | First Release Status | Notes |
|:---|:---|:---|
| Windows x64 | Supported | CI and native runtime required |
| macOS Apple Silicon | Supported | First graphics path uses Vulkan portability through MoltenVK |
| Linux x64 | Supported | CI and native runtime required |
| macOS Intel | Best effort | No release blocker unless regressions are caused by shared code |

## Backend Decisions

| Area | Decision |
|:---|:---|
| First graphics backend | Vulkan via `ash` |
| macOS graphics strategy | [MoltenVK](https://github.com/KhronosGroup/MoltenVK) for first usable release; native Metal evaluated later |
| Render abstraction | Backend-neutral `engine-render`; no concrete backend in `runtime-min` |
| Physics | Benchmark [Rapier](https://rapier.rs/) and [Jolt](https://github.com/jrouwe/JoltPhysics) before final selection |
| Editor UI | Start with egui evaluation because it is Rust-native and portable; keep Dear ImGui/imgui-rs as fallback for engine-tooling ergonomics |
| Scripting | Rust-only first release; Python/PyO3 is optional P2 under `script-python` |
| Agent tools | Design security model in P0; read-only tools in P1; write tools, transactions, and custom tools in P2 |

## Agent Policy

AI and agent capabilities are read-only by default. Write access requires explicit project configuration, permission gates, audit logging, and transaction records.

Agent execution defaults to sandboxed read-only mode. File edits use isolated worktrees unless direct writes are explicitly enabled. External commands require allowlisted patterns, sandbox limits, and audit logging.

## Initial Profiles

| Profile | Purpose |
|:---|:---|
| `runtime-min` | Core, ECS, platform abstraction, base assets, render abstraction only |
| `runtime-game` | Game runtime profile that can add concrete render/audio/physics later |
| `editor` | Native editor profile |
| `agent-tools` | Agent bridge, sandbox, worktree, transaction, trace, and tool metadata |
| `script-python` | Optional Python scripting and bindings |
| `dev-full` | Developer convenience profile that combines non-minimal profiles |

## Initial Targets

| Target | P0 Baseline |
|:---|:---|
| Build | `cargo test --workspace` passes on Windows, macOS, and Linux CI |
| Minimal runtime | `cargo build -p runtime-min --no-default-features --features runtime-min` |
| Startup | `runtime-min` should initialize explicit services and tick a frame without heap-heavy subsystems |
| Package size | `runtime-min` must stay free of editor, Python, physics, audio, importers, and concrete render backend dependencies |
| Dependency policy | Prefer small Rust-native crates with permissive licenses; isolate FFI in backend or `*-sys` crates |

## Dependency And License Inventory

| Dependency | Current Use | License Policy |
|:---|:---|:---|
| `thiserror` | Structured runtime errors | Permissive license required |
| `tracing` | Logging facade | Permissive license required |
| `bitflags` | Platform/input flags | Permissive license required |
| `ash` | Future Vulkan backend candidate | Backend crate only; no `runtime-min` dependency |
| `MoltenVK` | Future macOS Vulkan portability layer | Packaged only with graphics backend/profile |
| `Rapier` | Physics benchmark candidate | Physics crate only |
| `Jolt` | Physics benchmark candidate through FFI | Isolate C++/FFI boundary |
| `egui` | Editor UI candidate | Editor profile only |
| `imgui-rs` | Editor UI fallback candidate | Editor profile only |

Aster uses `MIT OR Apache-2.0` for workspace crates. New dependencies must be compatible with that distribution model, must not enter `runtime-min` unless required by the atomic core, and must be feature-gated by profile when optional.
