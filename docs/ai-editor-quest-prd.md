# Aster AI Editor and Quest Modes PRD

状态：Draft
目标版本：分阶段交付
最后更新：2026-06-20

## Problem Statement

Aster already has a detailed Copilot architecture proposal in `docs/ai-editor-copilot-prd.md`, including agent tooling, isolated task workspaces, policy, capability grants, validation, review, and traceability. What is still missing is a product-level interaction model for an AI-native editor where both direct editing and Quest work are AI-enabled, but differ in duration, persistence, autonomy, and review depth:

- **Editor AI:** temporary, local, small-task execution where the user is actively navigating files, scenes, assets, and selections while AI assists, modifies, diagnoses, or performs short SOLO runs nearby.
- **Quest:** long-horizon autonomous SOLO work where the user describes an outcome, AI controls the workflow, and the system records enough state and evidence to inspect, validate, revise, apply, or abandon the result safely.

Treating both workflows as one chat panel creates confusing authority boundaries. Sometimes the user wants a lightweight assistant beside the current file or scene. Other times the user wants a persistent AI application state: a recoverable Quest that may contain intent, generated specs, plans, branches, experiments, validation, artifacts, partial results, manual interventions, and final apply decisions. Aster needs these modes to be distinct at the product level while sharing the same underlying agent safety infrastructure.

The product position is AI-first and frontier-model-first. Aster should not try to win by outbuilding mature manual editors such as Unity, Godot, or VS Code, and should not dilute the UX or architecture with compatibility layers for weak models. Its differentiated surface is Quest Mode: an AI-native long-horizon SOLO application for turning intent into inspectable game changes. Quest must not be reduced to a locked, linear wizard. Flow control belongs to the AI orchestrator, bounded by policy, evidence, and user approval gates. Different tasks need different degrees of specification, exploration, planning, validation, user steering, and manual intervention. Editor Mode still matters as the direct manipulation surface where users inspect project state, make precise adjustments, and ask AI to execute temporary local tasks. The editor only needs enough manual game-making affordance to keep users in control and make Quest output inspectable; it does not need to compete as a full manual-first editor.

## Solution

Introduce two connected work surfaces in Aster: **Quest Mode** and **Editor Mode**.

**Quest Mode** is the primary AI-first product surface and the path toward ultra-long-horizon autonomous SOLO work. The primary object is a Quest: a named, persistent AI application state for a game-making outcome. A Quest can contain a goal, evolving spec, hypotheses, plan, execution timeline, progress slices, artifacts, validation evidence, review result, diff bundle, unresolved issues, manual intervention notes, and final apply controls. The user sets intent, constraints, and acceptance decisions. The AI orchestrator owns the workflow: it decides when to clarify, specify, plan, explore, branch, execute, validate, repair, ask for manual intervention, or present review, subject to safety policy and explicit final apply. Quest is for work that benefits from persistence, long-running autonomy, recoverability, and auditability.

**Editor Mode** is the supporting inspect-and-intervene surface and the home for temporary AI execution. The user remains the primary driver while inspecting Quest output, editing files or scene artifacts, manipulating selected entities, adjusting components, running diagnostics, and asking AI for localized help. Copilot belongs here: it is local explanation, scoped modification, diagnosis, and short-task execution. It may still run in a SOLO style for a bounded temporary task, but it should not become the durable long-horizon task container. AI may explain, propose, apply small scoped edits, generate snippets, run safe tools, or complete a short local task with explicit scope and approval. Context is anchored to the current editor state: selected entity, open scene, active file, selected text, diagnostics, recent commands, visible assets, and active editor tool.

The two modes should feel like a dual-interface AI-first product, not competing panels. Quest Mode is where game-making intent becomes tracked AI work and durable task state. Editor Mode is where users inspect, verify, and precisely adjust project state with direct AI assistance. A user can promote an Editor conversation into a Quest when the work needs persistence, evidence, branching, or review. A user can open Quest files, specs, diffs, scene changes, diagnostics, and review artifacts in the Editor to inspect or manually adjust the result. Both modes share project knowledge, model providers, tool permissions, traces, diagnostics, and memory, but they differ in who is driving the workflow and how much persistent task state and review evidence the work needs.

## Product Principles

