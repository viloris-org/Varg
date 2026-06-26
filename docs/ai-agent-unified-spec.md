# Aster AI and Agent Unified Specification

Status: Draft canonical specification
Last updated: 2026-06-21

## Purpose

This document is the canonical product and engineering specification for Aster's AI-assisted game creation workflows. It replaces the overlapping mode definitions in `docs/ai-editor-copilot-prd.md` and `docs/ai-editor-quest-prd.md`.

## Document Map

Use the AI documentation set this way:

- `docs/ai-agent-unified-spec.md` is the **authority** for product direction, naming, safety commitments, execution layers, prompt strategy, and rollout order.
- `docs/ai-editor-copilot-prd.md` is the detailed **Editor AI** specification: local chat, scoped edits, operation preview, approval, tool execution, diagnostics, and undo.
- `docs/ai-editor-quest-prd.md` is the detailed **Quest** specification: persistent task state, intent/spec artifacts, timeline, draft workspace direction, review, apply, and recovery.
- `docs/varg-language-family-spec.md` is the detailed **Varg language family** authority.

When these documents disagree, resolve in this order:

1. Current implemented behavior and tests.
2. This unified specification.
3. The relevant detailed sub-spec.
4. Historical PRD text, ADRs, or exploratory proposals.

The detailed sub-specs must not introduce stronger safety, autonomy, or product claims than this unified specification allows.

Aster's AI direction is:

- **Quest-led:** persistent AI work is the primary differentiator.
- **Editor-assisted:** direct editor work remains available for inspection, local correction, and small scoped AI help.
- **Example-rich for new languages:** Aster-specific languages need dense, task-relevant examples because models do not already know them.
- **Honest about safety:** the product must only promise the isolation and enforcement it actually implements.

This document intentionally separates current product commitments from future architecture. Future safety mechanisms may guide design, but they are not current guarantees until implemented and tested.

## Product Goal

Aster's AI goal is to let creators turn intent into inspectable game artifacts faster than a manual-first editor, while keeping the user able to understand, reject, revise, and recover from AI output.

The target experience:

- A creator can describe a game object, behavior, scene, mechanic, or fix in natural language.
- Aster gathers relevant project context and Aster language examples.
- The AI produces structured, previewable work rather than opaque prose.
- The user can inspect generated files, scene changes, diagnostics, and validation results.
- Small work happens locally in Editor AI.
- Durable or broad work becomes a Quest.
- The active project is not presented as "safe because AI said so"; it is protected only by implemented controls.

The business direction:

- Do not compete with Unity, Godot, or VS Code on manual feature depth.
- Compete on AI-native game creation, Aster-specific languages, validation loops, and persistent Quest workflows.
- Use frontier-capable models as the primary design target.
- Avoid weak-model UX compromises that add ceremony without improving output quality.
- Be explicit about current safety limits so commercial claims stay defensible.

## Decision Principles

When product, implementation, and safety goals conflict, use this order:

1. **Do not overpromise safety.** If the code does not enforce it, the product must not claim it.
2. **Protect inspectability.** Users must see what changed, why, what evidence exists, and how to reject or recover.
3. **Favor Quest for durable work.** If work needs persistence, broad context, validation, review, or restart recovery, route it to Quest.
4. **Favor Editor AI for local work.** If work is narrow, reversible, and tied to current selection or file, keep it in Editor AI.
5. **Teach Aster languages with examples.** Do not starve the model of syntax examples to make prompts look short.
6. **Move authority into code over time.** Prompt rules can guide model behavior, but real permissions, validation, and apply rules must live in trusted implementation.
7. **Hide internal choreography by default.** Users need outcome, evidence, and decisions, not raw agent routing.

## Product Model

Aster exposes two user-facing AI work surfaces.

### Editor AI

Editor AI is temporary, local assistance inside the editor. The user remains the driver.

Use Editor AI for:

- explaining the current scene, file, selection, component, asset, diagnostic, or command result;
- generating or modifying a small script, behavior, model declaration, scene fragment, or component value;
- applying a narrow edit that is easy to preview and undo;
- running a short local AI task tied to current editor context;
- inspecting and manually correcting Quest output.

