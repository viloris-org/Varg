# Aster UI Design Guidelines

This document defines the product UI language for Aster Editor, Copilot, and Quest. It is a product design contract, not a styling suggestion. UI decisions should start from user tasks, then expose only the surfaces needed for those tasks.

## Product Positioning

Aster is an AI-native game editor workbench.

The core experience is not an AI dashboard, task tracker, or agent runtime monitor. The user should feel that they are editing a game in a stable creative workspace, with AI available as contextual assistance.

## Design Principles

### Function Before Presentation

Every visible UI element must answer one of these questions:

- What am I editing?
- What can I change now?
- What did AI produce for me?
- What decision is required from me?
- What happened that affects my work?

Do not show UI that only explains internal orchestration, background stages, implementation details, or system process.

### Hide Process, Show Results

Agent and Copilot internals must not become product navigation.

Do not expose permanent UI for stages such as planning, tracing, checkpointing, validation, execution steps, or review phases unless the user needs to inspect evidence or resolve a decision.

Show:

- Generated spec
- Generated tasks
- Proposed changes
- Validation result
- Blocking issue
- Next action

Do not show by default:

- Agent timelines
- Raw trace streams
- Checkpoint lists
- Internal step progress
- Orchestration phase names
- Full execution logs

Logs, traces, checkpoints, and validation details belong in an on-demand Evidence or Details drawer.

### Progressive Disclosure

Surfaces appear only when they contain user-relevant content.

Spec, Tasks, Review, Changes, Diagnostics, and Knowledge must not be permanent top-level tabs in the editor. They are contextual artifacts. Show them only after AI creates them, or when a real error or user decision exists.

### Decision-First Interaction

When AI produces output, the UI should focus on the user's next decision.

Examples:

- Spec generated: Open spec, Use as brief, Dismiss
- Tasks generated: Review tasks, Start, Edit
- Changes prepared: Review changes, Apply, Request revision, Discard
- Validation failed: View issue, Ask AI to fix, Retry
- Quest blocked: Provide missing info, Retry, Cancel

Avoid making the user inspect the system's process before they can act.

### One Product Language

Hub, Editor, Copilot, and Quest must feel like one product. They may have different density and surface layouts, but they must share:

- Layout grammar
- Color semantics
- Typography
- Icon style
- Button hierarchy
- Artifact presentation
- State language

Quest is not a separate visual product. It is the asynchronous AI task mode of Aster Workbench.

### User Control Is Highest Priority

AI work must always remain interruptible, reviewable, and reversible from the user's point of view.

The following controls outrank normal navigation and panel content:

- Stop current AI work
- Undo applied AI changes
- Restore last stable editor state
- Save or discard local work
- Open failure details when recovery is not possible

If AI is running and could modify project state, the UI must expose a visible Stop action in the Copilot or Quest command area. If changes were applied, the UI must expose the most recent undo or restore action until it is no longer valid.

## Information Architecture

### Editor Default Surfaces

The editor's default workspace should prioritize making and editing the game.

Recommended persistent editor surfaces:

- Scene
- Assets
- Scripts
- Build

Conditional editor surfaces:

- Spec, only after a spec entity exists and is relevant to the current project or selected Quest
- Tasks, only after task entities exist and need review, execution, or progress tracking
- Changes, only after AI proposes file, asset, scene, or setting changes
- Diagnostics, only after issues exist or a recent operation failed
- Review, only when review output exists and requires acceptance, revision, or dismissal
- Knowledge, only when proposals, references, or scope controls exist

The old pattern of permanent `PRD | Tasks | Game | Assets | Scripts | Build | Diagnostics` navigation is not acceptable for the main editor because it exposes internal workflow and dilutes the scene-editing task.

Stable peer surfaces are areas that users intentionally visit as part of normal game creation even when AI is idle. Scene, Assets, Scripts, and Build qualify. Spec, Tasks, Changes, Diagnostics, Review, and Knowledge do not qualify unless they contain entities or require intervention.

If a conditional artifact becomes long-lived and high-frequency for a specific workflow, it may be promoted to a temporary workspace chip or tab for that project session. It must still be absent in new projects and hidden after the artifact is resolved or archived.

### Editor Layout

Default editor layout:

- Top bar: project identity, save/undo/redo, play/run, mode-specific high-frequency actions
- Left rail: scene hierarchy or asset/script navigation
- Center: viewport, document, code editor, or selected work surface
- Right panel: inspector or contextual AI panel
- Bottom bar: save state, selection, diagnostics summary, build/run state

The center work surface must remain visually dominant. AI should assist the current surface, not replace the user's mental model of the editor.

### Copilot Layout

Copilot should default to a compact contextual assistant, not a workflow dashboard.