- **Frontier-model-first.** Aster is designed for top-tier reasoning and coding models. Do not introduce UX, prompts, orchestration, or degraded fallback paths whose main purpose is to make weak models appear capable.
- **Maximize model capability extraction.** The core product goal is to extract the highest practical capability from frontier models. Design choices should increase the model's effective agency, context quality, tool reach, feedback speed, exploration bandwidth, and ability to recover from mistakes. Constraints should protect project state without unnecessarily reducing the model's problem-solving ceiling.
- **Strong framework, strong constraints.** Do not rely on loose chat conventions, best-effort prompts, untyped tool calls, informal task memory, or weak approval language for Quest execution. Frontier models should operate inside strong product primitives: explicit state, typed artifacts, scoped capabilities, durable traces, validation evidence, reviewable transaction bundles, and clear authority boundaries.
- **Minimal prompt burden, maximal tool leverage.** Avoid long, repetitive prompt scaffolds that try to teach the model the product every turn. Put durable knowledge, project context, schemas, commands, validators, asset operations, editor operations, and review artifacts into explicit tools and structured context packets.
- **Isolation over behavioral restriction.** Do not make safety depend on telling the model not to run commands or not to touch files. Give the model powerful tools inside isolated workspaces, snapshots, sandboxes, and scoped credentials, then validate, review, and transactionally apply only approved results to the active project.
- **AI-first, Quest-led.** Aster's primary differentiator is AI-native game creation through recoverable, long-horizon Quests, not parity with manual-first editors.
- **Mode reflects duration and persistence.** Quest Mode is persistent ultra-long-horizon SOLO; Editor Mode is temporary local AI execution and direct intervention.
- **AI owns workflow inside Quest.** Product surfaces expose state, evidence, controls, and approval gates; they should not hard-code a universal step-by-step process.
- **Editor is necessary but not the moat.** Editor Mode must support inspection, validation, precise manual fixes, and minimal game-making operations, but it should not drive the roadmap ahead of Quest.
- **A Quest is an AI application state, not a wizard.** It is a recoverable workspace with intent, evolving context, progress model, execution state, artifacts, evidence, and decisions.
- **Flow is adaptive.** Quests should support fast-path execution, spec-first work, exploratory investigation, paused/manual intervention, revision loops, and partial acceptance rather than forcing every task through the same linear sequence.
- **Specs are visible artifacts when useful.** Non-trivial write work should have a concise Markdown spec that can be reviewed and edited; simple, reversible, or investigative Quests may start from a smaller intent record and grow a spec only when needed.
- **Progress is auditable.** File reads, file edits, command runs, validation results, checkpoints, and reviews should appear in a timeline.
- **Final apply is explicit.** Quest output becomes a reviewed bundle that the user can approve, reject, partially accept, or revise.
- **The active project remains protected.** AI-led write work happens through isolated workspaces and reviewed transaction bundles, consistent with the Copilot PRD.
- **Internal complexity stays collapsible.** Users should see clear progress by default and expand details only when needed.

## Mode Definitions

### Editor Mode

Editor Mode is the direct manipulation surface with embedded AI assistance. It must be good enough to inspect, verify, and precisely adjust AI-generated game changes, while avoiding a roadmap that tries to match mature manual-first editors feature-for-feature. It is still AI-enabled: the distinction is that the user drives the current editor state directly, and AI acts against explicit local context with previewable, undoable operations.

Primary user intent:

- Inspect Quest-generated scene, asset, script, behavior, and component changes.
- Open Quest specs, diffs, diagnostics, review findings, and changed files in context.
- Make precise manual corrections when AI output is close but needs human judgment.
- Create or adjust simple objects, components, scripts, behaviors, materials, and assets when this is faster than starting a new Quest.
- Delegate temporary local work to AI without creating a durable Quest when the task is small, bounded, and easy to inspect.
- Manipulate selected entities in a scene or game view with basic transform, selection, and camera controls.
- Enter play mode or run checks to validate Quest output.
- Ask questions about the current scene, file, selected entity, component, asset, script, or diagnostic.
- Request small changes such as adding a component, fixing a script error, renaming a symbol, or generating a behavior snippet.
- Use AI while continuing to manually edit files and manipulate editor state.
- Keep tight control over each change.

Behavior:

- Context defaults to the active editor surface and explicit user selections.
- Manual editor actions are available for inspection and precision fixes, but persistent, broad, or long-running creation work should naturally route to Quest.
- The AI response can include explanations, suggested edits, inline patches, commands, or previewable operations.
- Write operations are scoped, previewable, undoable, and traceable. Editor AI should optimize for immediate local outcome, not durable task management.
- The user can directly edit while the AI panel remains open.
- The system can suggest promoting the session to a Quest when the request becomes persistent, cross-system, risky, or long-running.

Suitable tasks:

- Inspect and adjust a Quest-generated camera, light, player object, or scene layout.
- Add an `AudioSource`, collider, rigidbody, script, or behavior to a selected entity.
- Tune transform, rendering, physics, script, and audio component fields in the inspector.
- Create a material, prefab, script, behavior file, or scene from the Project panel.
- Explain why a Rust compile error occurs.
- Add a missing field to a selected component.
- Generate a small Rhai script.
- Refactor one function or one file.
- Inspect a scene object and summarize its dependencies.
- Run a targeted check and explain the result.
- Execute a short SOLO task such as "fix this diagnostic in the current file" or "wire this selected entity to the existing script."

### Quest Mode

Quest Mode is a persistent AI-native task application for outcomes that need durable context, progress, artifacts, evidence, branching, or review. It is not only a longer chat and not a fixed process funnel.

Primary user intent:

- Turn a feature, bug, refactor, content task, or investigation into a tracked unit of work.
- Let AI plan, explore, execute, validate, and revise without requiring per-operation supervision when the task and policy allow it.
- Review a complete result with diffs, validation evidence, and risk summary.
- Pause, resume, branch, revise, archive, or reopen task work.
- Move between lightweight intent capture, spec-first execution, investigation, implementation, manual intervention, and review as the task evolves.
- Delegate workflow control to the AI orchestrator while retaining explicit user control over intent, constraints, approvals, and final apply.

Behavior:

- Each Quest has a title, goal, status, intent record, timeline, artifacts, validation results, review state, and final decision. It may also have a spec document, plan, workspace, branches, experiments, checkpoints, and manual intervention records.
- The Quest starts by capturing intent. The AI orchestrator decides whether to ask clarifying questions, produce a full spec, start with a lighter brief, investigate first, prototype alternatives, or prepare execution.
- The user can edit the brief or spec at any time before execution starts, and can revise it during execution when the Quest is paused, blocked, or intentionally replanned.
- Execution occurs inside the isolated task workspace described by the Copilot PRD.
- The timeline records meaningful events: intent changes, clarifications, planning, file reads, edits, command runs, checks, validation failures, repairs, manual interventions, reviews, and user decisions.
- The final review surface summarizes the diff bundle, validation evidence, unresolved issues, and apply options.