Editor AI should feel immediate. It may propose a plan, show operations, ask for approval, execute approved changes, and record trace entries. It should not become the durable container for long-running work.

### Quest

Quest is a persistent AI task state for game-making outcomes that need durable intent, autonomous execution, evidence, review, branching, validation, or recovery.

Use Quest for:

- broad feature work;
- multi-file or multi-artifact changes;
- scene generation with assets and scripts;
- investigation or diagnosis that may branch into fixes;
- refactors or engine/editor changes;
- work that should survive editor restart;
- work that should run unattended until it produces a validated result, a policy decision, or a blocker.

A Quest may contain:

- title, goal, status, intent, and optional editable spec;
- execution timeline;
- changed files and generated artifacts;
- validation output and diagnostics;
- unresolved issues and quick-fix actions;
- review evidence and apply decisions.

Quest workflow is adaptive, not a fixed wizard. The orchestrator may clarify, specify, plan, inspect, execute, validate, repair, ask for manual intervention, prepare a review, or apply through policy depending on the task.

### Execution Styles And Profiles

Single-agent and multi-agent execution are Quest execution styles and implementation profiles, not separate product surfaces. Users choose **Quest** as the durable work surface. Quest may expose execution style as a meaningful productivity and cost control when it is useful.

- **Interactive profile:** one agent handles a small scoped request with visible operations and user approval.
- **Solo profile:** one autonomous agent owns the task loop: inspect, plan, edit in the allowed workspace, validate, repair, and prepare or apply the result according to policy.
- **Extra profile:** an agent cluster profile where a Manager decomposes work, Workers handle bounded slices, and Reviewers inspect or challenge the integrated result before policy apply.

Solo and Extra are commercial Quest capabilities. They should mean "the agent system can do the work," not "the user must drive every step." The boundary is implemented policy: humans define goals and policy, while agents work autonomously until a policy boundary, missing credential, ambiguity, blocker, or review route requires escalation.

## Current Safety Commitments

The MVP safety promise is **controlled AI editing**, not zero-trust enterprise isolation.

For the current product, Aster may promise:

- AI writes are shown as planned operations before execution when practical.
- Write operations require explicit user approval, session-level approval, configured apply policy approval, or an equivalent visible decision.
- Operations are routed through known editor or agent tools where possible.
- File paths are checked to reduce accidental writes outside intended project areas.
- Key editor mutations should be undoable or recoverable where the editor supports it.
- AI operations produce trace entries and diagnostics useful for inspection.
- Model output is treated as fallible draft work and can be rejected.
- Unsupported operations should be refused instead of simulated through prose.

For the current product, Aster must not promise:

- complete sandbox isolation;
- seccomp or OS-level process isolation;
- signed capability grants enforced on every tool call;
- a separate Policy Daemon;
- deterministic prevention of every prompt injection path;
- no direct mutation of active project state before policy apply unless that is actually enforced by the active execution path;
- fail-closed behavior for every uncertainty;
- enterprise-grade zero-trust automation.

User-facing copy should present AI edits as autonomous draft work that remains subject to implemented policy, validation evidence, audit, rollback, and review routes. It must not imply that every Quest requires a human click before progress; it must also not imply automatic active-project mutation unless the implemented apply policy permits it.

## Safety Roadmap

Safety evolves in layers. Each layer becomes a product commitment only after it is implemented, tested, and used by the relevant execution path.

### Layer 1: Controlled Active-Project Editing

This is the MVP layer.

Capabilities:

- plan preview;
- approval and apply-policy controls;
- project-relative path checks;
- command allowlists where available;
- command audit for agent-requested external process execution;
- undo or recovery hints;
- trace and diagnostics.

Limitations:

- this is not a hard security boundary;
- bugs in path checks, tools, provider code, or editor mutation logic may still cause damage;
- users or organization policy must decide which AI edits can apply automatically.

### Agent Command Authorization

Agent-requested external commands are split into two execution zones.

