# Varg Engine Context

Varg is a game engine whose runtime, editor, tools, and optional extensions share scene extraction, rendering, asset, scripting, and platform-independent engine policies.

## Language

**Runtime Layer**:
The crates and binaries that must be available to run a packaged game or simulation. Runtime code should not depend on editor UI, developer tools, or local authoring state.
_Avoid_: Everything under `crates/`, game loop

**Editor Layer**:
The Rust and TypeScript surfaces used to inspect, author, validate, and package projects. Editor code may depend on runtime contracts, but runtime crates should not depend on editor presentation details.
_Avoid_: Tauri app, debug UI

**Tool Program**:
A standalone command or worker used for build, import, shader, packaging, inspection, or diagnostics work. Tool programs should be scriptable and should not require the desktop editor to run.
_Avoid_: xtask subcommand pile, helper script

**Varg Plugin**:
A package of runtime modules, editor panels, importers, scripts, assets, or tools described by a manifest. Plugins are optional capabilities with declared dependencies and loading scope.
_Avoid_: random crate, feature flag only

**Project Config Stack**:
The layered configuration model that combines engine defaults, platform defaults, project settings, build profile overrides, and local editor preferences.
_Avoid_: config file, preferences

**Render World**:
The immutable, per-frame rendering input extracted from the active scene. One Render World contains zero or one camera and many render objects, sprites, lights, and particles.
_Avoid_: Render queue, scene snapshot

**Frame Pipeline**:
The compiled sequence of rendering passes, resource accesses, scaling stages, and presentation work used to produce one frame from a Render World.
_Avoid_: Render loop, hard-coded pass chain

**Visibility Set**:
The subset of a Render World selected for a particular view after frustum culling and level-of-detail selection.
_Avoid_: Visible list, culled scene

**Render Scaling**:
The policy and frame data that separate internal rendering resolution from output and UI composition resolution.
_Avoid_: Resolution hack, resize path

**Asset Registry**:
The project-wide index of assets, source paths, stable asset identifiers, dependencies, import settings, and cooked outputs.
_Avoid_: asset folder, file list

**Cooked Asset**:
An imported, validated, target-ready artifact produced from source assets and included in a runtime package.
_Avoid_: converted file, cache blob

**Runtime Profile**:
A named feature and packaging target, such as `runtime-min`, `runtime-game`, `editor`, `agent-tools`, or `dev-full`, that selects which engine capabilities are included.
_Avoid_: cargo feature set, build mode

## Example dialogue

> Developer: Does the Frame Pipeline consume the entire Render World?
>
> Graphics programmer: It first derives a Visibility Set for the active camera, then executes shadow, forward, scaling, post-processing, and UI passes according to the compiled Frame Pipeline.

> Tools programmer: Should this import path run inside the editor?
>
> Engine programmer: It should be a Tool Program first, backed by the Asset Registry, and the editor should call that same contract instead of owning a separate importer.