Suitable tasks:

- Implement an audio subsystem phase.
- Add a new editor panel and backend command path.
- Diagnose and fix a multi-crate regression.
- Generate a playable sample scene with assets and scripts.
- Perform a broad but bounded refactor.
- Convert a design proposal into implementation slices.

## Information Architecture

### Global AI Sidebar

The editor should expose a persistent AI entry point with these sections:

- **Quests:** active, paused, completed, blocked, and archived tasks.
- **Chats:** lightweight Editor Mode conversations that are not attached to a Quest.
- **Knowledge:** project memory, conventions, architecture notes, generated summaries, and user preferences.
- **Marketplace:** optional future entry for skills, tools, MCP integrations, model profiles, and workflow templates.

The global shell should also expose clear cross-surface navigation:

- **Open Editor:** returns to the main game-making editor surface.
- **Open Quest:** opens the active or selected Quest workspace.
- **Promote to Quest:** converts a growing Editor chat and selected context into a Quest intent record. The orchestrator may then create a spec, investigation plan, prototype branch, or execution plan.
- **Open in Editor:** opens a Quest file, spec, diff, scene artifact, diagnostic, or review finding in the relevant editor surface.

### Quest Workspace

A Quest workspace should include:

- **Quest Header:** title, project name, status, branch or workspace identity, last activity, and open-in-editor action.
- **Agent Run Stream:** the center column and primary live surface, showing the user's goal, plan updates, thought summaries, file reads, file edits, command runs, validation results, blocked issues, review readiness, and user decisions as compact chronological entries.
- **Quest Overview Panel:** the default right column, showing progress slices, artifacts, changed files, validation evidence, references, unresolved issues, risk, and final decision controls.
- **Artifact Viewer:** a right-column detail view for selected artifacts such as the Markdown spec, file diff, validation log, review finding, scene artifact, diagnostic, or generated asset. Opening in Editor is reserved for inspection that needs the full Scene View, Inspector, Project/Assets, or script editor.
- **Input Bar:** prompt box for steering, adding constraints, answering clarifications, pausing, requesting review, or triggering quick-fixes. The input continues the Quest, not a separate chat, and the AI orchestrator decides the next workflow step.

The default Quest layout should be a three-column cockpit:

- **Left:** Quest registry, Chats, Knowledge, and Marketplace navigation.
- **Center:** always-visible Agent Run Stream.
- **Right:** Quest Overview by default, switching to Artifact Viewer when the user selects a spec, changed file, validation, review finding, or reference.

The user should not need to switch away from the run stream to understand the task state. Progress, artifacts, validation, and final decisions stay visible beside the execution flow.

### Editor Workspace

Editor Mode should include:

- **Scene/Game View:** interactive viewport for inspecting and adjusting Quest-generated scene state, with camera controls, selection, transform gizmos, snapping, play preview, and overlays.
- **Hierarchy:** tree of scene objects with selection, focus, rename, duplicate, delete, and basic reparent controls.
- **Inspector:** component list and field editors for selected entities, with add/remove component controls for common components.
- **Project/Assets:** asset browser for scenes, prefabs, materials, scripts, behaviors, textures, audio, import status, and reload/open actions.
- **Script/Behavior Editing:** file editor or dedicated behavior authoring surface for inspecting and fixing gameplay logic.
- **Console/Diagnostics:** build, runtime, import, validation, script, and AI operation diagnostics.
- **Command Palette/Menu Bar:** manual access to core editor commands and project actions.
- **Context Controls:** current file, selected text, selected entity, scene, assets, diagnostics, and optional manual attachments.
- **Chat Panel:** conversational AI surface for explanations and localized changes.
- **Edit Preview:** inline or side-by-side preview for proposed modifications.
- **Command Result Cards:** compact cards for checks, builds, tests, editor commands, and tool outputs.
- **Promote to Quest:** action that converts the current conversation and selected context into a Quest intent record.

## Quest State Model

Quest should be modeled as a flexible state machine, not a required linear lifecycle. The AI orchestrator selects the next workflow step from the current goal, risk, available evidence, policy, validation state, and user constraints. The product must expose state and controls, not force every Quest through the same sequence. A content-generation Quest, a bug investigation, a broad refactor, and a scene-generation Quest need different paths.

Canonical states:

- **Draft:** intent has been captured from a prompt, template, issue, selected editor context, imported spec, or promoted Editor conversation.
- **Clarifying:** AI asks targeted questions or proposes assumptions because the goal, constraints, or acceptance criteria are underspecified.
- **Specified:** the Quest has enough written intent to proceed. This may be a brief for low-risk work or a full Markdown spec for non-trivial write work.
- **Planning:** AI proposes slices, strategy, affected areas, validation, and risk controls. Planning may be skipped for small reversible work or repeated during replanning.
- **Prepared:** the system has captured the necessary snapshot, workspace, permissions, context packet, and policy state for the next execution segment.
- **Running:** AI is reading, editing, generating, diagnosing, validating, or otherwise producing artifacts.
- **Waiting for user:** the Quest needs approval, clarification, credentials, manual Editor intervention, or a product/design decision.
- **Validating:** deterministic checks, targeted tests, schema validation, editor validation, asset validation, or play-preview checks are running.
- **Repairing:** AI is addressing validation failures, review findings, or user-requested revisions within bounded retry policy.
- **Ready for review:** the Quest has a reviewable result, partial result, investigation report, or blocked finding.
- **Applying:** approved changes are entering the active project through reviewed transactions with undo or rollback support.
- **Completed:** the Quest has a final decision record and any accepted work has been applied or intentionally left as an artifact.
- **Blocked:** the Quest cannot proceed without new information, external action, unavailable tools, or a changed constraint.
- **Archived:** the Quest is retained for history but removed from active work.