1. **Sandbox commands.**

   Commands may run without per-command user approval when all of these are true:

   - cwd is inside the active Quest workspace or another explicitly sandboxed task workspace;
   - argv is structured, not an arbitrary shell string;
   - network use matches the sandbox policy;
   - writes are limited to the sandbox workspace and approved build/cache outputs;
   - the command is not classified as destructive or privileged.

   Sandbox command execution must still be audited. "No approval prompt" does not mean "no record."

2. **Outside-sandbox commands.**

   Commands whose cwd, paths, network behavior, or side effects escape the sandbox require a matching allowlist rule or an explicit user/organization decision before execution.

   Allowlist rules should use Codex-style argv prefix matching as the MVP shape:

   ```text
   prefix_rule(pattern=["cargo", "check"], decision="allow")
   prefix_rule(pattern=["git", "ls-remote"], decision="allow")
   ```

   Aster rules should additionally record scope and constraints: once/session/permanent, cwd scope, network permission, write scope, risk, creator, reason, and last-used audit metadata.

Destructive commands are not allowed merely because they are inside the sandbox. Deletion, recursive deletion, force-clean, reset, prune, privileged container execution, shell interpreters, and arbitrary code execution forms require explicit policy handling. Sandbox deletion support, if added later, must be scoped to generated build/cache outputs or reviewed transaction groups.

Agents may request elevation when current policy blocks progress. An elevation request is a structured capability request, not an authorization. It must include:

- requested command or operation as structured data;
- current sandbox, cwd, and affected paths;
- requested scope: once, session, Quest, project, or permanent;
- requested capabilities: outside-sandbox execution, network, write scope, deletion, dependency install, container execution, credential access, or active-project apply;
- reason the task cannot continue under current policy;
- expected outputs and rollback or recovery plan;
- risk classification and proposed audit artifacts.

The execution gate routes elevation requests to user or organization policy. Allowed elevation creates a temporary or persistent rule with bounded scope. Denied elevation should produce a blocked or needs-review result with evidence and a lower-privilege fallback when possible.

Command policy is trusted implementation. The model may request a command but must not authorize it.

### Layer 2: Isolated Draft Workspace

This is the next major safety target for broad Quest work.

Capabilities:

- create a task workspace or staging area separate from the active project;
- run broad edits in the draft workspace;
- show diffs, generated artifacts, diagnostics, and validation results;
- discard, review, or apply the draft;
- apply validated results through editor transactions, organization policy, or an equivalent apply path.

Commitment once implemented:

- broad Quest writes should not directly modify the active project before policy apply.

Limitations:

- isolation may be filesystem/workspace-level, not OS sandbox-level;
- validation reduces risk but does not prove semantic correctness;
- active-project apply remains a trusted editor operation and must be tested independently;
- low-risk automatic apply is allowed only when implemented policy and validation evidence permit it.

### Layer 3: Auditable Task Authorization

This is a future hardening layer.

Capabilities:

- structured task scope;
- capability requests and decisions;
- grants bound to task, workspace, tools, paths, operation types, and expiry;
- apply and review routes based on risk;
- evidence contracts for validation and review.

Commitment once implemented:

- agents can only call tools covered by active grants in supported execution paths;
- policy may automatically apply low-risk validated Quest outputs and escalate medium/high-risk outputs.

Limitations:

- this is still not sufficient for enterprise zero trust without independent sandboxing and apply verification.

### Layer 4: Enterprise Isolation And Policy

This is future architecture, not a current product promise.

Possible capabilities:

- separate Policy Daemon;
- signed grants;
- isolated MCP subprocesses;
- OS-level sandboxing where supported;
- organization policy approval;
- stronger bundle verification;
- audit export.

These mechanisms should be documented as future design targets until they exist.

## Authority Model

The current authority model is practical and explicit.

Trusted implementation:

- compiled editor code;
- registered editor commands;
- validators;
- path checks;
- transaction and undo code;
- execution gate logic once introduced.

Untrusted inputs:

- user prompts;
- model output;
- project files and comments;
- third-party assets;
- generated scripts;
- plugin or skill text;
- AI summaries and plans.

Rules:

- Model output proposes work; it does not authorize work.
- User approval or organization policy authorizes proceeding within the current product's implemented controls; it does not prove correctness.
- Validators provide evidence; they do not prove the user's intent was satisfied.
- AI review can help find issues; it is not an authorization root.
- Future grant systems must be enforced by code, not by prompt instructions.

## Aster Language Family

Aster uses a language family because game creation needs several different authoring and interchange surfaces. Some are AI-first, some are shared by AI and humans, and runtime scripting is human-first AI-assisted. General-purpose imperative code is fragile for model generation, so AI should prefer declarative languages when they can express the task.

Canonical language family:

| Extension | Name | Audience class | Purpose | Turing complete |
| --- | --- | --- | --- | --- |
| `.varg` | Varg Logic | human-first AI-assisted | scripts, reusable modules, runtime logic, and declarative behaviors | Yes for `script` and `module`; no for `behavior` blocks |
| `.vscene` | Varg World | AI-first human-readable | scenes, prefabs, entity composition, layout intent, and network replication declarations | No |
| `.vasset` | Varg Asset | AI-first human-readable | models, materials, audio events, shader parameters, and primitive resource recipes | No |

Rules:

- Prefer `.vscene` and `.vasset` for generated content.
- Use executable `.varg` scripts only when runtime computation is actually needed.
- Keep declarative languages statically checkable.
- Treat JSON as interchange or generated artifact format, not the primary authoring format unless a specific tool path requires it.
- Keep `docs/varg-language-family-spec.md` as the detailed language-family authority.
- Each language spec must declare its audience class and use the corresponding documentation profile.

## Prompt And Example Strategy

Aster should not minimize prompts at the cost of generation quality. Models do not have strong prior knowledge of Aster's new languages.

The rule is:

- keep **policy and workflow prompts** compact;
- keep **language examples** dense, short, and task-relevant.

Do not use long prompts to carry:

- permission rules;
- safety guarantees;
- workflow state;
- product mode definitions;
- repeated warnings;
- tool contracts that should live in schemas.

Do use examples to teach:

- `.varg` script and authoring idioms;
- `.vscene` scene composition;
- `.vasset` asset/material declarations;
- common diagnostics and fixes;
- project-specific conventions.

### Example Bank

Aster should maintain a structured Example Bank.

Example organization:

- by language: `.varg`, `.vscene`, `.vasset`;
- by task: player controller, patrol enemy, camera setup, light setup, audio source, destructible object, UI rule;
- by concept: transforms, references, colliders, rigidbodies, materials, selectors, conditions, custom hooks;
- by diagnostic: invalid syntax, missing asset, wrong field, unsupported action, unsafe script pattern.

Each example should be small:

- one concept per example;
- usually 10-40 lines;
- no giant kitchen-sink examples as default context;
- include only examples relevant to the user's task.

High-value example shape:

```text
Task: Create a patrol behavior
Good example: ...
Common mistake: ...
Validator diagnostic: ...
Fixed example: ...
```

The model prompt should say:

```text
Follow the provided Aster examples as authoritative syntax and style.
Do not invent syntax, fields, actions, hooks, or helper functions not shown in examples, schemas, or tool manifests.
When unsure, generate the simplest statically checkable form and rely on diagnostics.
```

### Retrieval

The orchestrator should retrieve examples by:

- requested language;
- target artifact type;
- selected entity/component context;
- diagnostics;
- existing project conventions;
- prior failed generation attempt.

The goal is not a short total context. The goal is high-signal context: compact policy plus enough relevant examples for the model to write valid Aster files.

## Execution Flow

### Editor AI Flow

1. User asks a local question or requests a small scoped edit.
2. System gathers selected editor context.
3. Model produces an answer or structured operations.
4. Operations are normalized and previewed.
5. Read-only operations may run with low friction.
6. Write operations require user approval or session-level approval.
7. Approved operations execute through current implemented controls.
8. Diagnostics, trace entries, and undo/recovery hints are shown.

Editor AI should suggest creating or promoting to a Quest when the task becomes broad, durable, risky, or multi-artifact.

### Quest Flow

