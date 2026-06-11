# Aster Engine — Comprehensive Audit Report

**Date**: 2026-06-11  
**Scope**: All 22 crates, editor frontend, examples  
**Goal**: Assess readiness for building a large 3D game with AI-native workflows

---

## Executive Summary

Aster has a solid architectural foundation with ~25 well-organized crates, clean type systems, and good test discipline (zero `unsafe` in 19+ crates, `#![deny(missing_docs)]` enforced). The single-agent AI pipeline (`engine-ai`) is production-ready for basic game construction. However, the gap between architecture promises and implementation reality is large: the multi-agent cluster is structurally designed but functionally unimplemented, the declarative scripting backend has 11/18 stub action/condition variants, the ECS lacks query/iteration primitives, and critical math types (Mat4, Vec2, geometry) are missing. **The engine can build simple scenes via single-agent AI, but cannot yet support large 3D game development.**

---

## 1. Critical Gaps (Blocking)

### 1.1 Missing Math Types
| Type | Impact | Location |
|------|--------|----------|
| `Mat4` (4×4 matrix) | No view/projection/MVP matrices — rendering cannot work properly without inline math | `engine-core/src/math.rs` |
| `Vec2` | Used everywhere (Sprite2D, TileMap2D, UVs) but stored as `[f32; 2]` | `engine-core/src/math.rs` |
| `Vec4` | Colors stored as `[f32; 4]` everywhere | `engine-core/src/math.rs` |
| Geometry types (Ray, Plane, Frustum, AABB) | No picking, no culling, no spatial queries | `engine-core/` |

### 1.2 Multi-Agent Cluster Unimplemented
The `engine-agent-cluster` crate has clean architecture but zero execution capability:
- **`ModelWorker` does NOT implement the `Worker` trait** — Workers cannot run
- **`DefaultManager::decompose()`** uses keyword matching, not AI
- **`DeepReviewer`** never calls its AI model — all review logic is deterministic state checks
- **No orchestration loop** exists to connect Manager → Workers → Reviewer → Repair
- **Transaction bundles** are fully typed but no code populates or applies them
- **Grant hashes** are computed but never enforced (no tool-layer integration)
- **Context hashes** hardcoded to `"pending"` in `DefaultManager::build_context_packet()`
- The crate has **zero consumers** in the workspace (not in `runtime-min` dependency tree)

### 1.3 ECS Lacks Query System
The ECS uses `HashMap<Entity, Vec<Box<dyn Component>>>` — the simplest possible storage:
- **No query/iteration**: Cannot iterate all entities with a specific component set
- **No parallel execution**: `run_lifecycle` is single-threaded
- **No component removal**: `remove<C>()` for a single component type doesn't exist
- **No change detection**: No `Changed<T>`, `Added<T>` filters
- **No event system**: No cross-system communication
- **No system scheduling**: No `System` abstraction, no execution order

### 1.4 Scripting: Declarative Backend Largely Stubbed
11 of 16 `ActionExpr` variants and 3 of 10 `ConditionExpr` variants are TODO stubs:
- `Chase`, `Flee`, `Patrol`, `Attack`, `PlaySound`, `Spawn`, `Wait`, `ApplyImpulse` — all stubs
- `PlayerDistance`, `Health`, `HasTag` — all stubs
- `EntityExecutionState` is defined but **never used** in the execution loop
- Multi-frame actions (`Wait`, `Patrol`) always return `Running` and never complete
- `Repeat` node's `count` parameter is ignored
- No blackboard system for inter-node communication
- `generate_json_schema()` outputs opaque descriptions for ConditionExpr/ActionExpr
- All 6 behavior presets depend on stubbed conditions/actions — **zero presets function in-game**

---

## 2. Major Gaps (Significant)

### 2.1 Rendering Limitations
- **Forward rendering only** — limited to 8 lights total (2 directional + 6 local)
- **Single shadow map cascade** — fixed 2048×2048 orthographic shadow at camera center
- **No HDR pipeline** — tone-mapping is in-shader but no bloom, no eye adaptation
- **No post-processing** — `ViewKind::PostProcess` defined but no passes
- **No frustum culling** — all objects drawn always
- **No LOD system** — no mesh LOD selection
- **No normal mapping** — WGSL shader receives normals but doesn't sample normal maps
- **No compute shaders** — no GPU particles, no GPU culling, no post-processing passes
- **No skeleton animation rendering** — `upload_bone_matrices`/`draw_skinned_mesh` stubs exist
- **Vulkan backend** is completely non-functional (device creation + sync only, no pipelines)
- **Render graph** is architecturally defined but never executed by any backend
- Material system gap: `StandardMaterial3D` has 5 texture slots but WGPU backend only passes flat colors