Common transitions:

- Draft → Clarifying when intent is ambiguous.
- Draft or Clarifying → Specified when enough intent exists.
- Specified → Planning for multi-step, risky, or broad work.
- Specified or Planning → Prepared when execution can start.
- Prepared → Running for an execution segment.
- Running → Validating when an artifact, slice, or final candidate needs evidence.
- Running, Validating, or Ready for review → Waiting for user when human judgment or approval is needed.
- Validating → Repairing when failures are actionable and retry policy allows repair.
- Repairing → Running or Validating after a fix attempt.
- Running or Validating → Ready for review when a result, partial result, or investigation finding is ready.
- Ready for review → Applying when the user accepts all or part of the result.
- Ready for review → Specified or Planning when the user requests revision or changes scope.
- Any active state → Blocked when progress requires unavailable information, permission, tooling, or external state.
- Any active state → Archived when the user dismisses or shelves the Quest.

Common Quest paths the orchestrator may choose:

- **Fast path:** intent → prepare → run → validate → review → apply. Used when the orchestrator determines the task is bounded and low-risk enough.
- **Spec-first path:** intent → spec → plan → run → validate → review → apply. Used when the orchestrator determines the work needs explicit alignment before broad execution.
- **Investigation path:** intent → clarify/plan → run diagnostics → report findings. It may complete without diffs or later branch into implementation.
- **Exploration path:** intent → generate alternatives or prototypes → compare → select → implement or archive. It may create multiple artifacts before any final diff exists.
- **Manual-intervention path:** Quest requests Editor action → user adjusts project or artifact → Quest resumes from the updated evidence.
- **Revision path:** review → user changes goal/spec → replan or repair → review again.
- **Partial-accept path:** review → accept one transaction group → keep remaining groups pending, revised, or discarded.
- **Blocked-result path:** run/validate → blocked report with evidence, attempted actions, and next options.

State requirements:

- The UI must show current state, the orchestrator's chosen next action, available user overrides, and why the Quest is waiting, running, validating, blocked, or ready for review.
- Specs, plans, validation, and reviews are first-class artifacts, but only the artifacts needed for the current Quest path should be required.
- The timeline records state transitions and evidence. It should not imply that hidden phases were executed when they were skipped.
- Final apply remains explicit for write results, regardless of path.
- Durable learnings, decisions, and project conventions can be proposed for Knowledge after completion, blocking, or user-approved investigation findings.

## Functional Requirements

### Editor Mode Requirements

- Provide enough manual controls for scene, hierarchy, inspector, project assets, scripts/behaviors, play mode, and diagnostics to inspect and adjust Quest output.
- Let users make a minimal local correction without starting a new Quest: select object, edit transform, edit common component fields, open script/behavior, save, and run validation or play mode.
- Support common object operations needed for correction and review: create simple object, rename, duplicate, delete, parent, reparent, select, focus, transform, and undo/redo.
- Support common component operations through the Inspector: add, remove, edit fields, reset where applicable, and surface validation errors.
- Support asset operations through the Project panel: open, create simple assets where supported, import/reload assets, and reveal references.
- Show an AI panel inside the editor without blocking manual editing.
- Attach current file, selection, scene, entity, component, asset, diagnostics, and command output as explicit context.
- Let the user add or remove context before sending a request.
- Support read-only explanations without write approval.
- Support small scoped edits with preview, approval, trace, and undo.
- Support temporary Editor AI SOLO runs for small bounded tasks, with clear scope, progress, preview, and undo.
- Use existing editor commands and transaction systems rather than bypassing editor architecture.
- Show command/tool results as compact cards with expandable logs.
- Offer **Promote to Quest** when scope grows beyond localized assistance.

### Quest Mode Requirements

- Create, rename, pause, resume, cancel, archive, and reopen Quests.
- Store each Quest as a durable task record with stable ID, title, status, timestamps, intent path, optional spec path, optional workspace ID, artifact links, and trace links.
- Capture an intent record for every Quest and generate a Markdown spec before broad or non-trivial write execution.
- Allow user edits to the intent brief or spec before execution starts and support explicit revision during pause, block, or replan states.
- Support adaptive Quest paths, including fast-path, spec-first, investigation, exploration, manual-intervention, revision, partial-accept, and blocked-result paths.
- Represent Quest state transitions explicitly instead of deriving status from a fixed numbered flow.
- Let the AI orchestrator choose the next workflow step and explain why, while policy and user approval gates constrain authority.
- Maintain a timeline of meaningful events, not raw token-by-token model output.
- Display changed files with additions/deletions and status.
- Run validation commands through the policy-approved command registry.
- Present review for the relevant result type: diff bundle, generated artifacts, investigation report, alternatives comparison, validation evidence, unresolved issues, quick-fix actions, and apply controls where applicable.
- Support partial acceptance when transaction grouping permits it.
- Preserve blocked outcomes instead of silently hiding them.

### Knowledge Requirements

- Maintain project-level knowledge separate from transient chat history.
- Store durable facts such as architecture decisions, coding conventions, preferred workflows, known caveats, and recurring user preferences.
- Propose memory updates after completed Quests when new reusable knowledge appears.
- De-duplicate, compress, and validate memory references periodically.
- Clearly distinguish trusted project documentation from AI-generated summaries.

