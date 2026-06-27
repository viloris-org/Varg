# AI Tool Discovery, Skills, Permissions, and Modeling Plan

Status: Draft implementation plan
Last updated: 2026-06-27

## Purpose

This document captures the planned changes needed to make Varg's AI authoring surface scale beyond a short list of always-visible tools. It focuses on four connected needs:

- precise scene and model authoring tools;
- searchable tool discovery instead of a large system prompt;
- skills and references loaded only when relevant;
- permission checks that understand tool capabilities, not only broad read/write modes.

This plan is subordinate to [`docs/ai-agent-unified-spec.md`](./ai-agent-unified-spec.md), [`docs/ai-editor-copilot-prd.md`](./ai-editor-copilot-prd.md), [`docs/ai-editor-quest-prd.md`](./ai-editor-quest-prd.md), and [`docs/varg-language-family-spec.md`](./varg-language-family-spec.md). Product safety claims must continue to follow the implemented behavior described in the unified AI spec.

## Motivation

Varg currently has useful AI operation primitives such as `create_object`, `set_property`, `write_script`, `write_file`, `query_scene_semantic`, and `show_in_viewport`. Those tools are enough for small scene edits, but they are not enough for precise modeling or asset authoring.

Blender MCP demonstrates why agents can model effectively in Blender:

- the agent can inspect scene/object state;
- the agent can mutate objects through a stable API;
- the agent can capture viewport screenshots for visual feedback;
- the agent has a broad escape hatch through Blender Python.

Varg should borrow the first three ideas while avoiding a default arbitrary-code execution path. Modeling should primarily use structured Varg tools, declarative Varg authoring files, and validated asset generation.

The prompt surface also needs to stay small. Putting every tool, every `.vscene`/`.vasset` example, every permission rule, and every modeling instruction in the system prompt would make the agent slower and less reliable. Varg should expose a small base tool set and let the agent discover tools and read skills as needed.

## Design Principles

- Keep the system prompt short. It should describe the workflow, not every tool.
- Prefer structured Varg tools over shell commands or arbitrary scripts.
- Make tool discovery explicit and typed.
- Make skill loading explicit and scoped to the task.
- Keep permission authority in trusted Rust code, not in model instructions.
- Treat tool search as discovery only. Search results must not grant permission.
- Require evidence for meaningful AI-authored scene and asset changes.
- Route broad, risky, or multi-artifact modeling work to Quest.

## Reference Pattern From Codex

Codex provides a useful pattern to adapt:

- tools can be directly visible, deferred for discovery, direct-model-only, or hidden;
- deferred tools are searchable through lightweight metadata;
- tool search returns loadable tool specs rather than injecting every full schema up front;
- skills expose lightweight metadata by default, while full `SKILL.md` contents are read on demand;
- approval behavior is split between broad approval policy and granular approval categories.

Varg should use the same broad shape, with Varg-specific metadata for type, stage, capabilities, risk, and evidence.

## Default Tool Surface

Only foundational tools should be visible at turn start:

| Tool | Purpose |
| --- | --- |
| `tool_search` | Find deferred tools by query, type, capability, stage, and risk. |
| `skill_search` | Find available Varg skills and reference packs. |
| `skill_read` | Read a selected skill or referenced instruction file. |
| `get_current_context` | Read the current project, scene, selection, active file, and diagnostics summary. |
| `request_capability` | Ask the permission gate for scoped access needed by a tool chain. |
| `complete` | End the task with a summary and evidence pointers. |

All specialized tools should start as deferred unless there is a strong usability reason to expose them directly.

## Tool Exposure

Introduce an exposure enum for AI tools:

```rust
pub enum ToolExposure {
    Direct,
    Deferred,
    DirectModelOnly,
    Hidden,
}
```

Definitions:

- `Direct`: included in the initial model-visible tool list.
- `Deferred`: searchable but omitted from the initial tool list until selected.
- `DirectModelOnly`: visible to the model but not available through nested or secondary execution surfaces.
- `Hidden`: registered for trusted dispatch but not model-visible or searchable.

Expected defaults:

- base discovery/context tools are `Direct`;
- modeling, material, asset, validation, and command tools are `Deferred`;
- internal editor plumbing tools are `Hidden`;
- emergency compatibility tools can use `DirectModelOnly` only when needed.

## Tool Metadata

Varg tool metadata should extend a normal tool schema with planning and policy fields:

```rust
pub struct VargToolMetadata {
    pub name: String,
    pub description: String,
    pub tool_type: ToolType,
    pub stage: Vec<ToolStage>,
    pub capabilities: Vec<Capability>,
    pub risk: RiskClass,
    pub evidence: Vec<EvidenceKind>,
    pub skill_refs: Vec<String>,
    pub keywords: Vec<String>,
}
```

Suggested tool types:

```text
context
scene
entity
component
asset
mesh
material
viewport
script
validation
filesystem
command
memory
quest
skill
```

Suggested stages:

```text
inspect
author
refine
verify
repair
review
apply
```

Tool search should index:

- tool name;
- name with separators expanded;
- description;
- parameter names and parameter descriptions;
- tool type;
- stage;
- capabilities;
- keywords;
- related skill names and short descriptions.

## Tool Search

`tool_search` should support typed filtering and ranked discovery:

```json
{
  "query": "create a sci-fi door with bevels and inset panels",
  "types": ["mesh", "material", "viewport"],
  "capabilities": ["asset.write.mesh"],
  "stage": "author",
  "risk_max": "medium",
  "limit": 8
}
```

MVP ranking can use:

1. type filtering;
2. capability filtering;
3. risk filtering;
4. stage boosting;
5. BM25 or keyword scoring over indexed search text.

Semantic embeddings are optional later. They should not be required for the first implementation.

Search results should be grouped by stage when useful:

```text
Inspect: get_scene_info, get_asset_info
Author: create_primitive, create_mesh_asset
Refine: modify_mesh, set_material
Verify: capture_viewport, validate_scene
```

Each result should include:

- name;
- short description;
- type;
- stage;
- risk;
- required capabilities;
- related skills;
- whether the full schema has been loaded.

## Skills

Skills should carry task knowledge that would otherwise bloat the system prompt. Varg should support lightweight metadata plus on-demand content.

Initial skills:

```text
varg-modeling
varg-materials
varg-scene-authoring
varg-behavior-scripting
varg-permissions
varg-asset-pipeline
```

Varg skills are split by scope:

```text
<project>/.varg/skills/   # project skills, highest priority
~/.varg/skills/           # user-global skills
```

Project skills are for project-specific conventions such as asset naming,
gameplay architecture, world style, and local scripting patterns. User-global
skills are for reusable personal workflows and preferences that should not be
committed into every project.

Suggested project skill layout:

```text
<project>/.varg/skills/varg-modeling/SKILL.md
<project>/.varg/skills/varg-modeling/references/primitives.md
<project>/.varg/skills/varg-modeling/references/mesh-operations.md
<project>/.varg/skills/varg-modeling/references/examples/sci-fi-props.md
<project>/.varg/skills/varg-materials/SKILL.md
<project>/.varg/skills/varg-materials/references/pbr-materials.md
```

Suggested user-global skill layout:

```text
~/.varg/skills/varg-modeling/SKILL.md
~/.varg/skills/personal-quest-style/SKILL.md
```

`SKILL.md` should be concise. Deeper examples and syntax references should live in referenced files and be loaded only when needed.

When project and global skills have the same name, the project skill should take
priority in search ranking. Search results must still expose resolved IDs that
include the source, for example:

```text
project://skills/varg-modeling
global://skills/varg-modeling
```

`skill_read` should require one of these resolved IDs and an optional path inside
that skill directory. It must reject absolute paths and `..` traversal.

Tool metadata may point to skills:

```json
{
  "tool": "modify_mesh",
  "skill_refs": [
    "varg-modeling/references/mesh-operations.md"
  ]
}
```

Skill reads should be subject to policy. Reading built-in or project-approved skills is usually low risk. Reading external or plugin-provided skills may require a trust label, dependency check, or user approval.

## Permissions

The permission model needs to move from broad read/write categories toward capability-based checks.

Suggested capabilities:

```text
context.read
scene.read
scene.write.entity
scene.write.component
asset.read
asset.write.generated
asset.write.mesh
asset.write.material
viewport.capture
tool.search
skill.search
skill.read
script.execute.sandboxed
command.run
network.fetch_asset
quest.create
```

Each tool declares required capabilities. The permission gate decides whether the current turn, session, Quest workspace, or grant allows those capabilities.

Example tool policy:

```json
{
  "tool": "create_mesh_asset",
  "requires": ["asset.write.mesh", "scene.write.entity"],
  "risk": "medium",
  "evidence": ["diff", "asset_reference_check", "scene_preview"],
  "rollback": "required"
}
```

Capability decisions should use this shape:

```text
approved
narrowed
denied
requires_user_approval
requires_quest
```

Examples:

- `get_scene_info`: low risk, usually auto-approved.
- `capture_viewport`: low risk, usually auto-approved.
- `create_primitive`: medium risk, requires scene write approval or session grant.
- `create_mesh_asset`: medium risk, requires asset write approval plus evidence.
- `run_modeling_script`: high risk, requires explicit approval and should usually route to Quest.
- arbitrary code execution: critical risk, disabled by default.

## Granular Approval

Add Varg-specific granular approval controls:

```rust
pub struct GranularApprovalConfig {
    pub scene_write: bool,
    pub asset_write: bool,
    pub mesh_generation: bool,
    pub script_execution: bool,
    pub external_command: bool,
    pub network_asset_fetch: bool,
    pub skill_read: bool,
    pub request_capability: bool,
}
```

These controls decide whether the UI may ask the user for approval in a category. They do not automatically approve the operation. The deterministic policy gate still checks paths, assets, entities, risk, evidence requirements, and execution mode.

## Modeling Tools

The first modeling tool set should focus on structured, inspectable operations:

| Tool | Type | Purpose |
| --- | --- | --- |
| `get_scene_info` | scene | Return scene hierarchy, entities, components, cameras, lights, and selected object summary. |
| `get_object_info` | entity | Return detailed component, transform, mesh, material, and bounds information for one object. |
| `get_asset_info` | asset | Return metadata and references for an asset. |
| `create_primitive` | mesh/scene | Create a primitive object with transform and material. |
| `create_mesh_asset` | mesh/asset | Generate a mesh asset from structured primitives or mesh operations. |
| `modify_mesh` | mesh | Apply operations such as bevel, inset, extrude, mirror, boolean, or array. |
| `set_material` | material | Create or assign material parameters. |
| `set_transform` | component | Set object transform using a structured transform value. |
| `duplicate_object` | scene | Duplicate an entity with transform offsets or array placement. |
| `capture_viewport` | viewport | Capture a viewport preview for visual feedback. |
| `validate_scene` | validation | Check references, missing assets, schema validity, and basic scene constraints. |

Later tool additions:

```text
boolean_operation
array_modifier
bevel_edges
extrude_faces
assign_uv
generate_collision
retopology_hint
lod_generate
```

## Modeling Scripts and Asset DSL

Precise modeling should not require the model to emit large raw vertex arrays. Varg should provide higher-level authoring operations and declarative asset files.

Potential `.vasset` model declaration:

```varg
model SciFiCrate {
    material body {
        base_color: rgb(0.18, 0.22, 0.25)
        roughness: 0.75
    }

    cube "body" {
        size: [2.0, 1.2, 1.2]
        bevel: 0.08
        material: body
    }

    repeat x 4 {
        cube "rib" {
            size: [0.08, 1.35, 1.32]
            position: [index * 0.45 - 0.675, 0, 0]
        }
    }
}
```

This syntax direction should be reconciled with [`docs/varg-language-family-spec.md`](./varg-language-family-spec.md) before implementation.

An arbitrary modeling script runner may be useful later, but it should not be the default modeling path. If introduced, it must be sandboxed, bounded by time and output limits, audited, and gated by explicit approval or Quest policy.

## Evidence and Review

AI-authored modeling operations should produce evidence proportional to risk.

Recommended evidence kinds:

```text
operation_preview
scene_diff
asset_diff
asset_reference_check
viewport_preview
validation_log
rollback_plan
```

Examples:

- Low-risk inspection tools need trace entries only.
- Medium-risk scene writes need operation preview and scene diff.
- Mesh asset writes need asset diff, reference check, and viewport preview.
- Script execution needs logs, generated outputs, and rollback plan.
- Critical operations are refused unless a future policy explicitly supports them.

## Editor AI Versus Quest

Editor AI should handle:

- inspection;
- one or a few local scene edits;
- primitive placement;
- small material edits;
- focused mesh generation with user approval;
- viewport feedback loops that finish quickly.

Quest should handle:

- broad scene generation;
- multiple related assets;
- imported or generated external assets;
- repeated visual refinement loops;
- high-risk modeling scripts;
- work requiring persistent review and apply.

## Implementation Phases

### Phase 1: Tool Registry and Search

- Add tool exposure metadata.
- Build a registry of direct, deferred, and hidden tools.
- Implement `tool_search` over tool metadata.
- Keep existing Copilot tools working through compatibility mapping.
- Add tests for filtering by type, capability, risk, and stage.

### Phase 2: Skill Registry

- Add skill metadata discovery for built-in Varg skills.
- Add `skill_search` and `skill_read`.
- Add budgeted skill metadata injection to the prompt.
- Add skill trust labels and dependency metadata.
- Add tests for on-demand skill reading and disabled skill handling.

### Phase 3: Capability-Based Permission Gate

- Add capability declarations to tool metadata.
- Extend policy checks to evaluate capabilities, paths, entities, assets, risk, and evidence.
- Add `request_capability`.
- Add granular approval categories for Varg tools.
- Add tests that search can reveal a tool but execution still fails without capability.

### Phase 4: Structured Modeling Tools

- Add scene/object/asset inspection tools.
- Add primitive creation and transform/material tools.
- Add viewport capture and scene validation.
- Add mesh asset generation through structured operations.
- Add evidence output and rollback hooks.

### Phase 5: Declarative Modeling Authoring

- Extend `.vasset` or introduce a model authoring sub-syntax aligned with the Varg language family.
- Compile model declarations to mesh/material assets.
- Add examples in skills.
- Add validation and editor previews.

### Phase 6: Sandboxed Modeling Script Runner

- Evaluate whether an escape hatch is still needed after structured tools and `.vasset` modeling exist.
- If needed, implement it as sandboxed, bounded, audited, and off by default.
- Route high-risk use to Quest.

## Open Questions

- Should model authoring stay inside `.vasset`, or should Varg add a distinct `.vmodel` role?
- Should `skill_read` be a tool or should it use the same resource API as project docs and MCP resources?
- What is the minimum viewport capture format needed for model feedback in Editor AI?
- How much mesh topology editing belongs in Varg versus in imported DCC tools?
- Should tool search return full schemas immediately or require a second explicit load step?
- How should project/plugin skills be trusted and enabled in the editor UI?
