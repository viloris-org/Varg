# Aster AI Editor and Quest Modes PRD

状态：Draft  
目标版本：分阶段交付  
最后更新：2026-06-19

## Problem Statement

Aster already has a detailed Copilot architecture proposal in `docs/ai-editor-copilot-prd.md`, including agent tooling, isolated task workspaces, policy, capability grants, validation, review, and traceability. What is still missing is a product-level interaction model that separates two different ways users work with AI inside an editor:

- Immediate, user-led editing where the human is actively navigating files, scenes, assets, and selections while AI assists nearby.
- Long-running, AI-led task execution where the user describes an outcome, reviews a plan, watches progress, and approves a final change bundle.

Treating both workflows as one chat panel creates confusing authority boundaries. Sometimes the user wants a lightweight assistant beside the current file or scene. Other times the user wants a persistent task container that can plan, execute, validate, pause, resume, and produce an auditable review. Aster needs these modes to be distinct at the product level while sharing the same underlying agent safety infrastructure.

The product position is AI-first. Aster should not try to win by outbuilding mature manual editors such as Unity, Godot, or VS Code. Its differentiated surface is Quest Mode: an AI-led, spec-driven task cockpit for turning intent into reviewed game changes. Editor Mode still matters, but mainly as the inspect-and-intervene surface where users verify AI output, make precise manual adjustments, and handle small local edits. The editor only needs enough manual game-making affordance to keep users in control and make Quest output inspectable; it does not need to compete as a full manual-first editor.

## Solution

Introduce two connected work surfaces in Aster: **Quest Mode** and **Editor Mode**.

**Quest Mode** is the primary AI-first product surface. The primary object is a Quest: a named task with a goal, spec, plan, execution timeline, checkpoints, validation evidence, review result, diff bundle, unresolved issues, and final apply controls. The user steers through intent, clarifications, plan approval, review, and final acceptance. AI owns most implementation steps inside bounded workspaces and reports progress through an auditable timeline.

**Editor Mode** is the supporting inspect-and-intervene surface. The user remains the primary driver while inspecting Quest output, editing files or scene artifacts, manipulating selected entities, adjusting components, running diagnostics, and asking AI for localized help. AI may explain, propose, apply small scoped edits, generate snippets, or run safe tools with explicit approval. Context is anchored to the current editor state: selected entity, open scene, active file, selected text, diagnostics, recent commands, visible assets, and active editor tool.

The two modes should feel like a dual-interface AI-first product, not competing panels. Quest Mode is where game-making intent becomes tracked AI work. Editor Mode is where users inspect, verify, and precisely adjust project state. A user can promote an Editor conversation into a Quest when scope grows. A user can open Quest files, specs, diffs, scene changes, diagnostics, and review artifacts in the Editor to inspect or manually adjust the result. Both modes share project knowledge, model providers, tool permissions, traces, diagnostics, and memory, but they differ in who is driving the workflow and where trusted project state is edited.

## Product Principles

- **AI-first, Quest-led.** Aster's primary differentiator is AI-led game creation through recoverable Quests, not parity with manual-first editors.
- **Mode reflects control.** Quest Mode is AI-led with user steering and review; Editor Mode is user-led inspection, intervention, and local assistance.
- **Editor is necessary but not the moat.** Editor Mode must support inspection, validation, precise manual fixes, and minimal game-making operations, but it should not drive the roadmap ahead of Quest.
- **A Quest is not a chat.** It is a recoverable task workspace with a spec, execution state, artifacts, and lifecycle.
- **Specs are visible artifacts.** Long-running AI work should start from a concise Markdown spec that can be reviewed and edited.
- **Progress is auditable.** File reads, file edits, command runs, validation results, checkpoints, and reviews should appear in a timeline.
- **Final apply is explicit.** Quest output becomes a reviewed bundle that the user can approve, reject, partially accept, or revise.
- **The active project remains protected.** AI-led write work happens through isolated workspaces and reviewed transaction bundles, consistent with the Copilot PRD.
- **Internal complexity stays collapsible.** Users should see clear progress by default and expand details only when needed.

## Mode Definitions

### Editor Mode

Editor Mode is the supporting editor surface with embedded AI assistance. It must be good enough to inspect, verify, and precisely adjust AI-generated game changes, while avoiding a roadmap that tries to match mature manual-first editors feature-for-feature.

Primary user intent:

- Inspect Quest-generated scene, asset, script, behavior, and component changes.
- Open Quest specs, diffs, diagnostics, review findings, and changed files in context.
- Make precise manual corrections when AI output is close but needs human judgment.
- Create or adjust simple objects, components, scripts, behaviors, materials, and assets when this is faster than starting a new Quest.
- Manipulate selected entities in a scene or game view with basic transform, selection, and camera controls.
- Enter play mode or run checks to validate Quest output.
- Ask questions about the current scene, file, selected entity, component, asset, script, or diagnostic.
- Request small changes such as adding a component, fixing a script error, renaming a symbol, or generating a behavior snippet.
- Use AI while continuing to manually edit files and manipulate editor state.
- Keep tight control over each change.

Behavior:

- Context defaults to the active editor surface and explicit user selections.
- Manual editor actions are available for inspection and precision fixes, but broad creation work should naturally route to Quest.
- The AI response can include explanations, suggested edits, inline patches, commands, or previewable operations.
- Write operations are scoped, previewable, undoable, and traceable.
- The user can directly edit while the AI panel remains open.
- The system can suggest promoting the session to a Quest when the request becomes multi-step, cross-system, or long-running.

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

### Quest Mode

Quest Mode is a persistent AI task workspace for larger outcomes.

Primary user intent:

- Turn a feature, bug, refactor, content task, or investigation into a tracked unit of work.
- Let AI plan and execute multiple steps without requiring per-operation supervision.
- Review a complete result with diffs, validation evidence, and risk summary.
- Pause, resume, archive, or reopen task work.

Behavior:

- Each Quest has a title, goal, status, spec document, timeline, workspace, artifacts, validation results, review state, and final decision.
- The Quest starts by producing or importing a Markdown spec.
- The user can edit the spec before execution.
- Execution occurs inside the isolated task workspace described by the Copilot PRD.
- The timeline records meaningful events: planning, file reads, edits, command runs, checks, validation failures, repairs, reviews, and user decisions.
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
- **Promote to Quest:** converts a growing Editor chat and selected context into a draft Quest spec.
- **Open in Editor:** opens a Quest file, spec, diff, scene artifact, diagnostic, or review finding in the relevant editor surface.

### Quest Workspace

A Quest workspace should include:

- **Quest Header:** title, project name, status, branch or workspace identity, last activity, and open-in-editor action.
- **Spec Pane:** Markdown plan/spec with context, goals, non-goals, decisions, slices, files likely to change, acceptance criteria, and validation plan.
- **Execution Timeline:** chronological agent activity and user decisions with compact entries and expandable details.
- **Review Pane:** final diff summary, validation results, reviewer findings, unresolved issues, risk classification, and apply controls.
- **Input Bar:** prompt box for steering, clarifying, continuing, pausing, requesting review, or triggering quick-fixes.

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
- **Promote to Quest:** action that converts the current conversation and selected context into a Quest draft spec.

## Quest Lifecycle

1. **Create:** user starts a Quest from a prompt, template, issue, selected editor context, or promoted Editor conversation.
2. **Draft Spec:** AI creates a concise spec with context, goals, non-goals, vertical slices, affected areas, risks, and validation plan.
3. **Review Spec:** user can edit, approve, narrow, or cancel the spec.
4. **Snapshot:** system captures immutable project context and prepares an isolated task workspace.
5. **Plan:** AI decomposes the Quest into independently reviewable slices.
6. **Execute:** agents perform work inside the task workspace and report meaningful timeline events.
7. **Validate:** deterministic checks, targeted tests, schema validation, and editor-specific validations run where applicable.
8. **Repair:** scoped repair loops handle validation or review failures within retry limits.
9. **Review:** system presents diff, validation evidence, review findings, unresolved issues, and risk summary.
10. **Decide:** user approves, rejects, partially accepts, requests revision, triggers quick-fix, pauses, or archives.
11. **Apply:** approved changes enter the active project through reviewed transactions with undo or rollback support.
12. **Remember:** durable learnings, decisions, and project conventions are proposed for Knowledge after user approval or policy acceptance.

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
- Use existing editor commands and transaction systems rather than bypassing editor architecture.
- Show command/tool results as compact cards with expandable logs.
- Offer **Promote to Quest** when scope grows beyond localized assistance.

### Quest Mode Requirements

- Create, rename, pause, resume, cancel, archive, and reopen Quests.
- Store each Quest as a durable task record with stable ID, title, status, timestamps, spec path, workspace ID, and trace links.
- Generate a Markdown spec before execution for non-trivial write tasks.
- Allow user edits to the spec before execution starts.
- Maintain a timeline of meaningful events, not raw token-by-token model output.
- Display changed files with additions/deletions and status.
- Run validation commands through the policy-approved command registry.
- Present final review with summary, diff groups, validation evidence, unresolved issues, quick-fix actions, and apply controls.
- Support partial acceptance when transaction grouping permits it.
- Preserve blocked outcomes instead of silently hiding them.