### 2.2 No Real Audio Backend
- Only `NullAudioBackend` and `MemoryAudioBackend` exist — no sound output possible
- OGG/MP3 decoding is unimplemented (WAV PCM only)
- No spatial HRTF, no doppler effect, no audio streaming

### 2.3 Animation System is Hollow
- `engine-animation` is a generic property-track keyframe system
- **No skeletal animation**: No bone-indexed tracks, no skinning evaluation
- **No blend trees**: Zero blend tree code despite module doc claiming it
- **No animation state machine**: No transitions, conditions, or layers
- **No animation import**: `ResourceKind::Animation` exists but no importer handles it
- `engine-skeleton` has `Mat4` and skinning matrix computation — but it's disconnected from the animation crate

### 2.4 Scene Graph ↔ Runtime Gap
The 6 declarative layers are **schema-only** — they serialize/validate JSON but have no runtime execution:
- Scene schema: validates JSON but cannot construct an `engine_ecs::Scene`
- UI schema: defines layouts but no runtime UI rendering
- Systems config: defines combat/economy/progression but no integration with engine systems
- Asset manifest: defines assets but no auto-loading pipeline
- Project schema: no project loading/saving to disk

### 2.5 Editor Build/Run Gap
- **Build command** is registered but has no executable handler — cannot produce a standalone game build
- **Debugger** is absent — no breakpoints, no step-through, no variable inspection
- **Material editor** is metadata-only stub
- **Animation editor** doesn't exist at all
- 3D viewport gizmos exist in frontend code but backend `GizmoService` has no rendering integration
- `PickingService` has no raycast/BVH backend

### 2.6 Component Gaps for Large 3D Games
Missing ECS components:
- `NavMeshAgent` / `NavMeshObstacle` — AI pathfinding
- `Terrain` — heightmap terrain
- `LODGroup` — level-of-detail management
- `Animator` / animation state machine
- `BillboardRenderer` / `TrailRenderer` / `LineRenderer`
- `Cloth` / `Ragdoll`
- `ReflectionProbe` / `LightProbeGroup`
- `DecalRenderer`
- `Wind` zone
- UI components (Canvas, RectTransform, Text, Image, Button, Slider)
- Multiplayer (NetworkTransform, NetworkIdentity)

---

## 3. Integration Gaps (Wiring Needed)

### 3.1 AI ↔ ECS
- `AgentOperation::GenerateAsset` is a **hard stub** — returns error
- `AgentOperation::AttachBehavior` with inline behavior trees warns "not yet fully implemented"
- `AgentOperation::MoveEntityTo` animated movement is instantaneous
- `BatchOperation` rollback is broken (TODO: never applies undo state)

### 3.2 Scripting ↔ ECS
- Rhai scripts can only access transforms — no component read/write (health, inventory, AI state)
- Behavior tree `ActionContext` and `ConditionContext` hold scene references but don't access `ComponentData`
- No event/messaging between scripts or between Rhai and declarative backends
- `parse_entity_id()` in Rhai always uses `Generation::FIRST` — stale handle risk

### 3.3 Physics ↔ ECS
- `RigidbodyComponent` and `ColliderComponent` are data holders with no tick-level sync
- `PhysicsSync` in editor handles creation/destruction sync but not continuous transform sync
- `overlap_sphere` returns raw body handles, not entity IDs — no body→entity mapping

### 3.4 Audio ↔ ECS
- `AudioStreamPlayer2D/3DComponentData` are defined as serializable types
- No system exists to read these components and manage source lifecycle

### 3.5 Assets ↔ Runtime
- Asset import produces CPU bytes but no system maps them to GPU memory
- No asset streaming, no compression (all JSON), no build pipeline for release
- Script resources have zero validation

### 3.6 Frontend ↔ Backend
- Single stringly-typed `rpc(method, params)` IPC channel with ~59 methods
- No TypeScript type generation from Rust types
- `EditorHostState` uses `unsafe impl Send + Sync` because Rhai is `!Send`
- I18n format strings use `{}` in Rust but `{key}` in TypeScript — inconsistency

### 3.7 Orphan Code
- ~~`crates/engine-declarative/`~~ — **Removed** (was 4 orphaned source files with no `lib.rs` or `Cargo.toml`)

---

## 4. What Works Well

### 4.1 Single-Agent AI Pipeline (`engine-ai`)
- `AgentSession` is fully functional with 18 operation types
- 5 AI providers (Anthropic, OpenAI, Ollama, Gemini, Codex OAuth)
- Multi-turn conversation with streaming
- Plan-then-apply workflow with policy enforcement
- 7 prefab types and 6 behavior presets for LLM building blocks
- Rigorously tested (19 tests)