### Transition Requirements

- Editor Mode can promote a conversation into a Quest intent record.
- Quest Mode can open files, diffs, diagnostics, and specs in the Editor.
- Quest Mode can open scene artifacts in the Scene View, entity changes in the Inspector, asset changes in Project/Assets, and script changes in the script editor.
- Quest Mode can request user manual edits in Editor Mode when that is safer or faster than agent execution.
- Completed Quest learnings can be proposed for Knowledge.
- Knowledge can be attached as context to both Editor requests and Quest intent/spec artifacts.

## Non-Functional Requirements

- **Responsiveness:** Editor Mode should feel interactive; lightweight read-only questions should not spawn an agent cluster.
- **Manual viability:** Aster must remain usable for core game-making workflows when AI is disabled or unavailable.
- **No weak-model compatibility layer:** The product should target frontier-grade models and should not add separate degraded flows, artificial confirmation spam, rigid templates, or simplified orchestration whose purpose is to compensate for weak model reasoning. Provider abstraction may exist for integration, billing, or deployment, but not as a UX design constraint.
- **No weak framework or weak constraints:** Quest execution should not be built on free-form chat transcripts, implicit permissions, stringly typed operations, hidden state, unverifiable claims, or "trust the model" apply paths. Strong orchestration means explicit state machines, schemas, capability grants, sandbox/workspace boundaries, deterministic validation where possible, trace-linked evidence, and reviewable apply bundles.
- **Prompt efficiency:** System and task prompts should be compact, stable, and non-redundant. Repeated policy prose, project facts, tool instructions, and workflow recipes should move into schemas, tool descriptions, context packets, skill metadata, validators, and persistent Quest state.
- **Tool richness:** Frontier models should receive enough high-quality tools to act effectively: file/search/edit tools, project graph queries, scene/entity/component operations, asset operations, diagnostics, validators, build/test commands, preview generation, diff/review tools, and transaction assembly tools. Missing tools should be treated as product capability gaps, not patched with longer prompts.
- **Execution isolation:** Command execution is expected for Quest work and should happen in isolated workspaces or sandboxes with scoped filesystem, environment, credential, and network access. The safety boundary is where results cross from isolated workspace into active project state, not whether the model can run commands at all.
- **Capability extraction:** Quest should maximize effective model performance by providing high-signal context, low-latency tool feedback, broad isolated execution room, artifact comparison, self-correction loops, and access to relevant project structure. The system should remove friction that only slows the model down without improving safety or review quality.
- **Exploration bandwidth:** The orchestrator should be able to branch, prototype, run experiments, compare alternatives, discard failed attempts, and keep only reviewable artifacts. Exploration should be cheap inside isolation and strict only at the active-project apply boundary.
- **Recoverability:** Quests must survive editor restart and support resume where workspace state is valid.
- **Adaptivity:** Quest workflow must scale down to small reversible tasks and scale up to long-running autonomous work without presenting a fake universal sequence.
- **Traceability:** Every AI write, command run, validation result, review decision, and user approval must be trace-linked.
- **Safety:** No AI-led Quest write should directly mutate the active project before reviewed apply. Safety should rely on isolation, scoped capabilities, validation, review, and transactional apply rather than ordinary natural-language rules.
- **Clarity:** UI copy must distinguish proposed, workspace, reviewed, and applied states.
- **Extensibility:** Modes should support future model providers, skills, MCP tools, and specialized agents without redesigning the UI.
- **Local-first:** Project task metadata should be stored locally unless the user opts into remote sync later.

## User Stories

1. As a developer, I want to ask AI about my current file while I keep editing, so that AI help does not interrupt flow.
2. As a developer, I want AI to use my selected entity as context, so that scene edits target the object I mean.
3. As a developer, I want to preview small AI edits before applying them, so that I can reject incorrect changes quickly.
4. As a game creator, I want to create objects, cameras, lights, scripts, materials, prefabs, and scenes without AI, so that Aster is a usable editor by itself.
5. As a game creator, I want to add and edit components in an Inspector, so that I can hand-author gameplay objects.
6. As a game creator, I want Scene/Game views, Hierarchy, Project/Assets, Console, and Play controls, so that normal game-making workflows are visible and direct.
7. As a developer, I want to promote a growing chat into a Quest, so that complex work becomes tracked instead of buried in conversation.
8. As a developer, I want each Quest to capture intent durably, so that the task is not just transient chat history.
9. As a developer, I want non-trivial write Quests to have an editable spec, so that my intent overrides the model's first interpretation before broad changes start.
10. As a developer, I want to see a Quest timeline, so that I know what AI did and where it is blocked.
11. As a developer, I want changed files summarized with additions and deletions, so that review scope is visible at a glance.
12. As a developer, I want validation results attached to the Quest, so that I can trust the final review more than a prose claim.
13. As a developer, I want unresolved issues to include quick-fix actions, so that I can continue from a partial result.
14. As a developer, I want to open Quest files and scene artifacts in the Editor, so that I can inspect or manually adjust the result.
15. As a developer, I want final apply to be explicit, so that AI work does not silently enter my active project.
16. As an engine maintainer, I want Quests to use stable IDs and workspaces, so that traces, artifacts, and rollback remain reliable.
17. As an engine maintainer, I want Editor and Quest modes to share policy and transactions, so that the product split does not create separate security models.
18. As an engine maintainer, I want Knowledge updates to be reviewable, so that AI summaries do not become trusted project facts by accident.