Recommended states:

- Collapsed: 44px to 56px wide icon rail or small affordance
- Compact: 320px to 380px wide panel with chat input, context chips, and latest relevant result
- Expanded: 420px to 560px wide panel with conversation and artifact cards
- Focused: full-width review or diff experience only when the user chooses it

Copilot should not permanently display task timelines, operation plans, or process stages. It may show result cards and pending decisions.

At widths below 1180px, Copilot should default to collapsed unless the user explicitly opens it. At widths below 900px, Copilot should become an overlay drawer or bottom sheet and must not reduce the active work surface below a usable minimum.

The active work surface must keep at least 60% of available horizontal space on desktop widths. A temporary focused review surface may override this only after direct user action.

### Quest Layout

Quest is for asynchronous AI work. The user's primary question is: what decision is required now?

Quest should be status-driven, not tab-driven.

Recommended Quest model:

- Brief: define or refine the request
- Working: show concise status and allow pause or additional instruction
- Review: summarize outcome, changed files, validation, risks, and recommended action
- Blocked or Failed: show blocker, cause, and recovery actions
- Complete: show accepted result and links back to affected editor surfaces

The UI must not display these stages as a permanent process bar. The main view should morph based on the current state.

Quest uses the same workbench grammar as Editor:

- Top bar: project identity, Quest identity, global Stop or Resume when relevant, primary decision action
- Left rail: Quest queue, max 280px, collapsible, not a separate app shell
- Center: current decision or result, visually dominant
- Right drawer: artifacts and evidence, closed by default unless a decision depends on it

Quest should not use a permanent panel row of Overview, Intent, Spec, Review, Knowledge, Trace, Checkpoint, and Validation. Those are artifacts or evidence, not primary navigation.

Quest must preserve the editor's visual hierarchy: the center decision or result surface is primary, the queue is navigation, and evidence is secondary. A full three-column dashboard with equal visual weight is not acceptable.

## Artifact System

AI outputs should be represented as artifacts. Artifacts are user-facing results, not process steps.

Core artifact types:

- Spec
- Tasks
- Changes
- Review
- Diagnostics
- Knowledge proposal
- Build output
- Validation evidence

Artifact cards should include:

- Type
- Short title
- Source or affected surface
- Summary
- Primary action
- Secondary action
- Dismiss or archive affordance when appropriate

Artifact cards should not include raw trace data by default.

An artifact should render in the interface only when it contains a data entity, affects the current project, or requires user action. Placeholder tabs, empty artifact panels, and process-only artifacts should not render.

### Context Scope

AI context is product state and must be visible when it affects output quality or permissions.

Copilot and Quest must provide a Context Scope control that can show:

- Current project
- Selected scene objects
- Selected assets or scripts
- Referenced spec, tasks, or Quest
- Attached knowledge entries
- Allowed write scope
- Commands or tools that may be used

The compact form may be a small row of context chips. The expanded form should let users remove or limit context before asking AI to act. Context scope is not an agent trace; it is an input contract between the user and AI.

If AI is about to write, run commands, or use broad project knowledge, the confirmation UI must summarize the active scope before the user approves.

## Visual Language

### Style

Aster should use a professional dark workbench style.

The interface should feel:

- Stable
- Dense but readable
- Tool-like
- Low distraction
- Technical
- Contextual

Avoid:

- Marketing page composition
- Large decorative cards
- Dashboard spectacle
- Excessive blue or purple glow
- Timeline-heavy agent visualizations
- Decorative empty states that compete with work content

### Color Semantics

Use restrained color. Color should communicate state, selection, and risk.

Recommended semantics:

- Background: neutral dark workbench surfaces
- Primary accent: blue for selection, focus, and links
- Success: green for validation passed, running, saved
- Warning: amber for blockers, risk, pending decisions
- Danger: red for destructive actions, failed states, irreversible operations
- Purple: optional for AI identity, used sparingly and never as the dominant palette

Do not create a one-note blue or purple interface. The viewport, editor content, and data should remain the visual focus.

Color limits:

- Primary accent should occupy less than 12% of visible chrome in normal editor state.
- Purple AI identity color should occupy less than 6% of visible chrome and must not be used for ordinary selection.
- Glow effects are disabled by default. If used for live/run status, use only a single subtle shadow under 12px blur and under 35% alpha.
- Do not use large blue or purple gradients as panel backgrounds.
- Do not use color as the only status indicator; pair it with text, icon, or shape.

### Typography

Use a technical but readable pairing:

- UI text: IBM Plex Sans or equivalent
- Code, coordinates, IDs, logs: JetBrains Mono or equivalent

Guidelines:

- Normal UI text should be readable at dense desktop sizes.
- Use mono only for values, code, paths, commands, and diagnostics.
- Avoid oversized headings inside tool panels.
- Use label casing consistently.

### Shape and Density

Use professional desktop-tool geometry:

- Radius: 4px to 6px for controls, 6px to 8px for panels
- Avoid large rounded marketing cards
- Prefer thin borders and subtle surface contrast over heavy shadows
- Preserve stable dimensions for toolbars, icon buttons, rows, tabs, and counters

Cards are acceptable for repeated artifacts, review items, modals, and focused decision blocks. Do not put cards inside cards for layout decoration.

Allowed card containment exceptions:

- Code blocks inside review cards
- Diff hunks inside change cards
- Tables inside artifact detail cards
- Error detail panels inside failure cards

These exceptions must be structural content containers, not nested decorative cards. Use flatter treatments such as inset borders, code surfaces, or table rows instead of another floating card style.

### Icons

Use one consistent line icon set. Prefer Lucide-style icons when available.

Rules:

- No emoji icons
- No mixed icon stroke styles
- Icon buttons must have labels through visible text or tooltips
- Destructive icons must not rely on color alone
- Keep icon sizes consistent inside each toolbar or list

Fallback icons:

- Unknown artifact: file/document icon
- Unknown Quest/task type: check-circle or list icon
- Unknown asset type: package icon
- Unknown diagnostic: alert-circle icon
- Unknown AI action: sparkle or bot icon, only in AI-owned surfaces

Do not invent one-off icons for edge cases unless the icon becomes part of the shared system.

## Interaction Rules

### Navigation

Navigation should describe user surfaces, not implementation artifacts.

Good navigation labels:

- Scene
- Assets
- Scripts
- Build
- Review changes
- Open spec
- View diagnostics

Poor permanent navigation labels:

- Trace
- Checkpoint
- Intent
- Validation
- Proposed operations
- Execution plan

### Tabs

Tabs must be used for stable peer surfaces the user intentionally switches between. Do not use tabs to expose every possible backend artifact.

Conditional tabs or chips may appear when content exists. They should disappear or archive when resolved.

Stable peer surfaces meet all of these criteria:

- They are useful when AI is idle.
- They map to a user task, not a backend entity.
- They have enough content to justify a persistent surface.
- They remain meaningful across projects.

Temporary tabs or chips are allowed for large AI-generated artifacts when users repeatedly inspect them during a session. Temporary tabs must be visually different from permanent surfaces and must provide close, archive, or resolve actions.

### Status

Status should be short and action-oriented.

Good:

- Saved
- Unsaved changes
- 3 changes ready
- Validation failed
- Needs input
- Build prepared

Poor:

- Running phase 3 of 7
- Agent executing validation pipeline
- Trace pending
- Checkpoint workspace event accepted

### Empty States

Empty states should be compact and actionable.

They should explain:

- What is empty
- What the user can do next

They should not explain the entire product or internal AI workflow.

### Error States

Errors must be user-actionable and accessible.

Show:

- What failed
- Why it matters
- What the user can do
- Link to details when useful

Technical logs should be expandable, not the default presentation.

### Global Abort and Undo

When AI work is running:

- A Stop action must be visible in the active AI surface.
- Stop must be visually higher priority than non-critical navigation inside that surface.
- If stopping may leave partial work, the UI must say what will be preserved.
- A stopped operation should resolve into a reviewable partial result or a clear canceled state.

When AI work has changed project state:

- Show an Undo or Restore action near the completion summary.
- State what will be reverted.
- If undo is unavailable, explain why and provide the nearest recovery path.
- Destructive discard actions require confirmation when local user work could be lost.

### Degraded and Unrecoverable States

Not every failure is recoverable. The UI must define graceful degradation for:

- Network unavailable
- AI provider unavailable
- Authentication missing or expired
- Backend process disconnected
- Project file lock or permission failure
- Corrupt or unsupported project state
- Unknown fatal error

Degraded UI should show:

- Clear state title
- Plain-language cause
- What still works locally
- Primary recovery action
- Secondary action to continue without AI when possible
- Details drawer for technical evidence

If no recovery is available, do not pretend there is an action. Use a truthful terminal state such as `AI unavailable` or `Project cannot be opened`, then provide details and safe exit options.

## Accessibility

Accessibility is part of the design language.

Requirements:

- All functionality must be keyboard reachable.
- Tab order must match visual order.
- Focus states must be visible.
- Normal text contrast must meet WCAG AA, 4.5:1 minimum.
- Error messages must use accessible announcement where appropriate.
- Inputs must have labels.
- Color must not be the only indicator.
- Motion must respect `prefers-reduced-motion`.
- Resizable panes must expose separator semantics and keyboard resizing.

