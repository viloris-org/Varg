# Aster AI-Native Engine Loop Target

This document is the long-form contract for a Claude Code goal-mode run. It is
not a planning essay. It is a working brief for repeatedly editing, testing,
repairing, and improving this repository until the branch is meaningfully
stronger than the remote baselines.

Also read:

```text
docs/ai-native-engine-loop-target/LARGE_DELIVERABLE_CONTRACT.md
```

That document defines the required size of the outcome. Do not downgrade this
task into a small patch or planning-only pass.

## Core Intent

Aster should become an AI-native game editor, not a generic Qoder clone and not
a decorative IDE mockup. The product should feel like a game-engine-aware coding
and editing workbench:

- Quest mode handles longer AI tasks with execution evidence.
- Editor mode lets users inspect game state, assets, scripts, diagnostics, and
  AI outputs.
- Engine subsystems expose enough structure for AI tools to safely edit scenes
  and validate results.
- Changes are reviewed, validated, and applied like a local PR, not silently
  written into the active project.

The branch should compete with:

```text
origin/main
origin/takeover/upstream-integrated
origin/fix/render-pipeline-wgpu
```

The other branches are references, not authority. Inspect them, borrow useful
patterns, then build a cleaner version that fits this repository.

## Operating Rule

Do not stop after a good plan.

Each work loop must produce at least one of:

- a code change;
- a test or build result;
- a fixed failure;
- a documented comparison backed by actual repository evidence.

The normal loop is:

```text
inspect -> choose narrow target -> edit -> verify -> repair -> record -> continue
```

If a slice completes, immediately choose the next highest-value slice. If a slice
fails, reduce its size, repair it, or move to another target that advances the
same product goal.

## First Pass

Start by refreshing the remote view:

```bash
git fetch origin --prune
git status --short --branch
git branch -r --sort=-committerdate
git log --oneline --decorate origin/main..origin/takeover/upstream-integrated
git log --oneline --decorate origin/main..origin/fix/render-pipeline-wgpu
```

Then inspect the highest-value files:

```text
editor/src/renderer/pages/QuestPage.tsx
editor/src/renderer/pages/EditorPage.tsx
editor/src/renderer/quest.ts
editor/src-tauri/src/lib.rs
editor/src-tauri/src/quest.rs
crates/engine-ecs
crates/engine-editor
crates/engine-agent-cluster
crates/engine-policy
crates/engine-physics
crates/engine-render
crates/engine-render-wgpu
crates/engine-audio
crates/runtime-min
```

Before editing, update or create:

```text
docs/ai-native-engine-loop-progress.md
```

Use it as a running log, not a final report.

## Workstream 1: Quest/Agent Reality

The current product direction only matters if Quest can do real work. Improve
the Quest execution path toward this shape:

```text
quest request
-> isolated workspace or safe working context
-> runner
-> file changes or structured scene changes
-> diff
-> validation
-> review bundle
-> apply guard
-> rollback or rollback-ready record
```

Required improvements:

- identify and separate mock/demo execution from real execution;
- keep deterministic stub execution for tests, but make it use the same
  snapshot/diff/validation/review path as real execution;
- prevent direct active-project pollution;
- include stale fingerprint or equivalent stale-state guard before apply;
- record failure evidence instead of pretending success;
- make review data useful to the frontend: changed files, summary, validation,
  warnings, risk, unresolved items, apply readiness.

Avoid a giant rewrite of `editor/src-tauri/src/lib.rs`. If extracting modules,
avoid the Rust `quest.rs` versus `quest/mod.rs` collision. Prefer incremental
module names such as `quest_execution`, `quest_runtime`, or carefully staged
extraction.

## Workstream 2: Frontend Product Quality

The frontend should not merely look more complicated. It should become clearer,
faster to understand, and easier to maintain.

QuestPage should move toward an AI workbench:

- task/quest rail;
- run stream with current action and evidence;
- permission, question, failure, and validation states;
- artifact/review workspace;
- apply/reject/rollback decisions;
- clear empty, loading, running, failed, validating, review, applied states.

