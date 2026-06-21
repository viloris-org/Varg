# Aster Quest Detailed Specification

Status: Draft detailed sub-spec
Parent: [`docs/ai-agent-unified-spec.md`](./ai-agent-unified-spec.md)
Last updated: 2026-06-21

## Purpose

This document defines the detailed behavior for **Quest**, Aster's persistent AI task system for durable game-making work.

Quest is the primary AI-native product direction. It exists for work that needs persistent intent, artifacts, validation, review, recovery, and final apply decisions. Local temporary assistance belongs in Editor AI, defined in [`docs/ai-editor-copilot-prd.md`](./ai-editor-copilot-prd.md).

This document follows the safety commitments in [`docs/ai-agent-unified-spec.md`](./ai-agent-unified-spec.md). It must not claim stronger isolation or authorization than the currently implemented Quest execution path provides.

## Goals

Quest should let a user:

- turn a broad game-making intent into a durable task;
- preserve goal, constraints, spec, timeline, artifacts, and decisions;
- let AI inspect, plan, generate, validate, repair, and prepare reviewable results;
- review changed files, generated assets, diagnostics, validation, risks, and unresolved issues;
- open Quest artifacts in the Editor for inspection or manual correction;
- approve, reject, revise, partially accept, archive, reopen, or continue the task.

Quest succeeds when broad AI work feels recoverable and inspectable rather than like a long chat transcript.

## Non-Goals

Quest does not currently promise:

- full enterprise zero-trust automation;
- OS-level sandboxing;
- signed grant enforcement;
- no active-project mutation unless the implemented execution layer enforces draft workspace apply;
- fully unattended production changes;
- cloud synchronization;
- multi-user collaborative review;
- marketplace governance.

Quest is allowed to have future architecture hooks for these, but product copy must reflect only implemented guarantees.

## Quest Versus Editor AI

Use Quest when work is:

- durable;
- broad;
- multi-file;
- multi-artifact;
- risky;
- likely to need validation or repair loops;
- likely to need review before apply;
- useful to resume after restart;
- useful to archive as a task record.

Use Editor AI when work is:

- local;
- temporary;
- tied to current selection or file;
- easy to preview;
- easy to undo;
- answerable as explanation.

Editor AI can promote to Quest. Quest can open artifacts in Editor AI or the editor workspace for inspection and manual correction.

## Quest Record

Each Quest should have a durable record.

Required fields:

- `id`;
- `title`;
- `status`;
- `project_path`;
- `created_at`;
- `updated_at`;
- `intent_path` or embedded intent;
- `event_log_path`;
- `artifacts`;
- `review_state`;
- `execution_config`;
- `knowledge_context`;
- `final_decision`.

Optional fields:

- `spec_path`;
- `workspace_id`;
- `workspace_path`;
- `base_revision`;
- `snapshot_id`;
- `branch_name`;
- `parent_quest_id`;
- `model_config`;
- `validation_summary`;
- `risk_summary`.

Quest metadata should live in the editor profile by default, not inside the project, unless explicitly exported.

## Intent And Spec

Every Quest starts with intent.

Intent should capture:

- user's goal;
- selected project context;
- constraints;
- non-goals;
- relevant diagnostics;
- desired output;
- acceptance hints;
- source, if promoted from Editor AI.

A spec is optional for simple or investigative Quests, but required before broad write execution when the task is ambiguous, risky, or multi-artifact.

Spec should include:

- goal;
- scope;
- non-goals;
- affected files/artifacts if known;
- expected behavior;
- validation plan;
- review criteria;
- unresolved decisions.

The user can edit intent or spec before execution. During execution, edits should create a timeline event and may require replanning.

## Status Model

Canonical statuses:

- `draft`: intent exists but execution has not started.
- `clarifying`: Quest needs user answers.
- `specified`: enough intent/spec exists to proceed.
- `planning`: AI is preparing steps or execution approach.
- `prepared`: context, examples, and execution configuration are ready.
- `running`: AI is reading, editing, generating, or inspecting.
- `waiting_for_user`: Quest needs a decision, credentials, manual edit, or approval.
- `validating`: deterministic checks are running.
- `repairing`: AI is addressing validation or review findings.
- `ready_for_review`: result or report is ready for user review.
- `applying`: accepted work is entering the active project through the implemented apply path.
- `completed`: final decision recorded.
- `blocked`: cannot proceed under current constraints.
- `archived`: hidden from active work but retained.

Quest status is not a fixed wizard. The orchestrator may skip or repeat states based on task needs.

## Timeline Events

Timeline events should be append-only.

Event types:

- `intent_created`;
- `intent_updated`;
- `spec_created`;
- `spec_updated`;
- `clarification_requested`;
- `clarification_answered`;
- `plan_created`;
- `context_attached`;
- `example_retrieved`;
- `file_read`;
- `file_changed`;
- `scene_changed`;
- `asset_generated`;
- `command_run`;
- `validation_started`;
- `validation_passed`;
- `validation_failed`;
- `repair_started`;
- `repair_finished`;
- `manual_intervention_requested`;
- `manual_intervention_completed`;
- `review_ready`;
- `issue_reported`;
- `quick_fix_requested`;
- `decision_recorded`;
- `apply_started`;
- `apply_finished`;
- `blocked`;
- `archived`;
- `reopened`.

Event fields:

- `id`;
- `quest_id`;
- `type`;
- `timestamp`;
- `summary`;
- `details`;
- `artifact_refs`;
- `diagnostic_refs`;
- `actor`: user, system, model, validator, editor;
- `trust_label`.

Timeline should show meaningful progress, not raw token-by-token model output.

## Artifacts

Quest artifacts are reviewable outputs or evidence.

Artifact types:

- intent;
- spec;
- plan;
- changed file;
- generated file;
- scene preview;
- asset preview;
- validation log;
- diagnostic report;
- diff;
- review report;
- blocked report;
- unresolved issue;
- quick-fix result;
- transaction group;
- final decision.

Artifact fields:

- `id`;
- `type`;
- `label`;
- `path` or storage reference;
- `summary`;
- `created_at`;
- `source_event_id`;
- `trust_label`;
- `open_in_editor_target`;
- `validation_state`.

Artifacts should be openable in the appropriate editor surface when possible.

## Execution Profiles

Quest may use different execution profiles as implementation matures.

### Controlled Profile

MVP profile. Uses the same execution gate as Editor AI, with Quest timeline and review wrapping the result.

Appropriate for:

- simple file generation;
- small multi-step tasks;
- investigation reports;
- controlled validation runs.

Limitations:

- may still modify active project if the current execution path does;
- must not claim draft workspace isolation.

### Draft Workspace Profile

Target profile for broad Quest writes.

Behavior:

- create or reuse a task workspace or staging area;
- perform broad edits away from active project;
- collect diff, diagnostics, validation, and review artifacts;
- let user discard or apply reviewed results.

Commitment only after implemented:

- broad Quest writes do not directly touch active project before review/apply.

### Cluster Profile

Future profile.

Behavior:

- orchestrator decomposes work;
- workers handle scoped tasks;
- reviewers inspect artifacts;
- grants may constrain tool access if enforced by code.

This profile must remain hidden implementation detail unless there is a user-facing reason to expose it.

## Execution Flow

### Create

1. User enters a goal or promotes from Editor AI.
2. System creates Quest record.
3. Intent artifact is created.
4. Initial timeline event is recorded.
5. Orchestrator selects next state: clarify, specify, plan, inspect, or run.

### Clarify

Clarify only when the answer affects:

- product intent;
- scope;
- cost;
- reversibility;
- risk;
- final review expectations.

Do not ask the user to decide internal worker type, tool choice, or implementation detail.

### Specify

Generate or update a spec when:

- task is broad;
- multiple approaches are reasonable;
- expected outcome is ambiguous;
- validation or review needs acceptance criteria;
- the task may affect many artifacts.

### Run

1. Gather context and relevant examples.
2. Normalize planned operations or task steps.
3. Execute through the safest implemented profile.
4. Record timeline events.
5. Attach artifacts and diagnostics.
6. Stop on unsupported or unsafe operations.

### Validate

Validators should run when available:

- language syntax;
- schema;
- asset references;
- scene load;
- script diagnostics;
- targeted tests;
- command registry checks.

Validation output becomes artifacts.

### Repair

Repair is allowed when:

- failure is local and understandable;
- retry limit is not exhausted;
- repair remains in scope;
- repair does not require new risky authority.

Repair should create timeline events and preserve failed evidence.

### Review

Quest enters `ready_for_review` when it has:

- generated output or investigation report;
- changed artifact list;
- validation state;
- unresolved issues;
- decision options.

### Apply

Apply behavior depends on implemented safety layer.

Layer 1:

- apply may mean controlled active-project operations.
- UI must not imply draft workspace isolation.

Layer 2:

- apply promotes reviewed draft workspace changes into active project.
- UI should show transaction groups, diffs, validation, and rollback hints.

In all layers, final apply should be explicit.

## Review Surface

Quest review must answer:

- What was requested?
- What changed?
- Which files, scenes, assets, or entities are affected?
- What validation ran?
- What passed?
- What failed?
- What issues remain?
- What risks remain?
- What can be applied, partially applied, revised, quick-fixed, discarded, archived, or reopened?

Review should compress internal AI activity by default. Expandable details may show logs, traces, events, and intermediate artifacts.

## Unresolved Issues And Quick Fixes

Unresolved issue fields:

- `id`;
- `severity`: blocking, non-blocking, advisory;
- `summary`;
- `affected_artifacts`;
- `evidence_refs`;
- `recommended_action`;
- `requires_user_input`;
- `quick_fix_available`;
- `quick_fix_scope`.

Quick fixes should be scoped to the issue. They should not rerun unrelated work unless necessary.

## Partial Acceptance

Partial acceptance is allowed only when changes can be grouped cleanly.

Transaction group fields:

- `id`;
- `summary`;
- `artifact_refs`;
- `dependencies`;
- `validation_state`;
- `risk_hint`;
- `apply_state`.

If partial acceptance cannot be implemented safely, the UI should offer full accept, reject, revise, or manual open-in-editor instead.

## Open In Editor

Quest artifacts should open in the editor when useful.

Mappings:

- changed code file -> script/behavior editor;
- scene artifact -> Scene View and Hierarchy;
- entity change -> Inspector selection;
- asset artifact -> Project/Assets panel;
- diagnostic -> Console and relevant file;
- spec/intent -> text artifact editor;
- diff -> review surface.

Manual edits made in Editor while Quest is active should be recorded as manual intervention evidence where possible.

## Knowledge

Quest may propose Knowledge updates after completion, blocking, or investigation.

Knowledge proposals must include:

- proposed fact or preference;
- source artifacts;
- confidence;
- whether user approval is required;
- suggested scope: project or user.

Quest-local assumptions must not silently become Knowledge.

## UI Requirements

Quest UI should include:

- Quest registry;
- create/new prompt;
- title and status;
- project identity;
- intent/spec tabs;
- timeline;
- artifact list;
- validation section;
- unresolved issue section;
- review/decision controls;
- open-in-editor controls;
- archive/reopen controls;
- execution configuration where useful.

The user should always know whether work is:

- only intent;
- running;
- draft;
- validated;
- ready for review;
- applied;
- blocked;
- archived.

## Execution Configuration

Quest may expose limited configuration:

- model/provider selection or inherit editor default;
- thinking effort where supported;
- execution profile hint;
- validation level;
- whether to generate spec first.

Do not expose low-level agent routing or internal permission mechanics as default controls.

## Acceptance Criteria

Quest shell MVP is acceptable when:

- user can create, rename, archive, reopen, and delete a Quest;
- Quest persists across editor restart;
- intent artifact is stored and editable;
- optional spec artifact is stored and editable;
- timeline records major events;
- artifacts can be listed and opened;
- review surface can show changed files, diagnostics, issues, and decisions;
- Editor AI conversation can promote into a Quest intent;
- Quest can open relevant artifacts in Editor.

Quest execution MVP is acceptable when:

- a bounded task can run through the shared execution gate;
- execution events appear in timeline;
- generated or changed artifacts are attached;
- validator output is attached when available;
- blocked outcomes preserve evidence and next actions;
- final apply is explicit;
- UI does not claim stronger isolation than implemented.

Draft workspace milestone is acceptable when:

- broad Quest write work happens outside active project;
- diffs are available before apply;
- discard leaves active project unchanged;
- apply uses a reviewed path;
- failed apply has recovery behavior.

## Test Requirements

Required tests:

- Quest record persistence;
- status transitions;
- append-only event writing;
- intent/spec edit persistence;
- promotion from Editor AI;
- artifact open-in-editor routing;
- review decision persistence;
- blocked report preservation;
- execution profile labels do not overclaim safety;
- draft workspace discard/apply behavior once implemented.

## Migration From Earlier PRDs

Legacy names:

- "Copilot Mode" maps to Editor AI.
- "Auto Mode" maps to Quest with an autonomous or future cluster execution profile.
- "SOLO" maps to an execution style, not a product mode.
- "Manager/Worker/Reviewer" are future cluster implementation details.

Old zero-trust language is historical architecture context only. Current Quest promises are defined by this document and the unified spec.

## Open Questions

- What storage location and retention policy should Quest use in the editor profile?
- What is the first durable `quest.json` schema?
- Which timeline events are required for MVP versus later?
- Which broad writes must wait for draft workspace support?
- How should partial acceptance be grouped?
- What validation level is required before ready-for-review?
- How much execution configuration should users see?