## Responsive Behavior

Aster is a desktop editor first, but it must degrade cleanly.

Breakpoints:

- 1440px and above: full workbench layout is allowed.
- 1180px to 1439px: secondary rails may narrow; Copilot should prefer compact or collapsed.
- 900px to 1179px: Copilot collapses by default; right inspector or AI becomes a drawer if the active surface loses too much space.
- Below 900px: single primary surface with drawer panels. Do not force simultaneous left rail, center surface, and right AI panel.

Minimum useful widths:

- Active work surface: 640px preferred, 520px minimum for non-viewport surfaces.
- Viewport: 720px preferred on desktop, 560px minimum before collapsing side panels.
- Inspector drawer: 300px to 360px.
- Copilot compact panel: 320px to 380px.
- Quest queue: 220px to 280px.

Priority on smaller widths:

1. Preserve the active work surface.
2. Collapse AI before reducing viewport usability.
3. Collapse secondary rails before hiding primary actions.
4. Convert side panels to drawers when needed.
5. Keep status concise.

The AI panel must not permanently consume a large fixed width on constrained screens.

## Surface Visibility Matrix

Every surface must pass this matrix before becoming visible. Permanent surfaces are allowed in new projects. Conditional and temporary surfaces are hidden until they contain relevant entities or require user intervention.

| Surface | Value when AI idle | User primary task | Entity required | Decision required | Default visible | Hide or archive condition | Temporary tab allowed | Max width or role | New project visible |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Scene | Yes | Edit game world | No | No | Yes | Never while project is open | Permanent | Primary center surface | Yes |
| Assets | Yes | Manage project resources | No | No | Yes | Never while project is open | Permanent | Left/center surface | Yes |
| Scripts | Yes | Edit code/behaviors | No | No | Yes | Never while project is open | Permanent | Center surface | Yes |
| Build | Yes | Package/run project | No | Sometimes | Yes | Never while project is open | Permanent | Center surface | Yes |
| Spec | No | Review or edit AI brief | Spec entity | Sometimes | No | Accepted, archived, dismissed, or no longer relevant | Yes | Temporary chip/tab or document drawer | No |
| Tasks | No | Review or execute generated work | Task entities | Often | No | Completed, archived, dismissed, or converted into Quest history | Yes | Temporary chip/tab or Copilot artifact | No |
| Changes | No | Review proposed project mutations | Change set | Yes | No | Applied, rejected, partially archived, or superseded | Yes | Focused review surface | No |
| Review | No | Accept, revise, or reject result | Review entity | Yes | No | Accepted, revision requested, or archived | Yes | Center decision surface | No |
| Diagnostics | No | Resolve actionable issue | Issue entity | Sometimes | No | Issue resolved and cooldown elapsed | Yes | Drawer or center surface for many issues | No |
| Knowledge | No | Approve AI memory/scope | Proposal or reference | Sometimes | No | Proposal accepted/rejected or scope cleared | Yes | Drawer or settings sub-surface | No |
| Evidence | No | Inspect proof/debug details | Evidence bundle | No | No | Parent artifact archived | No permanent tab | Right drawer or modal | No |

Promotion rule: a conditional surface may become a temporary tab only when users need to inspect it repeatedly during the same session. It must have close, resolve, or archive controls and must not appear in new projects by default.

## Artifact Lifecycle Spec

Artifacts are lifecycle-managed product entities. They are not permanent navigation.

Generic lifecycle:

`created -> pending decision -> accepted | rejected | revised | archived -> hidden | retained in history`

| Artifact | Created when | Display condition | Primary CTA | Secondary CTA | Completion condition | Auto-hide condition | History | Project state impact | Undo/restore role |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Spec | AI or user creates a brief/spec | Relevant to active project, Quest, or review | Use as brief / Open | Edit / Dismiss | Accepted, replaced, or archived | Accepted and no longer pinned | Yes | Usually no direct mutation | Not required unless saved to project files |
| Tasks | AI decomposes work | Tasks need review, execution, or progress visibility | Start / Review | Edit / Archive | All tasks complete, canceled, or converted into Quest | Completed and unpinned | Yes | May trigger future mutations | Discard proposal before execution |
| Changes | AI proposes file/asset/scene changes | Unapplied or partially applied change set exists | Review changes | Apply / Reject | Applied, rejected, or superseded | Rejected or fully applied and summary acknowledged | Yes | Yes when applied | Requires undo/restore contract |
| Review | AI or validation summarizes result | Acceptance, revision, or risk decision needed | Apply / Accept | Request revision / Discard | User decision recorded | Accepted/rejected and not pinned | Yes | May apply or reject changes | Must link to restore if changes were applied |
| Diagnostics | Runtime, build, validation, or AI detects issue | Actionable issue exists | Open issue / Fix with AI | Dismiss / Details | Issue resolved or acknowledged | Resolved plus cooldown | Yes for failures | No direct mutation | May reference failed restore |
| Knowledge proposal | AI suggests reusable project knowledge | User must approve or reject memory/scope | Approve | Reject / Edit | Accepted or rejected | Decision recorded | Yes | May update knowledge store | Revert knowledge entry if possible |
| Build output | User or AI runs build/package | Recent build result affects next action | Open output / Rebuild | Details / Dismiss | Newer build supersedes or user archives | Successful and acknowledged | Yes | May create generated files | Restore only if generated files are tracked |
| Validation evidence | AI/build/test validation runs | Supports review or failure decision | View summary | Evidence | Parent review resolved | Parent artifact archived | Yes | No direct mutation | Evidence for undo/restore decision |