### 4.2 Capability-Based Security (`engine-policy`)
- `DefaultCapabilityIssuer` with 10-step deterministic evaluation
- Real HMAC-SHA256 signing with `hmac` + `sha2` crates
- 15 trust label variants, 4 risk classes
- Well-designed `ContextPacket`, `TaskBrief`, `ReviewRubric`, `RepairPolicy` types

### 4.3 Rhai Scripting Backend
- Complete lifecycle model (`on_start`, `on_update`, `on_fixed_update`)
- Per-entity persistent scopes
- Input, Transform, World, Physics, Resource APIs
- Proper sandboxing (disables eval, file I/O, networking)
- 24 tests

### 4.4 Physics
- Three backends: Null, Simple (deterministic), Rapier3D (full-featured, feature-gated)
- Raycasts, overlap queries, contact events
- Character controller (sphere-sweep)
- Joint system (6 types) with motors and limits
- 18 tests

### 4.5 Asset Pipeline
- Full glTF mesh/material extraction
- PNG decode with mip chain generation
- Hot-reload with `notify`-based file watcher
- Content-hash based import caching
- Versioned serialization formats throughout

### 4.6 Editor Copilot Integration
- AI agent is actually wired to the editor UI (`CopilotPanel.tsx`, `AiPanel.tsx`)
- Streaming response support via Tauri events
- Multi-turn conversation with context

### 4.7 Time System
- Fixed timestep accumulator with spiral-of-death protection
- Interpolation fraction for smooth rendering
- Max delta time clamping

### 4.8 Testing Discipline
- Zero `unsafe` in 19+ crates
- `#![deny(missing_docs)]` enforced
- ~150+ tests across the codebase
- Zero TODO/FIXME/HACK/XXX in most crates (except declarative scripting)

---

## 5. Priority Roadmap

### Phase 1: Foundation (Unblock Everything Else)
1. **Add `Mat4`, `Vec2`, `Vec4`, `Ray`, `Plane`, `Frustum`, `AABB` to `engine-core`**
2. **Implement ECS query/iteration** — archetype or sparse-set storage, `Query<(&T, &mut U)>` pattern
3. **Finish declarative behavior tree runtime** — implement all 13 TODO-stubbed actions/conditions, add state machine
4. **Implement `ModelWorker` for the `Worker` trait** — make Workers executable
5. **Wire `engine-agent-cluster` into `runtime-min`** — give it a consumer

### Phase 2: Core Game Systems
6. **Implement skeletal animation** — bone-indexed tracks, blend trees, state machine, GPU skinning
7. **Add real audio backend** — cpal or kira for cross-platform output, OGG decoding
8. **Upgrade rendering** — deferred path, shadow cascades, HDR+bloom, frustum culling, LOD
9. **Add missing ECS components** — NavMeshAgent, Terrain, LODGroup, Animator, UI components
10. **Implement ECS event system** — `EventReader<T>`/`EventWriter<T>`, cross-system communication

### Phase 3: Editor & Workflow
11. **Implement build/packaging pipeline** — produce standalone binary builds
12. **Add material editor, animation editor, terrain editor**
13. **Implement 3D gizmos** — wire `GizmoService` to actual rendering
14. **Add debugger** — breakpoints, step-through, variable inspection for scripts
15. **Type-safe IPC** — generate TypeScript types from Rust Tauri commands

### Phase 4: Production Features
16. **Asset build pipeline** — compression, binary formats, texture optimization
17. **GPU particles** — compute shader simulation
18. **Reflection probes, light probes**
19. **Physics features** — vehicles, cloth, convex mesh colliders for Simple backend
20. **Multiplayer/Networking** — `NetworkTransform`, `NetworkIdentity`, replication
21. **Plugin system** — extension API for editor

---

## 6. Quick Wins (High Impact, Low Effort)

| Task | Effort | Impact |
|------|--------|--------|
| ~~Remove `crates/engine-declarative/` dead code~~ | ~~5 min~~ | **DONE** |
| Add `Vec2`, `Vec4`, `Mat4` to `engine-core` | 2-4 hours | Unblocks rendering math |
| Fix batch rollback TODO in `engine-ai` (line 1146) | 1 hour | Correctness |
| Fix `parse_entity_id()` Generation::FIRST in Rhai | 30 min | Correctness |
| Implement `HasTag` condition | 1 hour | Unblocks 3 behavior presets |
| Implement `PlayerDistance` condition | 1 hour | Unblocks 5 behavior presets |
| Implement `Wait` action with state tracking | 2 hours | Enables timed behavior sequences |
| Wire grant hash enforcement to tool layer | 3 hours | Security |
| Fill in `ContextHash` with actual hashing | 1 hour | Cluster integrity |
| Add OGG decode to audio | 3 hours | Audio format support |