### Knowledge Requirements

- Maintain project-level knowledge separate from transient chat history.
- Store durable facts such as architecture decisions, coding conventions, preferred workflows, known caveats, and recurring user preferences.
- Propose memory updates after completed Quests when new reusable knowledge appears.
- De-duplicate, compress, and validate memory references periodically.
- Clearly distinguish trusted project documentation from AI-generated summaries.

### Transition Requirements

- Editor Mode can promote a conversation into a Quest draft.
- Quest Mode can open files, diffs, diagnostics, and specs in the Editor.
- Quest Mode can open scene artifacts in the Scene View, entity changes in the Inspector, asset changes in Project/Assets, and script changes in the script editor.
- Quest Mode can request user manual edits in Editor Mode when that is safer or faster than agent execution.
- Completed Quest learnings can be proposed for Knowledge.
- Knowledge can be attached as context to both Editor requests and Quest specs.

## Non-Functional Requirements

- **Responsiveness:** Editor Mode should feel interactive; lightweight read-only questions should not spawn an agent cluster.
- **Manual viability:** Aster must remain usable for core game-making workflows when AI is disabled or unavailable.
- **Recoverability:** Quests must survive editor restart and support resume where workspace state is valid.
- **Traceability:** Every AI write, command run, validation result, review decision, and user approval must be trace-linked.
- **Safety:** No AI-led Quest write should directly mutate the active project before reviewed apply.
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
8. As a developer, I want each Quest to have a spec, so that I can confirm the task before AI starts changing files.
9. As a developer, I want to edit the Quest spec manually, so that my intent overrides the model's first interpretation.
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
- Suggest Quest Mode when a request spans multiple files, crates, editor systems, assets, or validation steps.
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

Minimum manual workflows:

- Create an empty object, camera, light, mesh/sprite object, audio source, rigidbody, collider, script, behavior, material, prefab, and scene.
- Add, remove, reorder where applicable, and edit component fields from the Inspector.
- Move, rotate, scale, duplicate, delete, parent, reparent, focus, and select scene objects.
- Save, open, and play a scene.
- Create or edit a script/behavior and see diagnostics.
- Import or reload an asset and see import status.

### Timeline Entries

Timeline entries should be compact by default and expand on demand. Entry types include:

- Thought or plan update.
- File read or context attachment.
- File edit with additions/deletions.
- Command run with result.
- Validation pass or failure.
- Review finding.
- Repair attempt.
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
- What can be applied, partially applied, revised, or discarded?
- How can the user undo or recover?

## Technical Notes

- This PRD does not replace `docs/ai-editor-copilot-prd.md`; it narrows the product interaction model that sits above that architecture.
- Quest execution should reuse the Copilot PRD's isolated git-backed task workspace model.
- Editor Mode should reuse existing editor command registration, transaction, diagnostics, and context services.
- Quest metadata can initially be local JSON or Markdown sidecar files under an editor-managed internal directory; the exact storage path needs a follow-up design.
- Quest specs should be ordinary Markdown so they can be opened, diffed, edited, searched, and archived.
- Knowledge should be stored separately from Quest specs to avoid mixing durable project facts with task-local assumptions.

## Success Metrics

- Users can create a Quest, review its spec, execute it, inspect the timeline, and approve or reject its result.
- A completed Quest produces a durable spec, trace, changed-file summary, validation summary, and final decision record.
- A blocked Quest reports a clear reason and at least one next action.
- Users can create or modify a simple game scene primarily through Quest intent, then inspect and adjust the result in Editor Mode.
- Users can promote an Editor conversation into a Quest without retyping the full context.
- Users can open Quest artifacts in the Editor and return to the Quest review state.
- Users can complete a localized Editor Mode correction without leaving the current file or scene.
- No Quest write reaches the active project without final reviewed apply.

## Rollout Plan

### Phase 0 — Dual-Surface Product Model and Mocked UI

Deliverables:

- Define Editor Mode, Quest Mode, Knowledge, shared AI sidebar navigation, and **Open Editor/Open Quest** cross-surface navigation.
- Create static UI mockups for Quest list, Quest spec, execution timeline, review/diff surface, Open Editor flow, and the supporting Editor inspection surface.
- Define the minimum Editor inspection and intervention workflow needed to review Quest output.
- Define Quest status enum and lifecycle transitions.
- Define minimal Quest metadata schema.

Exit criteria:

- Product review confirms the dual-surface model: Quest is the primary AI-first surface; Editor is the inspect-and-intervene surface.
- Mocked UI demonstrates Quest creation, spec review, execute-progress, final review, open-in-editor, local correction, and return-to-quest flows.

### Phase 1 — Quest Shell MVP

Deliverables:

- Create, list, open, rename, pause, resume, archive, and delete local Quests.
- Markdown spec generation and editing.
- Timeline model with manual or stubbed event ingestion.
- Basic final review screen with changed files and decision controls.
- Open-in-Editor navigation from spec, changed files, diagnostics, and review findings.

Exit criteria:

- User can create a Quest from a prompt or promoted Editor conversation.
- Quest state survives editor restart.
- User can inspect a Quest artifact in the Editor and return to the Quest.

### Phase 2 — Quest Execution MVP

Deliverables:

- Connect Quest execution to the safest available subset of isolated workspace, policy, tools, validation, and trace records.
- Show real file edits, command runs, validation results, blocked issues, and user decisions in the timeline.
- Produce final review bundles with apply, reject, revise, and quick-fix actions.
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
- AI panel anchored to current editor context for read-only explanation and small scoped edits.
- Promote-to-Quest draft action for local work that grows beyond a correction.

Exit criteria:

- User can inspect Quest-generated scene, code, asset, and diagnostic artifacts in context.
- User can make a small local correction and return to the Quest review state.
- User can promote a multi-step Editor conversation into a Quest draft without retyping context.

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
  Mitigation: use control-oriented language: Quest is for AI-led creation and tracked work; Editor is for inspection, local correction, and direct control.

- **Risk: Editor roadmap dilutes the AI-first advantage.**  
  Mitigation: limit Editor scope to inspection, intervention, diagnostics, local corrections, and essential creation controls; prioritize Quest shell, execution, review, and apply loops.

- **Risk: Quest output cannot be trusted or adjusted without enough Editor support.**  
  Mitigation: make Scene/Game, Hierarchy, Inspector, Project/Assets, Play controls, diagnostics, and script/behavior views sufficient to inspect and fix Quest output, without trying to match full manual editors.

- **Risk: Quest becomes process-heavy.**  
  Mitigation: default to compact spec, compact timeline, and final outcome review; keep internal details expandable.

- **Risk: Editor Mode bypasses Quest safety for large work.**  
  Mitigation: detect scope growth and require promotion for broad write tasks.

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

## Open Questions

- What is the minimum Editor inspection/intervention set required before Quest results feel reviewable?
- Which component types should be considered "common components" for the first Inspector workflow?
- Should the default app launch into Quest Mode, a recent Quest, or an Editor/Quest split view?
- Should the Editor default layout be adaptive by selected Quest artifact type?
- Where should local Quest metadata live inside an Aster project or editor profile?
- Should Quest specs be committed to the project by default, or treated as local editor artifacts?
- What is the minimum transaction grouping needed for partial acceptance?
- Which editor contexts should be attached automatically versus manually selected?
- How should Quest status map to existing trace and workspace identifiers?
- What UI affordance best communicates the difference between workspace changes and active project changes?
- Should Knowledge updates require explicit user approval in all cases, or can low-risk convention updates be auto-proposed into a pending queue?

## Relationship to Prior Art

- **Qoder:** strongest reference for separating AI-led Quest work from user-led Editor work, with specs, timelines, and review surfaces.
- **Unity and Godot:** references for the minimum inspection and intervention affordances users expect around scenes, hierarchy, inspector, asset browser, play controls, and component editing. Aster should not attempt feature parity as its main advantage.
- **VS Code:** reference for code editing, file navigation, command palette, terminal integration, and a side AI/chat surface.
- **Claude Code:** reference for plan/execute discipline, terminal-native coding flow, and concise progress reporting.
- **OpenCode:** reference for provider abstraction, TUI architecture, commands, and extensibility.
- **Codex CLI:** reference for precise patch-based editing, minimal changes, and safe terminal collaboration.
- **Goose:** reference for local tool and extension integration.
- **MiMo Code:** reference for persistent memory, checkpointing, Compose-style long-task execution, and status visibility.
- **Cursor and Windsurf:** references for editor-native context, inline assistance, and low-friction code navigation.

The Aster-specific direction is not to clone any one tool. It is to be AI-first and Quest-led, using only enough Unity/Godot-style editor affordance and VS Code-style code navigation to inspect, correct, and trust the game changes produced by Qoder-style AI Quest work, while preserving Aster's existing safety, transaction, and engine architecture boundaries.