## UX Requirements

### Mode Selection

- Default to Editor Mode for ordinary chat and local editing.
- Keep Editor Mode available as the main game-making surface even when a Quest is active.
- Use explicit **Open Quest** and **Open Editor** navigation so users understand which surface they are in.
- Suggest Quest Mode when a request needs durable task state, multiple files, crates, editor systems, assets, validation steps, alternatives, long-running execution, or auditable review.
- Allow explicit user commands such as "New Quest", "Promote to Quest", "Open Editor", and "Continue Quest".
- Make the current mode visually obvious.

### Editor Game-Making Surface

The Editor surface should be designed as a real game editor, not only a code editor with AI attached. The first-screen experience should expose the project, current scene, selected object, inspector, play controls, and diagnostics. AI may live in a sidebar, but manual controls must remain discoverable and efficient.

Core first-class panels:

- **Scene View** for editing the world.
- **Game View** for previewing runtime output.
- **Hierarchy** for scene object structure.
- **Inspector** for selected object and component editing.
- **Project/Assets** for files, assets, prefabs, scripts, materials, audio, and scenes.
- **Console/Diagnostics** for errors, warnings, validation output, and tool results.
- **AI Assistant** for local questions and scoped edits.
- **Temporary AI Run** for bounded local SOLO tasks that do not need durable Quest state.

Minimum manual workflows:

- Create an empty object, camera, light, mesh/sprite object, audio source, rigidbody, collider, script, behavior, material, prefab, and scene.
- Add, remove, reorder where applicable, and edit component fields from the Inspector.
- Move, rotate, scale, duplicate, delete, parent, reparent, focus, and select scene objects.
- Save, open, and play a scene.
- Create or edit a script/behavior and see diagnostics.
- Import or reload an asset and see import status.
- Ask AI to execute a bounded local correction and inspect the preview before apply.

### Timeline Entries

Timeline entries should be compact by default and expand on demand. Entry types include:

- Intent or spec change.
- Thought or plan update.
- File read or context attachment.
- File edit with additions/deletions.
- Command run with result.
- Validation pass or failure.
- Review finding.
- Repair attempt.
- Manual intervention request or result.
- Alternative/prototype artifact.
- User decision.
- Blocked issue.

### Review Surface

The review surface should answer:

- What changed?
- Why did it change?
- Which files, scenes, assets, or schemas are affected?
- What validation ran?
- What review findings remain?
- What risks exist?
- What can be applied, partially applied, revised, continued, branched, archived, or discarded?
- How can the user undo or recover?

## Technical Notes

- This PRD does not replace `docs/ai-editor-copilot-prd.md`; it narrows the product interaction model that sits above that architecture.
- Quest execution should reuse the Copilot PRD's isolated git-backed task workspace model.
- Editor Mode should reuse existing editor command registration, transaction, diagnostics, and context services.
- Model/provider integration should serve the frontier-model-first product direction. It may support multiple capable providers, but the interaction model should not be designed around the lowest common denominator.
- Quest orchestration should be a structured application protocol, not a thin wrapper around chat. Agent messages may be part of the UI, but authoritative state should live in typed Quest records, events, artifacts, capability grants, validation records, and transaction bundles.
- Prompt text should not be the primary carrier for workflow, policy, or project context. The orchestrator should construct compact context packets from durable Quest state, project indexes, selected artifacts, schemas, tool manifests, validation state, and prior evidence.
- Command/tool access should be broad inside the assigned isolation boundary and narrow at the boundary to active project mutation. Blocking all command execution makes long-horizon SOLO ineffective; the correct control point is sandbox scope, credential scope, network policy, artifact review, validation, and transactional merge.
- Tooling should be designed as first-class product surface area for the model. If the model needs to inspect a scene graph, mutate a component, run a validator, compare artifacts, or assemble a transaction, that should be exposed as a structured tool rather than described repeatedly in prompt prose.
- Capability extraction should be treated as an engineering target. The orchestrator should minimize context noise, rank and compress relevant evidence, expose project topology, surface prior failed attempts, provide fast validator feedback, and allow multiple isolated attempts when that improves final quality.
- The system should measure and improve model throughput: time from intent to first useful action, tool-call latency, validator turnaround, context-packet relevance, failed-action recovery rate, and final review quality. These metrics matter because they determine how much of the frontier model's capability reaches the product.
- Quest metadata is owned by the editor profile, not by an individual project. The initial local store lives under the platform-specific Aster user-data directory in `quests/<quest-id>/`, so Quests remain available across project switches and editor restarts.
- Each Quest initially uses `quest.json` as the current state snapshot, `intent.md` as the durable task brief, optional `spec.md` as the editable specification for non-trivial work, and append-only `events.jsonl` as durable history. Archiving preserves this history; physical deletion requires a separate retention policy.
- A project-local `.aster/quests/` directory is reserved for explicit export or version-controlled publication of selected Quest artifacts. It is not the source of truth for the Quest registry.
- Quest briefs and specs should be ordinary Markdown so they can be opened, diffed, edited, searched, and archived.
- Knowledge should be stored separately from Quest specs to avoid mixing durable project facts with task-local assumptions.

## Success Metrics