EditorPage should move toward an inspection and repair workbench:

- hierarchy and selected entity context;
- inspector and component editing context;
- assets and script surfaces;
- diagnostics and validation evidence;
- AI panel that understands current editor selection.

Frontend changes should reduce dumping-ground files. Split only when it improves
clarity. Do not create decorative components that do not serve the workflow.

Build evidence matters:

```bash
cd editor && bun run build
```

If it fails, fix what is reasonable. If it cannot run because of environment,
write the exact reason to the progress document.

## Workstream 3: SceneCommand And Engine Semantics

AI editing needs structured operations, not free-form prose.

Build or improve a minimal scene command path:

- create entity;
- delete entity;
- add component;
- update component;
- remove component;
- set transform;
- attach asset, material, or script where existing architecture supports it.

Do not use `ComponentSchema` as the runtime payload. Schema is for describing,
validating, and rendering UI. Runtime commands need entity references, component
type ids, typed payload or component data, validation output, and patch/undo
information.

The target shape is:

```text
SceneCommand -> validation -> ScenePatch -> apply/undo evidence
```

Keep this minimal but real. Add tests around the semantics you introduce.

## Workstream 4: Physics, Render, Audio Validation

Do not promise to rewrite every subsystem. Add practical validation or
diagnostic entry points that Quest and Editor can use.

Physics examples:

- activation radius sanity;
- collision configuration checks;
- fluid/wind component parameter validation;
- step/smoke result with warnings.

Render examples:

- visibility or culling diagnostics;
- wgpu pipeline/surface failure evidence;
- GPU particle smoke or configuration validation;
- reuse ideas from `origin/fix/render-pipeline-wgpu` where appropriate.

Audio examples:

- source/listener consistency;
- clip/reference availability;
- spatial parameter ranges;
- basic smoke/diagnostic result.

The important thing is not breadth. The important thing is that subsystem
problems can become structured evidence instead of vague UI text.

## Workstream 5: Safety And Apply Policy

Any AI editing loop must be harder than a chat box that writes files.

Improve or verify:

- path traversal protection;
- command allowlist or policy checking;
- credential/token file guards;
- active project protection;
- review-before-apply;
- stale workspace detection;
- workspace cleanup;
- failure evidence.

If existing policy crates already cover part of this, wire them into the Quest
or Editor path rather than duplicating rules.

## Evidence Documents

Maintain:

```text
docs/ai-native-engine-loop-progress.md
docs/ai-native-engine-loop-comparison.md
```

`progress` should record:

- timestamp or run section;
- files touched;
- verification commands;
- failures and fixes;
- next selected target.

`comparison` should compare against:

```text
origin/main
origin/takeover/upstream-integrated
origin/fix/render-pipeline-wgpu
```

Use facts. Do not claim build/test success without running commands.

## Verification

Use targeted checks during the run:

```bash
cargo metadata --no-deps
cargo fmt --check
cargo test -p <crate>
cargo check -p <crate>
cd editor && bun run build
```

Discover crate names before using them. Do not invent command names. Full
workspace checks are useful later, but targeted checks keep long runs moving.

## Stop Rules

Do not stop for:

- finishing a plan;
- finishing one slice;
- a test failure;
- a build failure;
- a large file;
- needing to re-read context;
- uncertainty that can be resolved from code;
- rate limits that still allow local work;
- UI needing another polish pass.

Stop only for:

- missing credentials or external account access;
- destructive operation approval;
- serious git conflict that cannot be safely resolved;
- mutually exclusive product direction requiring user choice;
- a branch that has a demonstrable end-to-end improvement with code,
  verification, and comparison evidence.

## Final Response Requirement

When stopping, report:

- branch;
- changed files;
- what improved versus each remote reference;
- commands run and their results;
- remaining risks;
- whether the branch is PR-ready;
- the next most valuable follow-up.