1. User creates a Quest from a prompt, promoted Editor AI conversation, selected artifact, issue, or spec.
2. Quest captures durable intent.
3. The orchestrator decides whether to clarify, write a spec, inspect first, or run a fast path.
4. Execution uses the safest implemented profile available.
5. Timeline records meaningful events.
6. Validation and diagnostics attach to the Quest.
7. The result becomes a reviewable artifact: diff, changed files, generated assets, investigation report, blocked report, or transaction groups.
8. Apply policy classifies the result as auto-apply, needs human review, needs revision, blocked, or reject.
9. Auto-apply mutates the active project only through the implemented apply path and only when policy permits it.
10. Human review remains available for approval, rejection, revision, partial acceptance, quick-fix, archive, or reopen decisions.

## Review And Validation

Validation should be deterministic when possible:

- syntax parse;
- schema validation;
- asset reference checks;
- language diagnostics;
- targeted tests;
- command registry checks;
- scene load checks;
- script diagnostics.

AI review may be used for:

- semantic fit to the user's intent;
- code and artifact quality;
- consistency with project conventions;
- risk triage;
- explanation of validation output.

AI review must not be described as proof of safety.

Review surfaces should answer:

- What changed?
- Why did it change?
- Which files, scenes, assets, or entities are affected?
- What validation ran?
- What diagnostics remain?
- What risks remain?
- What policy decision was made?
- What can be auto-applied, manually applied, revised, discarded, or fixed?

## UX Requirements

### Global AI Navigation

The editor should expose:

- Quests;
- Editor AI chats or local sessions;
- Knowledge;
- provider/settings;
- future Marketplace entry only when there is a real extension story.

### Editor Workspace

Editor Mode should provide enough manual control to inspect and fix AI output:

- Scene/Game View;
- Hierarchy;
- Inspector;
- Project/Assets;
- Script/Behavior editor;
- Console/Diagnostics;
- play or validation controls;
- AI panel anchored to selected context.

The editor does not need to beat Unity, Godot, or VS Code on manual depth. It must be good enough to inspect, adjust, and trust AI-generated game changes.

### Quest Workspace

Quest should provide:

- Quest registry;
- title, status, project, and workspace identity where relevant;
- execution style: Solo or Extra where exposed;
- intent/spec editor;
- timeline;
- artifacts and changed files;
- validation and diagnostics;
- unresolved issues;
- apply policy, review, and decision controls;
- open-in-editor actions.

Internal agent complexity should be collapsible. Users should see outcome, evidence, and decisions by default, not raw agent choreography.

## Knowledge And Memory

Knowledge is separate from transient chat and separate from task-local Quest assumptions.

Knowledge may include:

- project conventions;
- accepted architecture notes;
- recurring user preferences;
- known caveats;
- stable language or asset patterns.

Rules:

- AI-generated knowledge starts as proposed unless directly derived from trusted project docs.
- Users can inspect, approve, reject, edit, or delete knowledge.
- Quest-local assumptions must not silently become project knowledge.
- Knowledge included in prompts should be labeled as project memory, not policy.

## Implementation Direction

### Near-Term Refactor Targets

1. Create a single `AgentExecutionGate` module.

   It should own operation normalization, permission classification, path checks, command authorization, execution routing, trace recording, and result shaping for the current MVP layer.

2. Make Editor AI and Quest use the same execution gate.

   Different UI adapters are fine. Different authority paths are not.

3. Consolidate AI review UI.

   Plan preview, operation groups, diagnostics, policy apply, approval, rejection, undo, and trace should share one decision model.

4. Create `SoloQuestRunner`.

   It should run the single-agent Quest loop: inspect, plan, execute in workspace, validate, repair within limits, produce review evidence, and invoke apply policy.

5. Create `QuestApplyPolicy`.

   It should classify Quest results into auto-apply, needs review, blocked, or rejected based on risk, changed paths, validation evidence, configured autonomy, and organization rules.

6. Create `AgentCommandPolicy`.

   It should distinguish sandbox commands from outside-sandbox commands, apply prefix allowlist rules, reject destructive commands by default, handle structured elevation requests, and emit command audit evidence for Quest and Editor AI.