- Users can create a Quest, inspect its intent or spec, execute or investigate through an appropriate path, inspect the timeline, and approve, revise, archive, or reject its result.
- A completed Quest produces durable intent, trace, relevant artifacts, validation or investigation summary where applicable, and final decision record.
- A blocked Quest reports a clear reason and at least one next action.
- Users can create or modify a simple game scene primarily through Quest intent, then inspect and adjust the result in Editor Mode.
- Users can promote an Editor conversation into a Quest without retyping the full context.
- Users can open Quest artifacts in the Editor and return to the Quest review state.
- Users can complete a localized Editor Mode correction without leaving the current file or scene.
- Small Quests can use a fast path without forced spec/review ceremony, while risky write Quests still require sufficient specification, validation, review, and explicit apply.
- The AI orchestrator can choose different Quest paths without UI rewiring or hard-coded lifecycle assumptions.
- Editor AI can complete short temporary SOLO tasks without creating durable Quest records.
- Quest execution shows measurable capability extraction improvements over a plain chat baseline: better context relevance, faster useful tool action, fewer dead-end loops, stronger validation evidence, and higher final review quality.
- The orchestrator can run isolated experiments or alternative attempts and preserve the best evidence without polluting the active project.
- No Quest write reaches the active project without final reviewed apply.

## Rollout Plan

### Phase 0 — Dual-Surface Product Model and Orchestrator-State Mocked UI

Deliverables:

- Define Editor Mode, Quest Mode, Knowledge, shared AI sidebar navigation, and **Open Editor/Open Quest** cross-surface navigation.
- Create static UI mockups for Quest list, intent/spec artifacts, orchestrator-selected state, execution timeline, review/diff surface, blocked/investigation result, Open Editor flow, and the supporting Editor inspection surface.
- Define the minimum Editor inspection and intervention workflow needed to review Quest output.
- Define Quest status enum and state transitions without requiring a fixed lifecycle sequence.
- Define minimal Quest metadata schema.
- Define capability-extraction targets: context relevance, tool latency, validator feedback speed, isolated exploration support, and review evidence quality.

Exit criteria:

- Product review confirms the dual-surface model: Quest is persistent ultra-long-horizon SOLO; Editor AI is temporary local execution plus direct inspection/intervention.
- Mocked UI demonstrates Quest creation, orchestrator-selected path, execute-progress, investigation/block result, final review, open-in-editor, local correction, and return-to-quest flows.

### Phase 1 — Quest Shell MVP

Deliverables:

- Create, list, open, rename, pause, resume, archive, and delete local Quests.
- Intent capture plus optional Markdown spec generation and editing.
- Timeline model with manual or stubbed event ingestion.
- Basic review screen for changed files, investigation results, blocked results, and decision controls.
- Open-in-Editor navigation from intent/spec artifacts, changed files, diagnostics, and review findings.

Exit criteria:

- User can create a Quest from a prompt or promoted Editor conversation.
- Quest state survives editor restart.
- Quest can show an orchestrator-selected next action without assuming a fixed lifecycle.
- User can inspect a Quest artifact in the Editor and return to the Quest.

### Phase 2 — Quest Execution MVP

Deliverables:

- Connect Quest execution to the safest available subset of isolated workspace, policy, tools, validation, and trace records.
- Show real file edits, command runs, validation results, blocked issues, and user decisions in the timeline.
- Produce final review bundles with apply, reject, revise, and quick-fix actions.
- Support at least one isolated exploration loop where the orchestrator can try, validate, repair, and compare before presenting a result.
- Keep active-project mutation behind final reviewed apply.

Exit criteria:

- A bounded multi-file task can run as a Quest without directly mutating the active project.
- Final apply uses reviewed editor transactions or an equivalent validated apply path.
- Blocked outcomes preserve evidence and next actions.

### Phase 3 — Editor Inspection and Intervention MVP

Deliverables:

- Scene/Game view, Hierarchy, Inspector, Project/Assets, Console/Diagnostics, and Play controls sufficient to inspect Quest output.
- Basic correction flows for selected entities, transforms, common components, scripts/behaviors, and asset references.
- Undo/redo and validation feedback for local corrections.
- AI panel anchored to current editor context for read-only explanation, small scoped edits, and temporary local SOLO tasks.
- Promote-to-Quest intent action for local work that grows beyond temporary correction.

Exit criteria:

- User can inspect Quest-generated scene, code, asset, and diagnostic artifacts in context.
- User can make a small local correction and return to the Quest review state.
- User can ask Editor AI to perform a bounded local task without creating a Quest.
- User can promote a multi-step Editor conversation into a Quest intent record without retyping context.

### Phase 4 — Quest Execution Hardening

Deliverables:

- Harden isolated workspaces, policy, tools, validation, trace records, transaction bundles, partial accept, and rollback.
- Expand validators for scenes, assets, scripts/behaviors, schemas, and targeted crate checks.
- Improve quick-fix repair loops and stale-context handling.

Exit criteria:

- Non-trivial game-making tasks can run as Quests with reliable validation, review, and rollback.
- Partial acceptance and revision flows preserve traceability.

### Phase 5 — Knowledge and Memory

Deliverables:

- Project Knowledge page.
- Memory proposal flow after completed Quests.
- De-duplication and reference validation for stored knowledge.
- Context attachment from Knowledge to Editor and Quest requests.

Exit criteria:

- Reusable project decisions can persist across sessions without relying on chat history.
- Users can inspect and remove stored knowledge.

## Risks and Mitigations

- **Risk: Mode split confuses users.**
  Mitigation: use duration and persistence language: Quest is persistent long-horizon SOLO; Editor AI is temporary local execution plus inspection, local correction, and direct control.

- **Risk: Editor roadmap dilutes the AI-first advantage.**
  Mitigation: limit Editor scope to inspection, intervention, diagnostics, local corrections, and essential creation controls; prioritize Quest shell, execution, review, and apply loops.