Pinning rule: a user-pinned artifact may remain visible after normal auto-hide, but it must move to a temporary chip/tab or artifact list, not permanent global navigation.

Partial application rule: if a change set is partly applied, split it into `applied`, `rejected`, and `pending` groups. Each group must show its own restore or archive status.

## Quest State Machine

Quest is asynchronous AI work inside the Aster Workbench. It must be modeled as a state machine, not a dashboard timeline.

| State | User sees | Primary CTA | Stop | Resume | Add instruction | Partial artifacts | Undo | Blocks other Quest | Evidence drawer |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Draft | Editable request/brief | Start Quest | No | No | Yes | Optional spec | No | No | Closed |
| Queued | Position and project target | Cancel | Yes | No | Yes | No | No | Maybe, if same write scope | Closed |
| Running | Concise working state and latest result | Stop | Yes | No | Yes, as follow-up | Maybe | No | Yes for overlapping write scope | Closed by default |
| Needs Input | Question or missing info | Provide input | Yes | Yes after input | Yes | Maybe | No | Yes if same write scope | Optional |
| Needs Permission | Requested permission and scope | Approve / Deny | Yes | Yes after decision | Yes | Proposed ops | No | Yes if same write scope | Open if permission is risky |
| Paused | Pause reason and saved state | Resume | No | Yes | Yes | Maybe | No | Maybe | Optional |
| Stopping | Stop in progress | Wait | No | No | No | Maybe | No | Yes until settled | Optional |
| Stopped With Partial Result | What was preserved | Review partial result | No | Yes | Yes | Yes | Maybe | No after lock release | Open if side effects exist |
| Review Required | Outcome, files/assets, risks | Apply / Accept | No | No | Request revision | Yes | If already applied | No unless locks held | Available |
| Applying Changes | Applying selected changes | Stop if safe | Conditional | No | No | Changes | No until settled | Yes for affected scope | Optional |
| Validating | Validation status | Stop if safe | Conditional | No | No | Validation evidence | Maybe | Yes for affected scope | Optional |
| Validation Failed | Actionable failure summary | Fix / Request revision | No | Yes after choice | Yes | Diagnostics/evidence | Maybe | No unless locks held | Available |
| Conflict Detected | Conflicting files/assets and choices | Resolve conflict | No | Yes after resolution | Yes | Changes/evidence | Maybe | Yes for affected scope | Open |
| Completed | Accepted result and links | Open affected surface | No | No | No | Summary | Restore if valid | No | Available |
| Failed Recoverable | Failure with recovery path | Retry / Fix | No | Yes | Yes | Evidence | Maybe | No unless locks held | Open |
| Failed Terminal | Truthful terminal failure | View details / Exit | No | No | No | Evidence | Maybe | No | Open |
| Canceled | Canceled before completion | Archive / Restart | No | Yes as new run | No | Maybe | Maybe | No | Available if side effects |
| Archived | Historical record | Reopen | No | No | No | Historical | No | No | Closed |

Conflict rule: if a user manually edits files, assets, scene graph, or project settings touched by a running Quest, the Quest must move to `Conflict Detected` before applying changes.

Cross-session rule: a Quest that continues after the editor closes must restore into one of `Running`, `Needs Input`, `Needs Permission`, `Review Required`, `Failed Recoverable`, or `Failed Terminal`. It must never restore into an ambiguous generic loading state.

## AI Permission Model

AI actions must be permissioned by risk. Confirmation strength increases with risk.