7. Keep `engine-agent-cluster` aligned with the Extra profile.

   Extra is the Agent cluster execution style: Manager, Workers, Reviewers, integration, validation, and policy apply.

8. Move language examples into an Example Bank.

   Keep `system_prompt_base.txt` focused on role, output protocol, tool use, and retrieved examples.

9. Mark enterprise safety mechanisms as future until implemented.

   Keep `engine-policy` and `engine-agent-cluster` useful as contracts and prototypes, but do not present them as current product guarantees.

### Naming

Use these names consistently:

- **Editor AI**: local temporary assistant surface.
- **Quest**: persistent AI task state.
- **Execution style**: user-meaningful Quest strategy such as Solo or Extra.
- **Execution profile**: implementation strategy used to perform work.
- **Solo**: single-agent autonomous Quest execution style.
- **Extra**: Agent cluster Quest execution style.
- **Execution gate**: trusted code module that checks and runs operations.
- **Command policy**: trusted code module that decides whether an external command can run without approval, needs allowlist approval, or must be denied.
- **Draft workspace**: isolated or staged work area when implemented.
- **Review artifact**: diff, diagnostic, validation output, generated file, issue, or report used for decision-making.

Avoid using these as primary product modes:

- Copilot Mode;
- Auto Mode;
- Solo Mode;
- Extra Mode.

Solo and Extra may appear as Quest execution styles, plans, or enterprise capability names. They should not replace Quest as the product surface.

## Rollout

### Phase 0: Spec Cleanup

- Make this document canonical.
- Add superseded notices to older AI PRDs.
- Convert old zero-trust language into future-architecture notes.
- Create initial Example Bank structure.

### Phase 1: Editor AI Reality Alignment

- Ensure current AI tools match documented MVP commitments.
- Centralize path checks, permission classification, operation preview, apply, trace, and undo result shaping.
- Remove UI claims that imply stronger isolation than exists.

### Phase 2: Quest Shell

- Durable Quest records.
- Intent/spec artifacts.
- Timeline.
- Review surface.
- Open-in-editor flow.
- Stub or controlled execution using the shared execution gate.

### Phase 3: Solo Quest Autonomy

- Single-agent Quest runner.
- Workspace-only writes for broad Quest work.
- Sandbox command execution with audit.
- Outside-sandbox command allowlist and approval flow.
- Validation and bounded repair loop.
- Review evidence and blocked reports.
- Policy classification before apply.

### Phase 4: Draft Workspace Apply Policy

- Stage broad Quest edits outside active project.
- Present diffs and validation before apply.
- Support discard, reviewed apply, and low-risk policy auto-apply.
- Preserve rollback evidence for automatic apply.

### Phase 5: Extra Agent Cluster

- Manager/Worker/Reviewer orchestration.
- Parallel bounded task execution.
- Integration review and validation.
- Same apply policy as Solo.

### Phase 6: Capability And Authorization Hardening

- Introduce structured grants only where the tool layer enforces them.
- Add risk routing.
- Add stronger validation evidence.
- Consider separate policy process only after the in-process gate is correct and tested.

### Phase 7: Enterprise Policy

- Organization policy;
- signed grants;
- stronger sandboxing;
- MCP isolation;
- audit export;
- administrative controls.

## Non-Goals

Current non-goals:

- claiming full zero-trust security;
- claiming OS-level isolation;
- supporting arbitrary unattended active-project mutation;
- building compatibility flows for weak models at the cost of the main experience;
- hiding the need for policy, audit, rollback, and review routes;
- making prompts short by starving the model of Aster language examples;
- treating AI review as proof;
- duplicating product modes for every execution strategy.

## Open Questions

- What is the minimum `AgentExecutionGate` interface for current Editor AI?
- Which operations must be blocked until draft workspace support exists?
- Which current UI copy over-promises safety?
- What is the first Example Bank file layout?
- Which validators are required before Quest broad-write MVP?
- Should `.aster` remain supported as a legacy extension, or should `.as` become canonical immediately?
- What should be the threshold for promoting Editor AI work into a Quest?