- **Risk: Quest output cannot be trusted or adjusted without enough Editor support.**
  Mitigation: make Scene/Game, Hierarchy, Inspector, Project/Assets, Play controls, diagnostics, and script/behavior views sufficient to inspect and fix Quest output, without trying to match full manual editors.

- **Risk: Quest becomes process-heavy.**
  Mitigation: let the AI orchestrator choose the minimum sufficient path; default to compact intent, compact timeline, and relevant outcome review; keep internal details expandable.

- **Risk: Editor Mode bypasses Quest safety for large work.**
  Mitigation: detect persistence, breadth, risk, or long-running scope growth and require promotion for broad write tasks.

- **Risk: Weak-model compatibility dilutes the product.**
  Mitigation: design interaction, orchestration, and review around frontier models. Do not add rigid compatibility flows or fallback UX that weakens the primary experience.

- **Risk: Weak framework or weak constraints make strong models unsafe.**
  Mitigation: use strong typed protocols, explicit Quest state, scoped capability grants, isolated workspaces, durable traces, validation evidence, and transaction bundles. Let the frontier model control workflow inside those constraints; do not replace constraints with prompt discipline.

- **Risk: Prompt bloat hides missing product capabilities.**
  Mitigation: move repeated instructions, policies, project facts, and workflows into tools, schemas, context packets, validators, and durable Quest state. Treat long prompts that compensate for missing tools as design debt.

- **Risk: Command restriction breaks SOLO without providing real safety.**
  Mitigation: allow command execution inside isolated workspaces with scoped filesystem, network, environment, and credential access. Protect the active project through validation, review, transaction assembly, explicit apply, and rollback.

- **Risk: Product design leaves model capability unused.**
  Mitigation: optimize for capability extraction: high-signal context packets, rich tools, low-latency feedback, isolated exploration, branch/compare workflows, and self-correction loops. Treat unnecessary confirmation steps, noisy context, missing tools, and slow validators as direct quality regressions.

- **Risk: Knowledge stores wrong AI summaries.**
  Mitigation: mark AI-generated knowledge as proposed until accepted or validated against project files.

- **Risk: Users mistake workspace edits for active project edits.**
  Mitigation: strongly label proposed/workspace/reviewed/applied states and require final apply.

- **Risk: Quest resume uses stale context.**
  Mitigation: bind execution to snapshots and require rebase or revalidation when the active project changes materially.

## Out of Scope

- Cloud synchronization of Quests.
- Multi-user collaborative Quest review.
- Mobile editor support.
- Fully autonomous active-project mutation.
- Marketplace implementation beyond navigation placeholders.
- Voice input and speech control.
- Proprietary model-specific optimization.
- Compatibility modes for weak or unreliable models.
- Thin chat-only Quest implementations without typed state, capability boundaries, validation evidence, and reviewable apply bundles.
- Prompt-heavy Quest implementations that substitute long instructions for tools, schemas, validators, and durable state.
- Safety designs that primarily rely on forbidding model command execution instead of isolating command execution and controlling apply boundaries.

## Open Questions

- What is the minimum Editor inspection/intervention set required before Quest results feel reviewable?
- Which component types should be considered "common components" for the first Inspector workflow?
- Should the default app launch into Quest Mode, a recent Quest, or an Editor/Quest split view?
- Should the Editor default layout be adaptive by selected Quest artifact type?
- What retention, compaction, and export policy should apply to the editor-profile Quest event history?
- Which Quest artifacts should users be able to explicitly export into project-local `.aster/quests/` for version control?
- What is the minimum transaction grouping needed for partial acceptance?
- Which editor contexts should be attached automatically versus manually selected?
- How should Quest status map to existing trace and workspace identifiers?
- What UI affordance best communicates the difference between workspace changes and active project changes?
- Should Knowledge updates require explicit user approval in all cases, or can low-risk convention updates be auto-proposed into a pending queue?
- What minimum model capability bar is required for Quest orchestration to be enabled?

## Relationship to Prior Art

- **Qoder:** reference for separating AI-led Quest work from user-led Editor work, with specs, timelines, and review surfaces; Aster should go further by making Quest orchestrator-driven rather than a fixed spec wizard.
- **Unity and Godot:** references for the minimum inspection and intervention affordances users expect around scenes, hierarchy, inspector, asset browser, play controls, and component editing. Aster should not attempt feature parity as its main advantage.
- **VS Code:** reference for code editing, file navigation, command palette, terminal integration, and a side AI/chat surface.
- **Claude Code:** reference for plan/execute discipline, terminal-native coding flow, and concise progress reporting.
- **OpenCode:** reference for provider abstraction, TUI architecture, commands, and extensibility.
- **Codex CLI:** reference for precise patch-based editing, minimal changes, and safe terminal collaboration.
- **Goose:** reference for local tool and extension integration.
- **MiMo Code:** reference for persistent memory, checkpointing, Compose-style long-task execution, and status visibility.
- **Cursor and Windsurf:** references for editor-native context, inline assistance, and low-friction code navigation.

The Aster-specific direction is not to clone any one tool. It is to be AI-first, frontier-model-first, and Quest-led: persistent ultra-long-horizon SOLO happens in Quest, temporary local SOLO happens in Editor AI, and only enough Unity/Godot-style editor affordance and VS Code-style code navigation is built to inspect, correct, and trust AI-produced game changes while preserving Aster's existing safety, transaction, and engine architecture boundaries.