| Level | Name | Examples | Confirmation | Stop | Evidence | Undo/restore requirement |
| --- | --- | --- | --- | --- | --- | --- |
| L0 | Read-only assist | Inspect context, answer questions, summarize selected object | No confirmation after context is visible | Not required | Optional | None |
| L1 | Generate draft artifact | Create spec, tasks, suggestions, non-applied review | Lightweight confirmation only if broad context is used | Optional | Artifact summary | Discard artifact |
| L2 | Propose project changes | Draft file/asset/scene changes without applying | Review required before apply | Yes while generating | Change preview and context scope | Discard proposal |
| L3 | Apply reversible changes | Write files, edit scene graph, update settings with checkpoint | Explicit confirmation with scope and affected entities | Yes while applying if safe | Change set and checkpoint | Undo or restore required |
| L4 | Run commands/builds | Build, test, package, long-running tools, dependency resolution | Strong confirmation with command, scope, duration, and side effects | Yes | Command output, exit status, environment summary | Restore generated files where tracked; explain untracked side effects |
| L5 | Destructive or irreversible operations | Delete assets, overwrite binary resources, migration, external publish, install/uninstall dependencies | High-friction confirmation; require explicit typed or two-step consent for irreversible work | Yes if operation supports it | Full evidence bundle | Must create restore point first, or clearly block operation if restore is impossible |

Network or external knowledge access must be disclosed at L2 or higher when it influences project changes. Installing dependencies, deleting files, publishing, or irreversible conversion must never be hidden inside a generic Apply button.

## Undo and Restore Contract

Undo language must be precise. Do not imply safety that the system cannot provide.

| Action | Meaning | Applies to | Must show | Invalid when |
| --- | --- | --- | --- | --- |
| Discard proposal | Remove unapplied AI output | Draft specs, tasks, change sets | What artifact is removed | Artifact already applied or archived |
| Cancel running operation | Stop work before final commit | Running AI/tool work | What partial data remains | Operation already settled or cannot be interrupted |
| Undo | Reverse most recent applied editor change | Scene graph, text edits, tracked project mutations | Items reverted and items not reverted | User edited affected data after apply, undo stack expired, or side effects untracked |
| Restore stable state | Return to pre-AI checkpoint | Project files/assets/settings covered by checkpoint | Checkpoint time, scope, excluded side effects | Checkpoint missing, corrupted, or conflicts with newer user edits |
| Revert selected files | Restore selected file set | Text files and trackable assets | File list and source version | Files deleted/renamed externally or contain newer user edits |
| Manual recovery | Provide steps when automatic recovery is impossible | External commands, untracked generated files, provider failures | Clear steps and evidence | Never invalid; may be incomplete but must be honest |

Every AI change summary must state:

- What will be reverted
- What will not be reverted
- Whether newer manual edits are affected
- Restore point creation time
- Restore point validity
- Why restore is unavailable, when unavailable

## Evidence Drawer IA

Evidence is the safety valve for hiding process without hiding accountability.

Default drawer sections:

1. Summary
2. User-relevant cause
3. Affected files, assets, scene objects, or settings
4. Validation evidence
5. Command output
6. AI context used
7. Tool actions
8. Recovery notes
9. Raw logs, collapsed by default

Rules:

- The drawer opens from artifact cards, review summaries, diagnostics, permission prompts, and terminal failure states.
- It opens by default only when user trust or recovery depends on details, such as conflicts, L4/L5 permissions, terminal failures, or validation failures.
- It must support copying relevant evidence for bug reports.
- Raw logs are collapsed by default.
- Sensitive values must be redacted.
- Evidence should be searchable when it contains more than 30 rows or long command output.
- Evidence belongs to a parent artifact or Quest state and follows its archive policy.

Evidence and Details are the same component. Use the label `Evidence` when proof or diagnostics matter; use `Details` only for low-risk explanatory expansion.

## AI Provenance

AI identity is a provenance signal, not a decorative theme.

Use these provenance states:

- AI proposed
- User accepted
- AI applied
- User edited after AI
- Generated but unreviewed
- Generated and validated
- Generated but failed validation

Rules:

- AI-owned surfaces may use the AI identity token and fallback AI icon.
- Once a user accepts AI output into normal project content, the content should use normal project styling.
- Keep provenance available in metadata, history, or Evidence, even when visual AI styling disappears.
- If the user edits AI-generated content, show `User edited after AI` in history or details, not as a permanent badge on the main surface.
- Failed validation must remain visible until resolved, dismissed with acknowledgement, or archived.

## Build, Run, Validation, and Diagnostics

These surfaces must not duplicate each other.

Definitions:

- Build: user or AI-triggered build, run, package, or export surface.
- Diagnostics: current actionable issues in the project.
- Validation evidence: proof from a specific AI, build, test, or review operation.
- Quest Review: asynchronous task outcome summary.

Rules:

- Build failure creates build output and may create Diagnostics if the issue remains actionable.
- Validation failure creates validation evidence and may create Diagnostics if the user can act on it outside that review.
- Quest Review references build output, diagnostics, and validation evidence; it must not duplicate their full content.
- Bottom bar shows current summary only, such as `2 issues` or `Build failed`; it links to the owning surface or artifact.
- Manual Run failures and AI-triggered validation failures use the same diagnostic language and severity model.

## Collaboration Assumptions

Current design rules assume a single local user unless a feature explicitly says otherwise.

Future collaboration must preserve these extension points:

- Quest owner
- User who approved AI write
- User who applied changes
- User who stopped or restored work
- Shared project lock or write scope
- Conflict owner and resolution state
- Review approval status
- Provenance of AI-generated artifacts

If multi-user collaboration is introduced, L3 to L5 AI actions must show who initiated them and who approved them. Restore and Stop permissions must be explicit.

## Design Tokens

These tokens define the workbench language. Product surfaces may override layout composition, but they should not invent ad hoc color, spacing, radius, or elevation values.

### Color Tokens

| Token | Role | Default |
| --- | --- | --- |
| `bg.base` | App background | `#18181B` |
| `bg.surface` | Panels and toolbars | `#232327` |
| `bg.subtle` | Inset/code/evidence surfaces | `#111216` |
| `bg.hover` | Hover background | `#2D2D32` |
| `border.default` | Standard border | `#36363B` |
| `border.strong` | Emphasized border | `#42424A` |
| `text.primary` | Primary text | `#F4F4F5` |
| `text.secondary` | Secondary text | `#A1A1AA` |
| `text.muted` | Muted labels | `#71717A` |
| `accent.primary` | Selection/focus/link | `#60A5FA` |
| `accent.ai` | AI identity only | `#A78BFA` |
| `status.success` | Saved/passed/running | `#22C55E` |
| `status.warning` | Risk/pending/blocker | `#F59E0B` |
| `status.danger` | Failure/destructive | `#EF4444` |
| `focus.ring` | Keyboard focus | `rgba(96, 165, 250, 0.35)` |

### Typography Tokens

| Token | Value |
| --- | --- |
| `font.ui` | IBM Plex Sans or system sans |
| `font.mono` | JetBrains Mono or equivalent |
| `font.size.xs` | 10px |
| `font.size.sm` | 11px |
| `font.size.base` | 13px |
| `font.size.panel-title` | 12px to 14px |
| `font.size.surface-title` | 16px to 20px |
| `line-height.dense` | 1.25 |
| `line-height.readable` | 1.45 to 1.6 |

### Density and Layout Tokens

| Token | Value |
| --- | --- |
| `toolbar.height` | 40px to 44px |
| `statusbar.height` | 22px to 26px |
| `panel.header.height` | 36px to 44px |
| `row.height.compact` | 28px to 32px |
| `row.height.default` | 34px to 40px |
| `button.height.compact` | 26px to 30px |
| `button.height.default` | 32px to 36px |
| `input.height` | 30px to 36px |
| `icon.size.sm` | 14px |
| `icon.size.default` | 16px |
| `icon.size.lg` | 20px |
| `target.minimum.pointer` | 28px desktop, 40px touch/overlay |
| `panel.padding` | 8px to 12px |
| `surface.padding` | 12px to 16px |
| `artifact.max-height-before-collapse` | 360px |

### Shape, Motion, and Layer Tokens

| Token | Value |
| --- | --- |
| `radius.control` | 4px to 6px |
| `radius.panel` | 6px to 8px |
| `radius.modal` | 8px |
| `border.width` | 1px |
| `shadow.panel` | None or subtle 0 4px 12px under 25% alpha |
| `motion.fast` | 120ms |
| `motion.normal` | 180ms to 220ms |
| `motion.max-ui` | 300ms |
| `z.base` | 0 |
| `z.panel` | 10 |
| `z.drawer` | 30 |
| `z.popover` | 50 |
| `z.modal` | 80 |
| `z.toast` | 90 |

Non-overridable tokens: semantic status colors, focus ring behavior, minimum target sizes, z-index order, and motion reduction behavior.

## Layout Priority Resolver

When width is constrained, resolve panels in this order:

1. Preserve active work surface minimum width.
2. Collapse Copilot unless the user is actively typing, reviewing, or confirming AI work.
3. Collapse secondary left rail.
4. Convert inspector to drawer.
5. Convert Copilot to overlay drawer or bottom sheet.
6. Hide non-critical bottom panels behind status summaries.
7. Enter focus mode for the active surface.

Rules:

- Inspector and Copilot should not both be fixed side panels if doing so pushes the viewport below its minimum width.
- Copilot may temporarily replace inspector only after user action or when an AI decision requires attention.
- Pinned panels restore when width returns, unless the user explicitly closed them.
- Breakpoint transitions should preserve user intent without trapping the user in a hidden state.
- Focus mode must keep a visible way to restore panels.

## Component-Level Accessibility

| Component | Required behavior |
| --- | --- |
| Scene hierarchy tree | Use tree semantics when hierarchy is interactive; arrow keys navigate; Enter selects; labels expose object name and type |
| Tabs and temporary chips | `role="tablist"` only for true tabs; temporary chips need close/resolve controls; active state must be announced |
| Resizable panels | `role="separator"` with orientation, current value, min/max, keyboard arrow resizing |
| Artifact cards | Card title, type, status, primary action, secondary actions in logical tab order |
| Diff viewer | Hunks keyboard navigable; accept/reject hunk buttons labeled; file path announced before hunk content |
| Command input | Label or accessible name; submission shortcut documented in tooltip/help; disabled state announced |
| Streaming assistant output | Do not spam screen readers; summarize completion via polite live region |
| Modal confirmation | Trap focus; Escape closes only when safe; destructive confirmations announce consequence |
| Evidence drawer | Focus moves to drawer heading; Escape closes; raw log regions are collapsible and labeled |
| Toast/status announcements | Use polite live region for info; assertive only for destructive failures or blocked work |
| Quest queue | List items announce title, status, owner when available, and whether action is required |
| Validation errors | Error summary uses live region; each issue links to affected file/asset/surface |

## Anti-Patterns

Do not:

- Surface internal AI process as primary navigation.
- Keep Spec, Tasks, Diagnostics, Review, and Knowledge visible when they have no content.
- Make Quest look like a separate product.
- Use permanent process bars for agent phases.
- Make the user inspect logs before deciding.
- Place the viewport behind documentation or planning UI by default.
- Use large decorative empty states in production work surfaces.
- Mix card-heavy SaaS dashboard language with desktop editor panels.
- Use color and glow to compensate for unclear information architecture.

### Bad and Good Patterns

| Bad | Good |
| --- | --- |
| Permanent `Overview / Intent / Spec / Review / Knowledge / Trace / Checkpoint / Validation` navigation | Center shows `3 changes ready`; Evidence drawer is available on demand |
| AI process timeline permanently occupies the right panel | Running state shows `Generating changes...`, Stop, context scope, and latest meaningful result |
| Empty Diagnostics tab visible in new projects | Bottom bar shows `No issues`; Diagnostics appears only when issues exist |
| Copilot fixed at 360px on narrow screens while viewport becomes unusable | Copilot collapses to rail or overlay drawer before viewport drops below minimum |
| Apply button hides command execution and dependency installation | L4/L5 permission prompt lists command, scope, duration, side effects, Stop, and restore limits |
| Quest opens as a separate three-column dashboard with equal-weight panels | Quest uses workbench grammar: queue collapsed/narrow, center decision primary, evidence drawer secondary |
| Raw trace is shown before result summary | Result summary appears first; raw trace is collapsed under Evidence |

## Implementation Checklist

### P0 Release Blockers

- Default editor opens to a creation/editing surface.
- AI write scope is visible before L3 to L5 actions.
- Stop exists while state-changing AI work is running.
- Undo/Restore exists after AI changes project state, or the UI truthfully explains why unavailable.
- Destructive or irreversible operations require high-friction confirmation.
- Error states are recoverable or explicitly terminal.
- Keyboard navigation reaches all critical controls.
- Color is not the only status indicator.
- Narrow widths do not destroy the active work surface.

### P1 Required Before Design Approval

- Permanent navigation contains only stable user surfaces.
- AI artifacts render only when they contain data entities, affect the current project, or require user intervention.
- Artifact lifecycle and archive behavior are implemented.
- Quest follows the state machine and decision-first layout.
- Evidence drawer exists for validation, conflicts, permissions, and failures.
- Context Scope can be reviewed and restricted.
- Editor and Quest share visual tokens and layout grammar.
- Component focus order matches visual order.
- Text does not overflow controls at supported widths.

### P2 Polish and Consistency

- No emoji icons are used.
- Fallback icons follow this document.
- No decorative nested cards are introduced.
- Diff/code/detail containment uses approved structural exceptions.
- Colors follow semantic roles.
- Color and glow usage follows quantified limits.
- Hover, active, disabled, and focus states use shared tokens.
- Empty states remain compact and actionable.

### P3 Future Hardening

- Collaboration provenance is displayed for shared AI actions.
- Evidence search/filtering exists for long logs.
- Multi-device Quest resume has explicit UI tests.
- Comfortable/touch density variants are defined if non-desktop targets are introduced.
