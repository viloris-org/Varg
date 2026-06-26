# Aster Editor AI Detailed Specification

Status: Draft detailed sub-spec
Parent: [`docs/ai-agent-unified-spec.md`](./ai-agent-unified-spec.md)
Last updated: 2026-06-21

## Purpose

This document defines the detailed behavior for **Editor AI**, Aster's local AI assistance surface inside the editor.

Editor AI is not the durable long-horizon task system. It is the immediate, user-driven assistant for explanation, inspection, small scoped edits, diagnostics, and local corrections. Persistent or broad work belongs in Quest, defined in [`docs/ai-editor-quest-prd.md`](./ai-editor-quest-prd.md).

This document must follow the safety and naming rules in [`docs/ai-agent-unified-spec.md`](./ai-agent-unified-spec.md). It must not claim zero-trust isolation, signed grants, OS sandboxing, Policy Daemon enforcement, or no active-project mutation unless those controls are actually implemented in the Editor AI path.

## Goals

Editor AI should let a user:

- ask questions about the current editor state;
- attach selected context explicitly;
- generate or modify small Aster artifacts;
- preview proposed operations before writes;
- approve, deny, or defer writes;
- run low-friction read-only operations;
- see diagnostics and trace entries;
- undo or recover from supported changes;
- promote growing work into a Quest.

Editor AI succeeds when a user can stay in the editor, ask for a local change, understand the proposed change, apply it safely enough for MVP expectations, and continue working without managing a long task workflow.

## Non-Goals

Editor AI does not provide:

- durable task state across broad work;
- multi-agent orchestration;
- enterprise authorization;
- hard sandbox security;
- unattended broad project mutation;
- hidden background work over many files;
- final review bundles for large tasks;
- marketplace or third-party automation governance.

When a request needs these, Editor AI should suggest creating or promoting to a Quest.

## Scope Boundary

Editor AI is appropriate for:

- "What is selected?"
- "Explain this diagnostic."
- "Add a light to this scene."
- "Create a simple patrol behavior."
- "Fix this one script error."
- "Attach an imported model asset to the selected entity."
- "Change this selected entity's transform."
- "Show me which assets this object references."
- "Run a targeted check and explain the result."

Editor AI should route to Quest when the request:

- touches multiple unrelated files or systems;
- needs persistent intent/spec/review;
- needs validation over a broad artifact set;
- needs draft workspace isolation;
- would take multiple execution loops;
- involves risky commands or broad deletion;
- requires partial acceptance or unresolved issue tracking;
- should survive editor restart.

## User Context Model

Editor AI context should be explicit and inspectable.

Default context candidates:

- current project;
- active scene;
- selected entity;
- selected component;
- active file;
- selected text;
- diagnostics visible in Console;
- recent command output;
- selected asset;
- current play/validation state;
- user-selected Knowledge entries.

The user should be able to add or remove context before submitting a prompt when practical.

Context sent to the model should be labeled by source and trust level:

- `EDITOR_STATE`: trusted snapshot produced by editor code;
- `USER_PROMPT`: untrusted user text;
- `PROJECT_FILE`: untrusted project content;
- `DIAGNOSTIC`: validator or tool output;
- `KNOWLEDGE`: user-approved or proposed memory;
- `ASTER_EXAMPLE`: retrieved language example.

## Interaction Flow

### Read-Only Question

1. User asks a question.
2. Editor AI gathers current context.
3. Model answers in prose or requests read-only tools.
4. Read-only operations may execute without write approval.
5. UI shows answer, cited context where useful, and trace entries if tools ran.

Read-only answers should not ask for write permission.

### Scoped Edit

1. User requests a local edit.
2. Editor AI gathers current context and relevant examples.
3. Model returns structured operations and optional explanation.
4. Operations are normalized by the execution gate.
5. UI shows operation preview with permission kind.
6. User approves, denies, or uses session-level approval where allowed.
7. Approved operations execute through the current Editor AI controls.
8. UI shows result, diagnostics, trace entries, and undo/recovery option where available.

### Diagnostic Fix

1. User asks to fix a diagnostic.
2. Editor AI includes diagnostic output, relevant file snippet, language examples, and prior failed attempt if any.
3. Model proposes a minimal fix.
4. User previews and approves the fix.
5. Editor AI applies the fix and runs the relevant validator when available.
6. If validation fails, Editor AI may propose one continuation loop or suggest Quest for broader repair.

### Promote To Quest

Editor AI should offer promotion when:

- the user keeps asking follow-up implementation steps;
- the operation set becomes broad;
- validation requires a durable run;
- the task needs a spec or review;
- the current request cannot be completed confidently in one or two local loops.

Promotion should create a Quest intent record containing:

- original user prompt;
- selected editor context references;
- current conversation summary;
- attempted operations and results;
- diagnostics and unresolved issues;
- suggested next action.

## Operation Model

Editor AI operations should be structured. Prose is allowed for explanation, but writes should be represented as operations.

Minimum operation fields:

- `id`: stable operation ID for the plan;
- `kind`: operation type;
- `permission_kind`: `read`, `write`, or `command`;
- `target`: file, entity, scene, asset, command, or memory target;
- `preview`: user-visible summary;
- `risk_hint`: low, medium, high, or unsupported;
- `requires_approval`: boolean;
- `undo_hint`: available, unavailable, or unknown;
- `validation_hint`: optional validator to run after apply.

Operation groups may be used when several operations form one user-visible change.

## Permission Behavior

Editor AI uses the current MVP permission model: controlled editing, not enterprise authorization.

Permission kinds:

- `read`: may execute with low friction.
- `write`: requires user approval or session-level write approval.
- `command`: requires command-specific approval or allowlist handling.
- `unsupported`: must not execute.

Session-level approval may approve routine writes for the current Editor AI session. It is a convenience, not a security boundary. It must not approve high-risk operations unless the implemented product explicitly supports that risk route.

Commands should be allowlisted by command ID where possible. A permanently allowed command means "allowed by current product setting," not "safe in all contexts."

## Risk Classification

Low-risk examples:

- read current scene;
- summarize selected entity;
- create a small new script or model file under expected project paths;
- modify a selected component field;
- run a registered read-only validator.

Medium-risk examples:

- modify an existing script;
- update project memory;
- create multiple related scene objects;
- run a command that changes generated diagnostics or cache files.

High-risk examples:

- delete files or entities in bulk;
- modify project manifests or dependency files;
- run arbitrary shell commands;
- access network or credentials;
- change many files without a Quest;
- write outside expected project roots;
- make irreversible or hard-to-review edits.

High-risk operations should usually be refused in Editor AI or routed to Quest. If supported, they require explicit UI treatment and implementation-backed checks.

## Tooling

Editor AI tools should prefer structured editor operations over arbitrary shell or text edits.

Useful tool classes:

- scene query;
- entity/component read;
- entity/component mutation;
- file read;
- file write under project roots;
- Aster language validation;
- Varg source validation;
- asset reference query;
- command registry execution;
- trace read;
- undo last AI edit.

Arbitrary command execution is not an MVP default. If exposed, it must be clearly classified and routed through implemented command controls.

## Aster Language Generation

Editor AI must retrieve examples for new Aster languages.

For `.varg`, `.vscene`, and `.vasset` generation:

- include short task-relevant examples;
- include allowed syntax or schema summary;
- include existing project conventions when available;
- include validator diagnostics after failures;
- prefer declarative files over runtime scripts;
- avoid invented helper functions or fields.

Editor AI should not rely on a single giant static prompt. It should retrieve examples by language, task, concept, and diagnostic.

## UI Requirements

Editor AI should include:

- chat transcript;
- context attachments;
- model/provider status;
- operation preview;
- permission labels;
- approve/deny/session-approve controls;
- execution status;
- diagnostics cards;
- trace cards;
- undo/recovery action;
- promote-to-Quest action.

The operation preview should answer:

- What will change?
- Where will it change?
- Is it read, write, command, or unsupported?
- Can it be undone?
- What validation will run?

The result view should answer:

- What was applied?
- What failed?
- What diagnostics remain?
- Can the user undo or retry?
- Should this become a Quest?

## State Model

Editor AI session states:

- `idle`: no active request.
- `thinking`: model is producing answer or operations.
- `ready`: operations are previewed and waiting for decision.
- `executing`: approved operations are running.
- `complete`: request finished.
- `error`: request failed.
- `interrupted`: user interrupted generation or execution where supported.

Operation states:

- `planned`;
- `approved`;
- `denied`;
- `running`;
- `applied`;
- `failed`;
- `skipped`;
- `undone`.

The UI should not imply a write is applied while it is only planned.

## Diagnostics And Trace

Every executed operation should produce enough trace data for user inspection and maintainer debugging.

Trace fields:

- operation ID;
- operation kind;
- target;
- permission kind;
- approval source;
- execution result;
- diagnostic references;
- recovery hint;
- timestamp.

Trace is not a security audit in the enterprise sense. It is an inspection and debugging aid for the current product.

## Acceptance Criteria

Editor AI MVP is acceptable when:

- read-only questions work without write approval;
- selected entity context can be attached and summarized;
- a small generated Aster file can be previewed, approved, written, and validated;
- a selected component field can be previewed, approved, changed, and undone where supported;
- denied operations do not execute;
- unsupported operations are refused with a clear message;
- trace entries appear for executed operations;
- failed validation is visible to the user;
- the UI can promote a local conversation into a Quest intent.

## Test Requirements

Required tests:

- operation normalization from model output;
- permission classification for representative read/write/command operations;
- path rejection for parent traversal and absolute paths where unsupported;
- denied operation does not execute;
- approved write executes once;
- trace entry is recorded for success and failure;
- generated Aster examples pass relevant validators;
- prompt construction includes relevant examples without including unrelated bulk context.

## Open Questions

- Which existing `AiPanel` and `CopilotPanel` behavior should survive the UI consolidation?
- Which operation types are safe enough for session-level approval?
- What should be the default threshold for recommending Quest?
- Which validators must run synchronously before reporting success?
- How should command allowlists be exposed without implying enterprise safety?
