# Architecture lessons from Unreal Engine

## Status

Draft. This document translates useful Unreal Engine architecture patterns into Varg-sized changes. It is not a plan to clone Unreal's object model, build system, reflection macros, or editor implementation.

## Reference boundary

Unreal is useful to Varg because it makes large engine boundaries explicit:

- `Runtime` modules are separated from `Editor` modules.
- `Programs` contain standalone tools and workers.
- `Plugins` package optional engine, editor, importer, and gameplay capabilities behind descriptors.
- `Platforms` and `Config` describe target-specific policy outside gameplay code.
- `Shaders`, asset cooking, and derived data are treated as first-class build inputs.
- Templates and samples are productized as project starting points, not incidental examples.

Varg should borrow those boundaries while keeping Rust, Cargo features, TOML manifests, schemas, and small standalone tools as the implementation style.

## Varg layer map

The current workspace is intentionally smaller than Unreal, but it still needs stable layer names so humans, tools, and AI agents can reason about dependency direction.

| Layer | Crates and directories | Rule |
| --- | --- | --- |
| Runtime | `engine-core`, `engine-ecs`, `engine-platform`, `engine-assets`, `engine-render`, `engine-physics`, `engine-audio`, `engine-script-*`, `engine-render-2d`, `engine-ui`, `engine-animation`, `engine-skeleton`, `runtime-min` | Must be usable by packaged games without editor UI dependencies. |
| Runtime backend | `engine-render-wgpu`, `engine-shader` | Implements runtime contracts for a concrete graphics/shader backend. |
| Editor | `engine-editor`, `editor/` | May depend on runtime contracts, but runtime crates should not depend on editor presentation. |
| Tools | `xtask`, `engine-packager`, future standalone binaries | Performs import, packaging, shader, inspection, validation, and diagnostics work without launching the desktop editor. |
| Extensions | `engine-ai`, `engine-agent-cluster`, `engine-policy`, `engine-quest`, `.varg/skills` | Optional capabilities that should move toward explicit plugin descriptors. |
| Project assets | `examples/`, future project directories | User-authored scenes, assets, scripts, project config, preferences, and build profiles. |

The root `Cargo.toml` mirrors this map in `workspace.metadata.varg.layers` so future tooling can read the intended classification without scraping file names.

## Changes worth making

### 1. Introduce plugin descriptors

Unreal's `.uplugin` files work because optional capabilities have a manifest. Varg should introduce a smaller `VargPlugin.toml` descriptor before extension behavior spreads across Cargo features, editor hard-coding, and ad hoc directories.

Initial descriptor fields:

```toml
[plugin]
id = "varg.quest"
name = "Quest Tools"
version = "0.1.0"
scope = ["runtime", "editor"]

[[module]]
crate = "engine-quest"
kind = "runtime"

[[editor_panel]]
id = "quest"
entry = "QuestPage"

[[asset_importer]]
extension = "vquest"
schema = "schema/varg-quest-schema.json"
```

This descriptor should start as documentation plus schema validation. Dynamic loading can wait.

### 2. Make tools first-class programs

Varg should avoid putting every workflow behind the desktop editor. The first standalone tool contracts should be:

- `varg-pack`: package a project using the same path as `engine-packager`.
- `varg-asset`: validate, import, and inspect asset registry entries.
- `varg-shader`: validate shaders and emit backend metadata.
- `varg-project`: inspect config stacks and runtime profiles.

These can initially be `xtask` subcommands, but their APIs should be designed as if they will become separate binaries.

### 3. Formalize the project config stack

Varg already has project config, build profile config, and editor preferences in examples. The missing piece is a documented merge order:

1. Engine defaults.
2. Platform defaults.
3. Project `Varg.toml`.
4. Build profile overrides such as `build.runtime-min.toml`.
5. User-local editor preferences such as `editor.preferences.toml`.
6. Environment and CLI overrides.

Only the final two layers should contain machine-local choices. Packaged runtime output should be reproducible from the earlier layers plus explicit build inputs.

### 4. Build an asset registry and cooked asset boundary

The asset system should grow around stable asset identity instead of direct file paths. A minimal asset registry should track:

- stable asset id;
- source path;
- asset kind;
- importer id and settings;
- dependencies;
- content hash;
- cooked output path per target profile.

The editor, packager, hot reload, and AI tooling should all talk through this registry rather than each scanning files independently.

### 5. Split editor features by descriptor

The React editor currently has feature pages under `editor/src/renderer/pages/`. As the editor grows, panels should register through a feature descriptor model:

- id;
- title;
- command palette entries;
- project asset kinds it handles;
- required runtime/editor capabilities;
- optional plugin origin.

This keeps Quest, AI, Script, Viewport, Project, and future asset editors from becoming one implicit app shell.

### 6. Treat shaders as build inputs

`engine-shader` and `engine-render-wgpu` should define a shader module registry, validation command, cache metadata, and backend feature flags. Runtime rendering should consume validated shader metadata rather than relying only on backend-local loading behavior.

## Changes to reject

Do not import these Unreal patterns into Varg:

- UObject inheritance as the gameplay model.
- C++ macro-style reflection as a public authoring model.
- Blueprint graph storage as the primary script format.
- A large mandatory editor module graph before plugin descriptors exist.
- A deep `.ini` hierarchy when typed TOML and schemas are enough.
- A monolithic build graph that bypasses Cargo instead of complementing it.

## Implementation order

1. Keep the layer map current in `workspace.metadata.varg.layers`.
2. Add JSON Schema for `VargPlugin.toml` and validate one internal plugin descriptor.
3. Define the project config stack and write a merge/inspection test.
4. Add an asset registry manifest type in `engine-assets`.
5. Route packaging through the asset registry.
6. Move editor panel registration toward descriptors.
7. Add shader validation and cache metadata as a tool contract.
