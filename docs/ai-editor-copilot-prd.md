# Aster AI Editor Copilot PRD

## Problem Statement

Aster is growing into a full game-making engine, but authoring scenes, assets, components, scripts, and editor workflows still requires users to know the engine's data model and tool layout in detail. The repository already contains the first AI agent foundation through `engine-ai`, the `agent-tools` feature profile, editor command registration, sandbox policy contracts, worktree tracking, transactions, and trace records. These pieces are not yet shaped into a user-facing Copilot that can safely help developers build games inside the editor.

Users need an AI assistant that understands an open Aster project, can propose useful game-making changes, and can apply those changes through safe editor tools. The assistant must be powerful enough to create objects, modify components, write scripts, inspect project files, and run editor commands, while still respecting undo, permissions, sandboxing, diagnostics, and user approval.

## Solution

Build an Editor Copilot for Aster behind the `agent-tools` and `editor` feature profiles. The Copilot will live in the editor experience and act as a development assistant for the project author. It will not be a runtime gameplay AI system.

The Copilot offers two modes of operation. **Copilot mode** is the interactive default: a single Agent reads project context, proposes changes, and executes tool calls directly, with per-operation user approval (or an auto-accept toggle for routine operations). **Auto mode** targets larger authoring requests and enterprise workflows where many operations must be completed by agents without per-step human approval: a Manager decomposes the request into minimal tasks, Workers execute them in parallel inside a git-backed task workspace, policy automatically reviews and issues task-bound permissions, peer agents review quality and risk, a Deep Reviewer inspects the integrated result, and the user receives a final report with diffs, validation results, review findings, unresolved problems with quick-fix actions, and risk assessment. Both modes share the same safety infrastructure — policy, sandbox, validators, transaction engine, trace — but differ in orchestration depth and user-interruption cadence.

The Copilot will read structured project context, ask a model for a plan and tool calls, validate those tool calls against a permission policy, execute approved changes through editor services, and record every operation in a trace. User-visible changes will be previewable, undoable, and recoverable. The active project must never be modified directly by AI agents. All AI-generated changes are draft proposals until they pass validation, review, and explicit user approval. The desired workflow should feel like a local pull request system integrated into the editor.

## Modes of Operation

The Copilot surface presents two distinct modes. They share the same safety infrastructure (policy, sandbox, validators, transaction engine, trace), but differ in orchestration depth, user involvement, and agent topology.

### Copilot Mode (Interactive Single-Agent)

Copilot mode is the default interactive experience. A single Agent receives the user request, reads project context, proposes changes, and executes tool calls directly. No Manager/Worker/Reviewer cluster stands between the user and the result.

- The Agent calls tools directly; there is no decomposition into Workers.
- Every write operation requires user permission approval before execution.
- The user may enable **auto-accept** for the current session, which pre-approves low-risk and medium-risk tool calls within the declared task scope. High-risk operations still require step-up confirmation.
- Auto-accept is a user-facing convenience toggle, not a policy bypass. Deterministic policy, sandbox checks, and validators still reject out-of-scope or unsafe calls regardless of auto-accept state.
- The Agent may propose a plan before acting, but plan review is lightweight compared to the Auto mode cluster pipeline.
- Changes are previewable, undoable, and traceable per operation.
- Suitable for: quick questions, scene inspection, single-entity edits, script creation, component tweaks, and workflows where the user wants tight control.

### Auto Mode (Agent Cluster)

Auto mode targets L4 automation for larger authoring requests. The user states an outcome; the system decomposes, executes, validates, reviews, repairs, and bundles — interrupting the user only for meaningful product choices, high-risk escalation, blocked outcomes, or final approval.

- A **Manager** agent takes the request and creates an immutable context snapshot and isolated git-backed task workspace.
- The Manager decomposes the request into the smallest independently reviewable tasks and assigns them to specialized **Workers**.
- Workers execute in parallel inside the sandboxed task workspace with automatically issued, task-bound capability grants. Some enterprise tasks require broad read/write or command access to be useful; broad grants are allowed only when they are bound to task scope, workspace, time, evidence requirements, risk class, trace, and rollback. The user is not asked to approve individual Worker grants.
- Worker outputs pass through **local review** before integration. The Manager merges approved outputs into an integration candidate.
- Deterministic validators and a **Deep Reviewer** inspect the integrated result. If validation or review fails, a scoped **repair ticket** is created and a Repair Worker patches the candidate.
- **Problems are not silently skipped.** If a Worker encounters an unrecoverable failure, an out-of-scope need, or an ambiguous condition, it must stop and report to the Manager. The Manager decides whether to reassign, narrow scope, request escalation, or mark the task blocked. Every blocked or partially-completed task appears in the final report.
- The Manager generates a **final report** summarizing all changes, validation results, review findings, repaired issues, and **unresolved problems with quick-fix actions**. The user can act on unresolved problems directly from the report — re-triggering a scoped fix without replaying the entire request.
- To avoid context pollution, the Manager may spawn **fresh sessions** for individual Workers, Repair Workers, and the Deep Reviewer. A fresh session receives only the role-specific context packet (task brief, allowed scope, relevant snippets, evidence references) and carries no chat history from the Manager or sibling Workers.
- The user reviews the final report, diffs, previews, and risks, then approves, rejects, partially accepts, or requests further revision.
- Suitable for: multi-file scene + script authoring, asset pipeline changes, cross-cutting refactors, and any task too large for a single Agent round-trip.

### Mode Selection

- The Copilot surface defaults to Copilot mode.
- The user may switch to Auto mode explicitly (e.g., "auto: build a third-person camera controller with input handling") or the system may suggest Auto mode when the request spans multiple artifact boundaries, tools, or risk classes.
- Both modes produce trace records, support undo/rollback, and respect the same safety invariants. The difference is in orchestration depth and user-interruption cadence, not in authority over the active project.
- Read-only questions always run in a lightweight single-agent path regardless of mode selection.

The long-term target is L4 editor automation: the user states an outcome, the system independently plans, decomposes, executes, validates, repairs, and prepares an applyable bundle, and the user is interrupted only for meaningful product choices, high-risk approval, rejected/blocked outcomes, or ambiguity that cannot be resolved from project context. L4 does not mean unattended authority over the active project. It means high autonomy inside bounded, observable, reversible workspaces, with deterministic gates and final human control over trusted-state mutation.

The system must avoid both single-agent fragility and process-heavy enterprise workflow. Authority should be separated across deterministic policy, capability grants, sandboxed tools, validators, reviewers, transaction bundling, and user approval. No single model, Worker, Manager prompt, plugin, command label, reviewer report, or user click can grant itself enough authority to bypass the rest of the chain. At the same time, this separation must remain mostly invisible to the user: routine worker routing, permission sizing, validation, review, repair, rebasing, and trace collection should run automatically behind a compact Copilot surface.

The first useful version should focus on project authoring workflows:

- Explain the current scene, selected entity, assets, and scripts.
- Create and modify scene objects and components.
- Create or update `.aster` Aster Script files in the project asset root.
- Execute registered editor commands through the existing command registry.
- Read project files through a sandboxed read tool.
- Present a plan before applying write operations.
- Apply changes only after validation, review, and explicit user approval.
- Record trace entries, diagnostics, and recovery hints for each operation.

The orchestration model should evolve toward an Agent cluster rather than a single monolithic assistant. A Manager agent owns request intake, immutable context snapshot creation, AI task workspace creation, planning, architecture decisions, worker selection, permission scoping, task handoff, integration, and final report generation. Specialized Worker agents execute bounded tasks such as scene editing, asset inspection, script generation, diagnostics analysis, explanation, or scoped repair under policy-issued capabilities requested or routed by the Manager. Workers should execute in a sandbox by default with task-appropriate access and should make write changes in the isolated task workspace rather than directly mutating the active project mainline. "Least privilege" means least privilege that can still complete the assigned enterprise task; it does not mean artificially tiny grants that force the user to supervise every useful operation.

The Manager is an orchestrator, not a root authority. It may request scoped grants and route work, but deterministic policy code and the capability issuer decide which tools, files, commands, operation types, and risk levels are allowed. This prevents the Manager from becoming a single point of compromise while preserving one coherent user-facing workflow.

The Manager itself is also untrusted AI output when it uses a model. Manager plans, decompositions, capability requests, worker assignments, merge decisions, final reports, and risk summaries must be reviewed as artifacts. They are checked by deterministic policy, capability issuance rules, scope validators, independent Reviewer/Risk Auditor passes where appropriate, and immutable trace comparison against actual operations. A Manager claim is never evidence that work is safe, complete, or in scope.

Worker outputs pass through a local review before they can be integrated. The Manager merges approved worker outputs into an integration candidate, runs deterministic validation, then sends the integrated result to a Deep Reviewer. The Deep Reviewer reviews the whole integrated result, not individual worker drafts. If validation or review fails, the Manager creates a scoped repair ticket and assigns a Repair Worker to patch the integration candidate. The original Deep Reviewer verifies the repair to preserve review context. Only after validation, deep review, final report generation, and user approval may changes enter the active project through an editor transactional merge with undo and rollback support.

AI write workflows require a git-backed task workspace. If git is not installed, or the active project is not initialized as a git repository, the Copilot should keep read-only assistance available but block write-capable agent execution and guide the user through installing git or initializing project version control. This keeps the task isolation model simple, auditable, and familiar while still allowing non-mutating explanation workflows.

### Copilot Mode Workflow

1. User request.
2. Agent reads project context (scene, selection, assets, scripts, diagnostics).
3. Agent proposes a plan and lists intended operations.
4. User reviews the plan. If auto-accept is enabled, low-risk and medium-risk operations within scope proceed without per-operation prompts; high-risk operations still require step-up confirmation.
5. Agent executes approved operations directly through the tool layer.
6. Every operation is validated by deterministic policy, sandbox checks, and the capability system before execution.
7. Results, diagnostics, and trace entries are produced per operation.
8. Applied changes are previewable, undoable, and recoverable through editor transactions.
9. Trace log records all tool calls, results, and recovery hints.

### Auto Mode (Agent Cluster) Workflow

1. User request with an outcome description.
2. Session orchestrator starts an AI task in Auto mode.
3. Manager creates an immutable project context snapshot.
4. Manager verifies git availability and repository initialization; blocks write-capable work with setup guidance if missing.
5. Manager creates an isolated git-backed task workspace with a stable branch convention (e.g., `ai/task-0001`).
6. Manager decomposes the request into the smallest independently reviewable tasks.
7. Manager requests capability grants from deterministic policy for each Worker; grants are issued automatically when policy can prove task binding, workspace binding, evidence requirements, risk handling, and rollback. The user is not asked to approve individual Worker permissions.
8. Workers execute in parallel inside the sandboxed task workspace, each in a **fresh session** carrying only its role-specific context packet. Workers do not share chat history, sibling Worker context, or Manager reasoning.
9. Worker outputs receive local review (approved / needs_revision / blocked).
10. Manager merges approved outputs into an integration candidate.
11. Deterministic validators run against the integrated result. Failures create scoped repair tickets.
12. Deep Reviewer reviews the whole integrated result in a fresh session with only the review rubric, task briefs, accepted artifacts, validator output, and evidence references.
13. Repair loop runs when validation or review fails. Repair Workers receive fresh sessions with only the repair ticket, failing evidence, and integration candidate. Retry cap: 3 cycles.
14. **Problems are not skipped.** Unrecoverable failures, out-of-scope needs, ambiguous conditions, and exhausted retries are reported to the Manager and appear in the final report as unresolved issues with quick-fix actions.
15. Manager generates the final task report: summary, changes, logical change groups, validation results, review findings, repaired issues, unresolved problems with quick-fix actions, risk assessment, and traceability.
16. User reviews diff, previews, validation summary, review findings, risks, and unresolved problems.
17. User may approve, reject, partially accept, request further revision, or trigger a quick-fix for a specific unresolved problem.
18. Approved changes merge into the active project through editor transactions.
19. Transaction bundle supports undo and rollback without requiring git knowledge.

## User Stories

1. As a game developer, I want to ask the Copilot what is in my current scene, so that I can understand the project quickly.
2. As a game developer, I want the Copilot to summarize the selected entity and its components, so that I can reason about a specific object without manually scanning the inspector.
3. As a game developer, I want the Copilot to create a player object with common components, so that I can bootstrap gameplay faster.
4. As a game developer, I want the Copilot to create a camera and light setup, so that a new scene becomes usable quickly.
5. As a game developer, I want the Copilot to add or remove components from an entity, so that routine scene setup takes less manual work.
6. As a game developer, I want the Copilot to modify component fields, so that I can tune values through natural language.
7. As a game developer, I want the Copilot to write an Aster Script asset, so that simple gameplay behavior can be generated from a description.
8. As a game developer, I want the Copilot to update an existing script, so that I can iterate on behavior without manually editing every line.
9. As a game developer, I want script changes to stay under the project asset root, so that generated code cannot write outside the project.
10. As a game developer, I want the Copilot to reference existing asset paths, so that generated components point to real project resources.
11. As a game developer, I want the Copilot to use exact entity identifiers from the scene context, so that changes apply to the intended objects.
12. As a game developer, I want the Copilot to explain what it is about to change, so that I can approve or reject the plan.
13. As a game developer, I want proposed changes grouped into a preview, so that I can understand their scope before applying them.
14. As a game developer, I want to apply approved changes with one action, so that Copilot work feels integrated with the editor.
15. As a game developer, I want every applied change to enter undo or an equivalent transaction, so that I can recover from bad output.
16. As a game developer, I want failed operations to appear in the console diagnostics, so that I know what went wrong.
17. As a game developer, I want the Copilot to continue after recoverable tool failures, so that one bad operation does not discard the whole session.
18. As a game developer, I want a trace log of tool calls and results, so that I can audit what the Copilot did.
19. As a game developer, I want recovery hints in the trace, so that I know how to undo, discard, or fix a failed action.
20. As a game developer, I want read-only questions to avoid requesting write permission, so that asking for explanations feels low-risk.
21. As a game developer, I want write operations to require an explicit write policy, so that the assistant cannot silently mutate my project.
22. As a game developer, I want direct AI writes to the active project to be disallowed, so that the active project is protected.
23. As a game developer, I want every AI write task to happen away from the active project, so that larger AI changes can proceed while I keep working.
24. As a game developer, I want the Copilot to refuse unsupported operations, so that it does not pretend to edit systems it cannot safely change.
25. As a game developer, I want the Copilot to use registered editor commands where possible, so that behavior remains consistent with the rest of the editor.
26. As a game developer, I want the Copilot panel to show chat, plan, changes, apply controls, and trace, so that the workflow is visible in one place.
27. As a game developer, I want the Copilot to support local and hosted model providers through adapters, so that I can choose a model setup.
28. As a game developer, I want provider credentials to live outside project files, so that secrets are not committed.
29. As a game developer, I want model responses parsed as structured operations, so that the engine does not rely on arbitrary prose.
30. As a game developer, I want malformed model output to be rejected with a useful diagnostic, so that failures are understandable.
31. As an engine maintainer, I want Copilot tools to be tested through deterministic model stubs, so that CI does not depend on a live provider.
32. As an engine maintainer, I want tool execution to be isolated behind simple interfaces, so that provider, planning, validation, and editor mutation can evolve independently.
33. As an engine maintainer, I want feature gates to keep Copilot out of minimal runtime builds, so that `runtime-min` remains lean.
34. As an engine maintainer, I want Copilot operations to respect the existing project context and command registry, so that agent functionality does not fork the editor architecture.
35. As an engine maintainer, I want sandbox path checks to use canonical project roots, so that path traversal and symlink mistakes are avoided.
36. As an engine maintainer, I want process and network execution disabled for the first user-facing version, so that the security surface stays narrow.
37. As an engine maintainer, I want a clear boundary between Editor Copilot and runtime gameplay AI, so that the feature scope does not blur.
38. As an engine maintainer, I want a Manager agent to split complex requests into bounded tasks, so that planning, execution, and review do not become a single fragile prompt.
39. As an engine maintainer, I want Worker agents to receive only the permissions and context needed for their assigned task, so that one bad worker output cannot exceed its intended scope.
40. As an engine maintainer, I want handoff records between Manager, Workers, and Reviewer, so that task state can move between agents without losing assumptions, decisions, or trace links.
41. As an engine maintainer, I want a Reviewer agent to block work before it reaches the user-approved mainline, so that incomplete, unsafe, or off-spec changes are caught early.
42. As an engine maintainer, I want Reviewer rejections to return to the original responsible Worker when possible, so that fixes reuse the worker's accumulated task context instead of starting from scratch.
43. As a game developer, I want the Copilot to explain when work is under review or has been sent back for revision, so that longer AI-assisted changes remain understandable.
44. As an engine maintainer, I want Worker agents to execute inside a sandbox with task-appropriate permissions, so that each task can receive enough authority to complete while remaining bound to scope, workspace, evidence, trace, and rollback.
45. As a game developer, I want Worker write tasks to happen in an isolated task workspace, so that failed or rejected AI work cannot directly pollute my active project.
46. As an engine maintainer, I want only reviewed and user-approved changes to be promoted from an isolated worker area into the mainline project state, so that the trunk remains protected.
47. As a game developer, I want the Copilot to create an immutable snapshot before agent work starts, so that workers do not read from a live project state that changes while I continue editing.
48. As an engine maintainer, I want every AI write task to use a dedicated git-backed isolated task workspace, so that agent execution behaves like a local pull request.
49. As an engine maintainer, I want worker output reviewed locally before integration, so that blocked or unsafe partial results cannot enter the integrated candidate.
50. As an engine maintainer, I want deterministic validators to run before deep review, so that build, schema, asset, and dependency failures produce repair tickets without relying on reviewer debate.
51. As an engine maintainer, I want deep review to inspect the integrated candidate rather than isolated worker outputs, so that final quality is judged on the result the user might actually apply.
52. As an engine maintainer, I want repair tickets with severity, affected files, reproduction details, expected outcomes, allowed repair scope, and retry counts, so that repair work stays bounded and traceable.
53. As an engine maintainer, I want repair cycles capped, so that repeatedly failing work becomes a blocked outcome with an escalation report.
54. As a game developer, I want the final report to include summary, changes, logical change groups, validation results, review findings, risk assessment, and traceability, so that I can make an informed approval decision.
55. As a game developer, I want approval controls to support approve, reject, partially accept, and request further revision, so that I can keep useful parts without accepting the entire AI proposal.
56. As a game developer, I want approved merges to use editor transactions instead of branch checkout or force operations, so that editor state, asset database integrity, and undo remain consistent.
57. As a game developer, I want the Copilot to detect when git is missing or the project is not initialized as a git repository, so that it can guide me through the required setup before enabling AI write workflows.
58. As a game developer, I want read-only Copilot help to remain available before git setup is complete, so that I can still ask questions while write-capable workflows are disabled.
59. As an engine maintainer, I want model output, project files, third-party scripts, plugin metadata, Worker reports, and Reviewer reports treated as untrusted input, so that no prompt injection source can change policy, tools, permissions, task scope, or approval requirements.
60. As an engine maintainer, I want every Worker action bound to a policy-issued capability grant requested or routed by the Manager, so that Workers can only execute in the assigned workspace with the assigned tools, commands, files, entities, assets, and operations.
61. As an engine maintainer, I want Workers to request capability escalation through the Manager when they need access outside their grant, so that permission changes remain explicit, minimal, reviewed, and traceable.
62. As an engine maintainer, I want Reviewer agents to evaluate objective artifacts instead of Worker claims, so that completion, safety, and scope compliance are proven by diffs, validators, audits, and diagnostics.
63. As an engine maintainer, I want high-risk commands and third-party scripts to pass deterministic audit and optional secondary model risk audit, so that suspicious side effects, prompt injection, and unsafe behavior are flagged before user review.
64. As a game developer, I want user approval to apply only to already validated and reviewed transaction bundles, so that a mistaken or manipulated approval cannot override hard safety failures.
65. As a game developer, I want high-risk approvals to require step-up confirmation tied to the immutable bundle, so that destructive or security-sensitive changes cannot be accepted through a misleading generic approve button.
66. As a game developer, I want to describe the outcome I want instead of managing agents, grants, repair loops, or validation steps, so that Copilot work does not become another project-management task.
67. As a game developer, I want the Copilot to ask me questions only when the answer affects game intent, cost, security, reversibility, or final approval, so that routine implementation details stay automated.
68. As a game developer, I want routine safe repairs to happen automatically inside the task workspace, so that I only see the final result or a concise blocked report.
69. As a game developer, I want the final review surface to compress internal agent activity into a clear summary, diff, preview, risks, and decisions, so that I do not have to audit every internal handoff unless I choose to.
70. As an engine maintainer, I want authorization to be split between Manager orchestration, deterministic policy, capability issuance, sandbox enforcement, validators, reviewers, and transaction application, so that compromising one layer does not give full project-write authority.
71. As an engine maintainer, I want the Manager to request capabilities rather than mint unrestricted permissions, so that the orchestrator cannot become a single point of privilege escalation.
72. As an engine maintainer, I want the system to have an explicit friction budget for user prompts, approvals, and review surfaces, so that safety mechanisms do not grow into slow, bureaucratic workflows.
73. As an engine maintainer, I want L4 automation to be measured by autonomous task completion inside bounded workspaces, not by bypassing user approval or deterministic validation, so that autonomy and zero trust remain compatible.
74. As an engine maintainer, I want Manager plans, grants, routing decisions, merge decisions, and final reports to be treated as reviewable artifacts, so that the Manager cannot become an unchecked root agent.
75. As an engine maintainer, I want auto mode to fail closed when policy, validation, audit, review, stale-context checks, or rollback planning are incomplete, so that automation cannot convert uncertainty into active-project mutation.
76. As a game developer, I want auto mode to clearly state its safety boundary and residual risk, so that I do not mistake bounded automation for an absolute safety guarantee.
77. As a game developer, I want the Copilot to turn broad intent into a PRD, task plan, and implementation tasks when needed, so that ambiguous work becomes reviewable before execution.
78. As an engine maintainer, I want generated PRDs and task plans treated as untrusted planning artifacts, so that they can guide work without granting authority or bypassing policy.
79. As an engine maintainer, I want the Manager or Planner to split work into the smallest independently reviewable tasks, so that each Worker receives minimal context, minimal permissions, and explicit acceptance criteria.
80. As an engine maintainer, I want every task to include objective, non-goals, allowed scope, forbidden scope, acceptance criteria, review rubric, required evidence, and repair policy, so that review is explicit instead of subjective.
81. As an engine maintainer, I want context packets to be generated per task from immutable snapshots and trust-labeled summaries, so that prompt injection and cross-task context pollution do not spread.
82. As a game developer, I want the Copilot to auto-correct routine failures inside scoped repair limits, so that I am only interrupted for product choices, high-risk escalation, blocked work, or final approval.
83. As a game developer, I want to choose between Copilot mode (interactive, per-operation approval) and Auto mode (cluster-driven, outcome review), so that I can match the workflow to the task size and my desired level of control.
84. As a game developer, I want Copilot mode to let me enable auto-accept for the current session, so that routine low-risk operations proceed without per-click approval while high-risk changes still require confirmation.
85. As a game developer, I want Auto mode to run Workers automatically with policy-issued permissions, so that I do not have to approve individual file reads, tool calls, or Worker grants.
86. As a game developer, I want Auto mode to report every unresolved problem in the final report with a suggested quick-fix action, so that I know what failed and can address it with one click instead of restarting the entire request.
87. As a game developer, I want quick-fix actions to launch a scoped repair task that reuses the original snapshot and workspace, so that fixing a reported problem is fast and does not re-execute unrelated work.
88. As an engine maintainer, I want Auto mode Workers, Repair Workers, Reviewers, and Risk Auditors to execute in fresh sessions with only their role-specific context packet, so that context pollution and prompt injection do not spread across agents.
89. As an engine maintainer, I want fresh sessions to exclude Manager conversation history, sibling Worker chat, prior repair reasoning, and raw user prompt text, so that each agent role evaluates only the evidence it needs.
90. As a game developer, I want read-only questions to run in a lightweight single-agent path regardless of mode, so that asking about my scene never spawns an unnecessary cluster.
91. As a game developer, I want to import a third-party skill from a file with YAML frontmatter, so that community-authored Copilot extensions can guide my editor workflows.
92. As an engine maintainer, I want third-party skill frontmatter to be deterministically validated by the Capability Issuer (tool names, path scopes, trust tier consistency, content hash), while the instruction body is injected into the agent context under an explicit untrusted label and never treated as policy, so that a skill's prose cannot override tool permissions, validation requirements, or system instructions.
93. As an engine maintainer, I want MCP servers to run in seccomp-ed subprocesses with network denied by default and filesystem access restricted to declared paths, so that external MCP processes cannot escape their sandbox.
94. As a game developer, I want skill failures (the agent following a skill's instructions produces invalid output, goes out of scope, or gets confused) to result in a structured problem report with a quick-fix action rather than silently producing wrong results, so that one bad skill does not corrupt the task.
95. As an engine maintainer, I want skill instruction text to be isolated from system policy by context boundary markers and post-injected system prompts, so that a skill cannot instruct the agent to override its safety constraints.
96. As an enterprise user, I want Auto mode to run broad multi-file and command-heavy tasks without asking me to approve every intermediate permission, so that agent automation remains useful for real production workflows.
97. As an enterprise admin, I want broad agent permissions to be task-bound, workspace-bound, time-bound, evidence-bound, risk-classified, traceable, and revocable, so that useful automation does not become permanent or global authority.
98. As an enterprise admin, I want permission and command requests to be automatically reviewed by deterministic policy and agent reviewers, so that most work can proceed without human interruption while risky work is escalated consistently.
99. As an engine maintainer, I want peer agents to review plans, permission requests, commands, diffs, and risk reports, but never mint permissions themselves, so that agent-to-agent review improves judgment without becoming an authorization root.
100. As a game developer, I want Auto mode to treat broad permissions as an implementation detail and show me only the resulting bundle, validation evidence, risk summary, and unresolved decisions, so that I do not have to manage the agent cluster.
101. As an enterprise admin, I want critical operations to route through organization policy or human approval while low-risk and medium-risk operations proceed automatically, so that governance matches the consequence of the action.

## L4 Automation And Friction Budget

L4 automation for the Editor Copilot means autonomous execution of a bounded editor task from intent to reviewed transaction bundle. The user should not need to choose Worker types, grant individual low-risk file reads, approve routine tool calls, inspect every retry, resolve ordinary merge mechanics, or understand git. The Copilot should absorb implementation complexity and surface only product intent, meaningful tradeoffs, final review, and exceptional risk.

Auto mode must not be described as absolutely safe. The product guarantee is narrower: auto mode can run only within explicit scope, isolated task workspaces, task-appropriate capability grants, deterministic validators, monotonic audits, review gates, immutable bundles, transactional apply, and rollback. If any required gate cannot prove its condition, the result is blocked or requires user review. The system should optimize for "safe by construction and fail closed," not "safe because the AI says so."

For enterprise workflows, "least privilege" must be interpreted as **least authority that can complete the task without turning the user into an operator for the agent cluster**. Many useful tasks require broad project reads, multi-file writes, importer execution, build or test commands, and iterative repair. Blocking those capabilities by default would make Auto mode safe but useless. The system should allow broad task-local authority while refusing permanent, global, unbounded, or active-project authority.

### Enterprise Dynamic Authorization

Enterprise Auto mode uses dynamic authorization instead of static tiny grants. A Worker may receive broad permissions when the Capability Issuer can bind those permissions to:

- `task_id`, `snapshot_id`, `base_revision`, `scope_hash`, `workspace_id`, and `grant_hash`.
- A declared objective, non-goals, acceptance criteria, required evidence, and rollback plan.
- An isolated task workspace rather than the active project.
- Expiration, step limits, retry limits, and revocation rules.
- Risk classification and required review route.
- Trace logging for every tool call, command, grant decision, escalation, review, and repair.

Broad grants are valid only inside the task workspace and only for the declared task. A broad read grant for `assets/**` does not imply write access. A broad write grant in the task workspace does not imply active-project mutation. A command grant to run a validator does not imply shell access. A permission granted to one Worker does not transfer to sibling Workers, Repair Workers, Reviewers, MCP servers, or later tasks.

Permission requests are automatically reviewed through a split-authority pipeline:

1. **Agent proposes.** The Manager or Worker emits a structured permission or command request with necessity, expected artifacts, scope, alternatives, risk tags, and rollback expectations.
2. **Policy decides mechanically.** The Capability Issuer validates schema, task binding, path scope, command identity, risk class, workspace isolation, credential rules, and rollback requirements. It may approve, narrow, deny, or require escalation.
3. **Peer agents review judgment.** Reviewer or Risk Auditor agents may inspect sanitized evidence to identify task drift, suspicious intent, overbroad access, unsafe command semantics, or missing evidence.
4. **Review is monotonic.** Peer agents may raise risk, request narrower scope, recommend blocking, or require confirmation. They cannot mint grants, lower deterministic risk, approve blocked commands, or bypass policy.
5. **Execution remains bounded.** Worker tool calls must carry the active grant hash and pass tool-layer enforcement before each operation.
6. **Apply remains separate.** No grant, however broad, can apply changes to the active project. Only an immutable reviewed transaction bundle can be applied through the editor transaction engine.

This model lets agents do substantial work without per-call human approval while preserving zero-trust authority boundaries. The enterprise promise is not "the agent has little permission." It is: **the agent may have large task-local permission, but every permission is bounded, reviewed, logged, revocable, and unable to directly mutate trusted state.**

### Agent-to-Agent Review

Agent-to-agent review is a judgment layer, not an authorization layer. It is used because deterministic policy cannot fully understand intent, architectural quality, semantic drift, command consequence, or whether a broad permission request is proportional to the task.

The review topology should include:

- **Manager review** of Worker requests, blocked outcomes, escalation needs, and integration consistency.
- **Local Reviewer** review of individual Worker outputs before integration.
- **Deep Reviewer** review of the integrated candidate against the task brief and evidence.
- **Risk Auditor** review of high-risk commands, scripts, dependency changes, process execution, network access, credential usage, and suspicious broad grants.
- **Optional adversarial reviewer** for enterprise policy tiers that require a second model or second prompt template to challenge the plan, command request, or final bundle.

Peer agents can disagree. Disagreement does not create authority by majority vote. If reviewers conflict, the system chooses the safer monotonic outcome: raise risk, narrow scope, request more evidence, send back for repair, escalate to organization policy, or mark the task blocked. Agent quorum can improve confidence, but it cannot bypass deterministic policy or user/organization approval for critical operations.

### Enterprise Risk Routing

Auto mode should route decisions by consequence:

- **Low risk:** automatically grant and execute inside the task workspace; include in trace and final report.
- **Medium risk:** automatically grant when evidence and rollback are complete; require local review and final bundle approval.
- **High risk:** require deterministic audit, Risk Auditor review, Deep Reviewer review, and step-up confirmation bound to the immutable bundle.
- **Critical risk:** require organization policy approval or an explicit human approver before execution or apply, depending on enterprise configuration.

Critical risk includes credential changes, production publishing, destructive deletion of large asset trees, dependency or build-system changes with supply-chain impact, network-capable third-party execution, command grants that can mutate outside the task workspace, and any operation where rollback cannot be proven.

### Problem Reporting and Quick-Fix

Auto mode must not silently skip or paper over problems. Every unrecoverable failure, out-of-scope need, ambiguous condition, exhausted retry, or blocked task must appear in the final report as an **unresolved issue**. Each unresolved issue includes:

- A concise description of what failed and why.
- The affected files, entities, assets, or commands.
- Severity (blocking / non-blocking / advisory).
- A suggested **quick-fix action** that the user can trigger from the report — for example, "Regenerate this script with corrected imports," "Re-run asset import with updated path," or "Remove the orphaned component."
- Whether the issue requires user clarification (product intent, ambiguous choice) or can be retried with adjusted parameters.

Quick-fix actions launch a scoped Auto mode task limited to the unresolved issue. They reuse the original snapshot and task workspace where possible, and produce a new mini integration candidate and review pass. Quick-fix results are appended to the original final report.

### Fresh Sessions and Context Pollution Control

Auto mode Workers, Repair Workers, the Deep Reviewer, and the Risk Auditor each execute in a **fresh session**. A fresh session receives only:

- The role-specific context packet (task brief, allowed scope, acceptance criteria, review rubric).
- Relevant evidence references (diffs, validator output, audit output, scene previews).
- Trust-labeled summaries of necessary upstream decisions — never raw chat history, sibling Worker reasoning, or Manager internal deliberation.

A fresh session must not carry:
- The Manager's full conversation or planning rationale.
- Other Workers' tool-call history, self-reports, or rejected drafts.
- Prior repair attempts' internal reasoning.
- The user's original prompt text beyond what the task brief normalizes.
- Any model output from a prior session unless explicitly included as trust-labeled untrusted evidence.

Session boundaries are a context-isolation mechanism. They prevent cross-task prompt pollution, reduce drift, and ensure each role evaluates only the evidence it needs. The Manager is the sole integrator of fresh-session outputs; Workers and Reviewers do not communicate directly.

### Trust-Level Scoping

L4 automation must be scoped by trust level:

- Read-only and explanation work should run with minimal friction and no write approval (Copilot mode, single-agent path).
- Low-risk write work should run autonomously in an isolated task workspace after a clear task brief and should ask the user only for final bundle approval (Auto mode).
- Medium-risk work may receive broad task-local access when policy can prove scope, evidence, and rollback; it may require local review, compact plan confirmation, and final approval before apply (Copilot mode with auto-accept, or Auto mode with plan preview).
- High-risk work requires deterministic audit, peer-agent risk review, deep review, and step-up confirmation bound to the immutable bundle (either mode).
- Critical-risk work requires organization policy or explicit human approval before execution or apply, depending on configured enterprise governance.
- Blocked work should produce a concise escalation report with the smallest useful user decision and quick-fix actions, not expose internal agent debate by default.

The user-facing workflow should optimize for outcome review rather than process supervision. Internal steps such as decomposition, Worker assignment, capability narrowing, local review, validation, repair, and trace capture are required for safety, but they should appear as progress states and expandable details, not mandatory user tasks. The default review screen should answer: what changed, why it changed, what validation proved, what risks remain, what will be applied, and how to undo it.

The Copilot should maintain a friction budget that varies by mode:

**Copilot mode friction budget:**
- Ask at most one clarification round before starting unless the requested outcome is unsafe or underspecified in a way that would produce likely wrong work.
- Present a compact plan before executing write operations. With auto-accept enabled, skip per-operation prompts for low-risk and medium-risk tool calls within declared scope.
- Require step-up confirmation only for risk classes that policy marks high-risk, even with auto-accept enabled.
- Prefer automatic safe defaults over exposing internal configuration.
- Preserve full traceability in expandable logs for audit, debugging, and maintainer review.

**Auto mode friction budget:**
- Ask at most one clarification round before starting. Do not ask for clarification on implementation details the system can resolve from project context.
- Do not ask the user to approve individual Worker grants, routine sandbox reads, deterministic validators, local repair attempts, or low-risk formatting changes. Permissions are issued automatically by policy.
- Batch all approved changes into one reviewed transaction bundle for final approval.
- Do not expose internal agent debate, handoff details, or repair-loop internals by default. Surface only the compressed summary: what changed, why, what was validated, what was reviewed, what problems remain, and what risks exist.
- Require step-up confirmation only for risk classes that policy marks high-risk.
- Unresolved problems must be visible in the final report with clear quick-fix actions — the user should not need to read trace logs to understand what failed and how to fix it.
- Preserve full traceability in expandable logs for audit, debugging, and maintainer review.

The system should treat excessive process complexity as a product risk. If safety requires many visible steps, the design should first look for stronger deterministic defaults, narrower capabilities, better previews, or better rollback, before adding more user-facing approvals.

## Deterministic Rules And The Gray Area

The safety architecture has two tiers: deterministic rules that cover the clear cases, and AI judgment that handles the rest. The system must be honest about the boundary.

### What Deterministic Rules Cover (Non-Negotiable)

- Path sandboxing (canonical roots, parent traversal rejection, symlink resolution).
- Command capability registry (is this command `ai_safe`? what risk level? what contract?).
- Static script audit (does this script call `std::process::Command`? does it access the network?).
- Capability grant enforcement (does this Worker's grant hash cover this tool call?).
- Schema validation (is the operation JSON well-formed? are required fields present?).
- Snapshot binding (is this action bound to the current `snapshot_id` and `scope_hash`?).
- Transaction integrity (does the bundle hash match? is the rollback journal complete?).

These are enforced by trusted Rust code. No AI agent, user, or combination of both can waive them.

### What Deterministic Rules Cannot Cover (The Gray Area)

Deterministic rules are blind to intent, quality, and novelty:

- Is this generated script correct for the gameplay behavior the user wants? (Rules check safety, not correctness.)
- Are these two Worker outputs architecturally consistent with each other? (Rules check scope, not coherence.)
- Is this scene change a reasonable interpretation of the user's request, or did the Worker drift? (Rules check entity IDs, not semantic intent.)
- The Worker needs a file outside its grant but the file is clearly related to the task — should we expand the grant or block? (Rules enforce the current grant; they don't judge whether the grant should be different.)
- The Deterministic Static Auditor flagged a script as medium risk for using `eval`-like patterns, but it's a legitimate Rhai `call_fn` for a plugin the user installed. (Rules flag patterns; they don't understand project-specific legitimacy.)
- The Deep Reviewer flagged inconsistent naming between two scripts, but the user's project already uses both conventions. (Rules don't know project conventions.)
- A Worker produced three approaches for the same sub-task — which one best matches the user's unstated preferences? (Rules don't know user taste.)

These are judgment calls. They require understanding of intent, context, conventions, and tradeoffs.

### Who Handles the Gray Area

The gray area is split across three AI roles, each with bounded authority and escalating when uncertain:

**Manager** — handles task-level gray areas during decomposition and integration:
- Whether a Worker's out-of-scope need is legitimate enough to request a grant expansion from the Capability Issuer.
- Whether two approved Worker outputs are semantically consistent or need reconciliation.
- Whether a repair failure means "try differently" or "this task is fundamentally blocked."
- Whether the original task brief was underspecified and needs narrowing.
- **Limit:** The Manager may request narrower or expanded scope from the Capability Issuer, but the Issuer still decides. The Manager may choose between Worker outputs, reassign work, or mark a task blocked. It may NOT declare an unsafe script safe, bypass a blocked audit, or merge unreviewed outputs.

**Deep Reviewer** — handles quality and coherence gray areas during integrated review:
- Whether the integrated result is architecturally consistent.
- Whether the code/style/API usage is reasonable for this project.
- Whether the changes collectively satisfy the user's stated outcome.
- Whether there are regression risks or performance concerns the validators didn't catch.
- **Limit:** The Reviewer may return `approved`, `needs_revision`, or `blocked`. It may NOT approve a candidate that failed deterministic validation, lower a script's risk classification, or waive audit requirements. If the Reviewer is uncertain between `needs_revision` and `blocked`, it must escalate rather than guess.

**Risk Auditor** — handles security/consequence gray areas for high-risk scripts and commands:
- Whether a flagged pattern is a false positive given the project context.
- Whether the risk is acceptable given the operation's scope.
- Whether an unusual but legitimate pattern needs extra confirmation.
- **Limit:** The Risk Auditor may raise risk level, recommend blocking, or request user confirmation. It may NOT lower the Deterministic Static Auditor's risk classification, approve blocked scripts, or grant permissions.

### When Gray Areas Escalate to the User

An AI role must escalate to the user when:
- **Product intent is genuinely ambiguous.** The user said "add a camera" but the project has three camera modes — the choice affects gameplay, not just implementation.
- **The decision is irreversible or costly to undo.** Deleting a large asset tree, changing a shared base class, modifying the project manifest.
- **The system is uncertain and the cost of being wrong exceeds the cost of asking.** The Deep Reviewer sees something suspicious but can't prove it's wrong. The Manager can't tell if the Worker misunderstood the task or the task was underspecified.
- **Multiple reasonable approaches exist and the user hasn't expressed a preference.** The Worker found two valid ways to structure the script; both pass validation. The user should choose.
- **A blocked outcome has no safe automatic fallback.** Three repair cycles failed. The Manager cannot narrow further without changing the task objective.

When escalating, the system must:
- Ask a specific, narrow question — not dump context or expose internal debate.
- Present the concrete options with tradeoffs, not ask the user to debug.
- Include the affected artifacts, risk level, and what happens with each choice.
- Allow the user to answer and resume without restarting the whole task.

### What Must NOT Escalate

The following are implementation details the system must resolve without the user:
- Which Worker type handles which sub-task.
- Individual capability grant requests (the Issuer handles these).
- Routine repair cycles within the retry cap.
- Deterministic validator failures that have clear fixes.
- Low-risk formatting, naming, or organizational choices.
- Tool selection, context-packet assembly, or session management.

The principle: escalate only when the answer affects product intent, safety, reversibility, or cost. Everything else is an implementation detail the system owns.

## PRD, Task, And Review Planning

The Copilot should support planning artifacts as first-class outputs: PRDs, task breakdowns, acceptance criteria, review rubrics, repair tickets, and final reports. These artifacts help the system and user agree on intent, but they are untrusted until normalized and checked against policy. A generated PRD or task plan may clarify objectives and propose work; it must not grant tools, broaden scope, waive validation, or authorize active-project changes.

Planning should use smallest-reviewable-task decomposition. A task is small enough only when one Worker can execute it with a bounded context packet, a task-appropriate capability grant, and objective review criteria. In simple cases this means narrow grants. In enterprise cases it may mean broad task-local access with stronger evidence, audit, and review requirements. Tasks should be split by artifact boundary and trusted tool boundary: scene changes separate from scripts, assets separate from importer commands, diagnostics separate from repair, and PRD/task generation separate from write execution. A task that requires unrelated files, unrelated entity subtrees, multiple risk classes, or ambiguous product choices should be split or sent back for clarification.

Every generated task must include:

- Objective and user-visible outcome.
- Non-goals and explicitly forbidden changes.
- Allowed files, scenes, entities, assets, commands, tools, and operation types.
- Required context packet identifiers and trust labels.
- Acceptance criteria stated as observable outcomes.
- Review rubric with correctness, scope, safety, rollback, diagnostics, and user-impact checks.
- Required evidence such as diffs, scene previews, asset reference checks, validator logs, audit output, or screenshots.
- Repair policy, retry limit, and conditions that escalate to blocked.

PRD generation should produce user-reviewable intent, not implementation authority. Task generation should produce scoped execution tickets, not direct tool calls. Review-rubric generation should produce objective checks that the Reviewer can apply to artifacts. Repair-ticket generation should produce narrow patches against known failures, not a chance to redesign the task.

## Context Isolation And Pollution Controls

Context must be bounded like tools. The Manager or Planner may maintain a broad coordination view, but Workers, Reviewers, Risk Auditors, and Repair Workers should receive role-specific context packets generated from immutable snapshots. Each packet must be trust-labeled, scoped to the task, and referenced by `context_packet_id` and `context_hash`. Enterprise tasks may require broad project context, but that context is still task-local, evidence-oriented, hash-bound, and excluded from unrelated roles by default.

Context packets should avoid raw context stuffing. They should prefer structured summaries, entity IDs, asset IDs, relevant snippets, validator output, and explicit evidence references. Raw project files, script comments, third-party metadata, Worker reports, Reviewer reports, generated PRDs, generated tasks, prior chat, and model rationales remain untrusted and must be labeled as such when included. Cross-task context may be shared only through sanitized handoff records, accepted artifacts, deterministic validator output, or explicit Manager-approved findings.

Automatic repair must not accumulate polluted context. Repair Workers receive the original task brief, the failing evidence, the repair ticket, the current integration candidate, and only the minimal additional context needed to fix the failure. They should not receive unrelated Worker chat, broad prior conversation, speculative rationale, or old rejected drafts unless those artifacts are necessary and trust-labeled as untrusted evidence.

## Zero Trust: The Organizing Principle

The Copilot architecture has one organizing principle, and every other mechanism — hard rules, Worker mediation, Policy Daemon, sandboxing, fresh sessions, Reviewers, transaction bundles, undo — is an implementation of it:

**Assume everything is compromised. Assume everything will fail. Design so that no single failure is catastrophic.**

This is not a pessimistic engineering stance. It is the only coherent way to build a system where untrusted AI output, third-party code, third-party prompts, and user input all interact with trusted project state.

### What "Everything" Means

"Everything" is not rhetorical. The architecture must assume each of these can be wrong, malicious, or faulty at any time:

| Component | Failure mode assumed | Defense |
|-----------|---------------------|---------|
| **User prompt** | Prompt injection, accidental destructive intent, social engineering embedded in text | User prompt is untrusted data. It suggests intent; it does not authorize. Only the Capability Issuer authorizes. |
| **Model output** | Hallucination, malformed JSON, wrong entity IDs, fabricated file paths, prompt injection from prior context | All model output is normalized, schema-validated, and checked against snapshot and scope before execution. Unknown fields, prose, and unsupported operations are discarded. |
| **Manager** | Bad decomposition, wrong Worker assignment, missed scope violations, fabricated completion claims, prompt-injected instructions in plan | Manager decisions are reviewable artifacts, not trusted proof. Policy code, capability grants, validators, and Deep Reviewer independently verify every Manager claim. |
| **Worker** | Out-of-scope tool calls, hallucinated file paths, malicious MCP coordination, fabricated success reports | Worker tool calls must carry a valid grant hash. Every call is checked by the Capability Issuer and sandbox. Worker self-reports are untrusted — only diffs, validators, and review count as evidence. |
| **Deep Reviewer** | Missed issues, biased approval, prompt injection in review output | Reviewer may only approve or reject. It cannot grant permissions, bypass validators, or override blocked audits. Reviewer output is itself reviewable. |
| **Risk Auditor** | Missed risks, false positives, prompt injection in risk report | Risk Auditor output is monotonic (can raise, cannot lower). It cannot override Deterministic Static Auditor blocked decisions. |
| **MCP server** | Data exfiltration disguised as API calls, credential abuse, malformed output, crash, hang | MCP never calls tools directly. Worker mediates all MCP interaction. MCP runs in seccomp-ed subprocess with declared-host-only network. Credentials are scoped and revocable. |
| **Skill text** | Prompt injection, instructions to bypass validation, social engineering of the agent | Skill text is `UNTRUSTED_SKILL_INSTRUCTION`. System prompt is injected after skill text. Tool layer enforces actual grant, not what the skill says. |
| **Project files** | Malicious script comments, poisoned asset metadata, misleading file names, symlink attacks | All project content is untrusted data. Script comments are preprocessed. Paths are canonicalized. Symlinks are resolved and validated. |
| **Third-party scripts** | Malicious syscalls, data exfiltration, prompt injection in string literals | Deterministic Static Auditor (Layer 1) blocks scripts with dangerous patterns. No model override possible. |
| **Capability Issuer** | Logic bug, missing edge case, incorrect risk classification | Sandbox independently enforces path constraints. Seccomp independently blocks syscalls. Git workspace isolates all writes. Defense in depth. |
| **Policy Daemon** | Crash, memory corruption, dependency vulnerability | Daemon is a separate minimal process. Its failure blocks operations (fail-closed). Editor and AI orchestration run in separate processes — daemon crash does not corrupt active project. |
| **Transaction engine** | Partial apply, corrupted bundle, race condition with user edits | Atomic apply with rollback journal. Pre-apply and post-apply hash verification. Stale snapshot detection. Undo stack integration. |
| **Git workspace** | Corruption, merge conflict, branch mismatch | Workspace is isolated from active project. Nothing in the workspace affects the active project until a validated transaction bundle is applied. Workspace can be discarded entirely. |

### Defense in Depth: What Happens When Each Layer Fails

Zero trust means no single layer is sufficient. The system must survive the failure of any one component:

```
Layer 1: User Approval (necessary but not sufficient)
  ↓ bypassed? → Layer 2 still blocks
Layer 2: Manager + Worker + Reviewer (AI judgment)
  ↓ compromised? → Layer 3 still blocks
Layer 3: Capability Issuer (deterministic grant enforcement)
  ↓ bug or gap? → Layer 4 still blocks
Layer 4: Sandbox + Seccomp (path isolation + syscall filtering)
  ↓ bypassed? → Layer 5 still blocks
Layer 5: Git Workspace Isolation (writes never touch active project)
  ↓ corrupted? → Layer 6 still blocks
Layer 6: Transaction Bundle Verification (hash mismatch → rollback)
  ↓ tampered? → Layer 7 still recovers
Layer 7: Undo + Rollback (user can always revert to pre-bundle state)
```

**No single layer is trusted to be correct.** Each layer independently enforces constraints. A failure at Layer 2 does not grant Layer 3 authority. A bug in Layer 3 does not remove Layer 4's sandbox. A compromised workspace at Layer 5 cannot reach the active project without passing Layer 6 verification.

### What Zero Trust Does NOT Mean

Zero trust does not mean "verify everything with AI." AI is the least trustable component in the system. Zero trust means:

- Trust nothing by default, verify everything deterministically where possible.
- Where deterministic verification is impossible (intent, quality, semantic correctness), use AI as advisory judgment with hard limits — AI can recommend, flag, or reject, but never authorize or grant.
- Where AI judgment is insufficient, escalate to the user with objective evidence.
- Where the user might be wrong, present objective facts (diffs, hashes, validator output, audit reports) rather than model summaries.

### The Core Invariants

These are the non-negotiable rules derived from zero trust. Every component, every workflow, every test must satisfy them:

1. **AI never mutates trusted state directly.** AI produces structured proposals or writes to isolated draft workspaces only.
2. **Model output is never an authority source.** Permissions, grants, and apply authorization come only from deterministic policy code.
3. **No single AI agent can authorize its own actions.** Manager requests, Worker executes, Reviewer reviews, Issuer authorizes — no role combines them.
4. **Untrusted input must not alter system instructions, task scope, permission grants, tool schemas, validation requirements, approval requirements, or Reviewer rubric.** This includes user prompts, model output, project files, script comments, plugin metadata, command labels, skill text, and Worker/Reviewer reports.
5. **Every action must bind to `task_id`, `snapshot_id`, `base_revision`, `scope_hash`, and where applicable `grant_hash`.** Unbound actions are rejected.
6. **Every write-capable workflow must produce an immutable transaction bundle before the active project is touched.** No incremental writes, no partial applies.
7. **Any out-of-scope operation must be rejected even when it appears useful or correct.** "Helpful" out-of-scope changes are scope violations.
8. **AI review decisions are monotonic safety signals.** They may increase required review level or recommend blocking. They must never override deterministic validators, lower risk classifications, or grant permissions.
9. **User approval is necessary for applying approved write bundles, but it is not sufficient to bypass policy, validation, sandbox, audit, review, rollback, or stale-context checks.**
10. **The system must fail closed.** Uncertainty, missing evidence, stale snapshots, tampered hashes, and incomplete validation must block apply — never default to apply.

Write workflows should be atomized into explicit phases:

1. Snapshot: freeze `snapshot_id`, `base_revision`, scene state, asset index, diagnostics baseline, selection state, editor metadata, and active dirty-state baseline.
2. Proposal: ask the model for structured intent or operation proposals without executing side effects.
3. Normalize: convert model output into a canonical operation list and discard prose, unknown fields, implicit side effects, and unsupported operation shapes.
4. Validate: deterministically check schema, scope, paths, commands, capabilities, permissions, undo contracts, rollback contracts, dependency impact, and task acceptance criteria.
5. Stage: apply candidate changes only in the Manager-created task workspace or staging transaction, never in the active project.
6. Audit: run deterministic command, script, dependency, asset, and prompt-injection audits; optionally run a secondary Risk Auditor model over sanitized evidence.
7. Review: evaluate objective artifacts against the task brief, acceptance criteria, scope, validator output, audit output, code/file diffs, scene diffs, asset diffs, and diagnostics.
8. Bundle: package approved work into an immutable transaction bundle with operation list, touched artifacts, before and after hashes, rollback journal, validation report, audit report, review report, and approval metadata.
9. Apply: use the trusted editor transaction engine to atomically apply the bundle to the active project.
10. Verify: reload scenes, rescan assets, validate hashes, and compare diagnostics. If any apply or verification step fails, the entire bundle must roll back to the previous trusted state.

The transaction bundle should include at least:

- `bundle_id`, `task_id`, `snapshot_id`, `base_revision`, `scope_hash`, and `bundle_hash`.
- Canonical operation list and logical change groups.
- Touched files, entities, scenes, assets, commands, and settings.
- Before and after hashes for all modified trusted artifacts.
- Rollback journal for file creates, file updates, file deletes, asset database changes, scene changes, settings changes, and editor dirty-state changes.
- Deterministic validation report, deterministic audit report, optional Risk Auditor report, Deep Reviewer report, unresolved concerns, and user approval record.
- Apply and post-apply verification results.

Partial acceptance must create a new scoped integration candidate and a new transaction bundle. The editor must not directly splice unchecked fragments from a larger rejected or partially accepted bundle into the active project.

## Trust Boundaries

Trusted inputs are limited to compiled policy code, operation schemas, command capability registry entries, deterministic validators, sandbox checks, the transaction engine, and editor-owned state snapshots. Built-in editor commands and importers are conditionally trusted only when they explicitly declare an AI-safe capability, parameter schema, effect classification, sandbox contract, and undo or rollback contract.

All other content is untrusted, including user prompt text, model output, project files, third-party assets, third-party scripts, plugin metadata, command labels and descriptions, generated code comments, Worker summaries, Worker rationales, Worker claimed test results, Reviewer reports, and prior conversation text. Prompt context must label trust boundaries explicitly with labels such as `TRUSTED_POLICY`, `TRUSTED_TASK_SCOPE`, `TRUSTED_COMMAND_SCHEMA`, `DETERMINISTIC_VALIDATION_REPORT`, `UNTRUSTED_USER_PROMPT`, `UNTRUSTED_PROJECT_FILE`, `UNTRUSTED_SCRIPT_CONTENT`, `UNTRUSTED_PLUGIN_METADATA`, and `UNTRUSTED_WORKER_REPORT`.

Tool descriptions, command labels, asset names, component metadata, file contents, script comments, and strings from third-party sources must be treated as data fields, not instructions. They must not be able to add tools, hide tools, broaden permissions, waive validations, change the task objective, or instruct a Worker, Manager, Reviewer, Risk Auditor, or user approval UI to ignore policy.

### Process-Level Isolation for the Trusted Core

Trust boundaries drawn at the code level are necessary but not sufficient. In-process trust boundaries rely on the Rust type system, `#![forbid(unsafe_code)]`, and correct implementation. A logic bug in untrusted-data handling, a vulnerability in a dependency, or a supply-chain compromise in a transitive crate could allow untrusted input to corrupt trusted state across what should be a hard boundary.

The long-term target is to move the trusted authority surface into a dedicated, minimal process: the **Policy Daemon**.

The Policy Daemon owns exclusively:
- Capability Issuer logic and grant signing keys.
- Deterministic Static Auditor and Script Preprocessor.
- Command capability registry.
- Sandbox path policy and canonical root resolution.
- Risk classification rules.
- Transaction bundle verification and signing.

The Policy Daemon exposes a narrow IPC interface (Unix domain socket, local-only). The editor process and AI orchestration process call into it as clients. The Policy Daemon:
- Runs with minimal dependencies — no AI model runtime, no GPU code, no asset pipeline, no scripting engine, no network stack beyond the local socket.
- Accepts structured requests (validate this grant, audit this script, preprocess this source, verify this bundle) and returns structured, signed responses.
- Rejects any input that does not match the expected schema — malformed requests are dropped, not interpreted.
- Never receives raw model output, raw user prompts, raw plugin metadata, or raw Worker chat. It only sees the normalized, schema-validated fields relevant to its decisions.
- Is the sole signer of capability grants and transaction bundles. If the Policy Daemon did not sign it, the tool layer rejects it.

This design means:
- A memory corruption bug in the AI orchestration code, the model provider client, the editor renderer, or a third-party asset importer cannot reach the Policy Daemon's memory space.
- A compromised dependency in the editor process cannot forge a grant signature or tamper with an audit report.
- The blast radius of any single-process vulnerability stops at the Policy Daemon boundary.
- The Policy Daemon's correctness can be audited, fuzzed, and verified independently of the much larger editor and AI codebase.

For the initial milestones, the trusted core may run in-process behind a strict module boundary with a clear IPC-ready API. The module boundary must:
- Accept only owned, schema-validated structs — never raw strings, byte slices, or deserialized-from-unknown-source types.
- Never hold references to data owned by untrusted subsystems.
- Return only signed, immutable result types.
- Be auditable as a single compilation unit with `#![forbid(unsafe_code)]` and `cargo vet`-verified dependencies.

The process split to a Policy Daemon should be prioritized before Auto mode handles real user projects with third-party scripts or plugins.

## Capability-Gated Worker Execution

### Who Controls Permissions: The Capability Issuer

Worker permission control belongs to a single deterministic component: the **Capability Issuer**. It is trusted Rust code, not an AI agent, not the Manager, not the user.

The flow:

1. **Manager requests.** The Manager decomposes a task and emits a structured capability request for each Worker: "ScriptWorker for task T3 needs read access to `assets/scripts/player/` and write access to the task workspace under `scripts/`." For enterprise tasks, the request may be broad: "AssetWorker for task T7 needs read access to `assets/**`, write access to the task workspace mirror, and permission to run the registered asset reference validator."
2. **Issuer decides.** The Capability Issuer evaluates the request against deterministic inputs — task scope, command capability registry, sandbox policy, risk classifier, current snapshot metadata, evidence requirements, and rollback contract. It may approve, narrow, deny, classify as high-risk, or route to organization approval. It issues a signed grant with a `grant_hash`.
3. **Tool layer enforces.** Every Worker tool call must carry the active grant hash. The tool layer rejects calls with missing, stale, mismatched, or insufficient grants before execution.
4. **No bypass.** The Manager cannot mint grants. The Worker cannot self-expand grants. The user cannot override the Issuer with an approval click. Policy code is the sole authority.

The Issuer's inputs are all deterministic and trusted:
- **Command capability registry** — which editor commands are marked `ai_safe`, their parameter schemas, effect kinds, risk levels, sandbox contracts, and undo/rollback contracts.
- **Sandbox policy** — canonical project roots, allowed read roots, allowed write roots (always the task workspace for writes), path traversal rejection rules, symlink canonicalization rules.
- **Risk classifier** — maps operation types, file patterns, command IDs, credential access, network access, process execution, dependency changes, and rollback quality to risk levels (low / medium / high / critical).
- **Task scope** — the normalized, policy-checked task brief with allowed files, entities, scenes, assets, and operation types.
- **Snapshot metadata** — `snapshot_id`, `base_revision`, workspace root, task ID.

The Issuer is the narrow point in the system where AI intent meets deterministic authorization. If the Issuer says no, the operation cannot proceed — regardless of what the Manager, Worker, Reviewer, or user believes. If the Issuer says "approved with broad scope," that approval still applies only to the signed task, workspace, risk route, expiration, and evidence contract. It is not a reusable trust relationship with the Worker.

The Manager coordinates Worker capability requests, but it is not the root authority for grants. The Manager may narrow, deny, or route work, but it cannot mint unrestricted permissions or override policy. Workers execute only in the Manager-assigned task workspace and may call only the tools, editor commands, roots, files, entities, assets, operation types, and model capabilities explicitly listed in their active capability grant. A Worker may receive broad task-local authority when policy approves it, but it cannot convert that into permanent authority, sibling authority, MCP authority, active-project authority, or future-task authority. Workers cannot self-expand permissions through prompts, tool output, project files, plugin metadata, Reviewer comments, user prompt text, or generated code.

## Manager Oversight

The Manager is checked by layered controls rather than by a single superior agent:

- Policy code checks whether the Manager's requested scope, grants, tools, commands, files, and risk level are allowed.
- The capability issuer creates task-bound grants from trusted task scope and policy, not from Manager prose. Grants may be narrow or broad depending on the task, but broad grants require stronger binding, evidence, review, and rollback guarantees.
- Worker tool calls prove actual behavior against grant hashes, so Manager plans cannot authorize hidden side effects.
- Integration validation checks the resulting diffs, scenes, assets, commands, dependency changes, and diagnostics against the original task and scope.
- Deep Reviewer and Risk Auditor review objective artifacts and may increase risk, block work, or require user confirmation.
- The transaction bundler compares Manager reports against canonical operation lists, diffs, validator output, audit output, and rollback journals.
- The user approval UI is generated from trusted bundle metadata, not Manager summaries.
- Immutable trace records let maintainers audit divergence between Manager intent, Worker behavior, review findings, and applied changes.

Manager output must fail closed when it is internally inconsistent, unsupported by artifacts, outside scope, missing evidence, stale against the active snapshot, or contradicted by validators or reviewers. Auto mode may continue with safe repairs inside the task workspace, but it must not apply or present work as ready when Manager oversight is incomplete.

A Worker capability grant should include:

- `task_id`, `worker_id`, `snapshot_id`, `workspace_id`, `workspace_root`, `base_revision`, and `grant_hash`.
- Allowed tools, allowed commands, allowed read roots, allowed write roots, allowed files, allowed entities, allowed scenes, allowed assets, allowed operation types, and forbidden operation types.
- Network and process execution flags, both disabled by default.
- Risk class, review route, escalation route, organization-policy gate if required, and whether the grant is narrow or broad.
- Step limit, expiration, retry limit, revocation state, and trace parent ID.
- Acceptance criteria, expected artifacts, required evidence, and rollback expectations for the Worker task.

Every Worker tool call must carry the active grant hash and must be checked by deterministic policy code. The tool layer must reject calls with missing, stale, mismatched, or insufficient grants. Workers must not create their own worktrees, choose active project paths, execute shell commands directly, access unauthorized files, write unauthorized paths inside the task workspace, invoke ungranted editor commands, or call non-AI-safe commands.

If a Worker needs access outside its grant, it must stop that portion of work and emit a structured capability escalation request. The request must include the requested capability, necessity, minimal viable scope, expected artifact, risk level, alternatives considered, rollback impact, evidence impact, and impact on the original task. The Manager may deny the request, narrow it, request a new policy-issued grant, reassign the task, ask a peer reviewer or Risk Auditor for judgment, ask the user or organization policy for confirmation when the decision affects product intent or high-risk authority, or block the task. All escalation requests, peer reviews, policy decisions, and Manager decisions must be recorded in trace history.

## Prompt Injection And Drift Controls

The system must defend against toxic prompts, prompt injection, context drift, task drift, poisoned third-party scripts, and poisoned command metadata. Immutable snapshots reduce drift only if every subsequent action remains bound to the snapshot and scope. Every Manager, Worker, Reviewer, Repair Worker, Risk Auditor, final report, transaction bundle, and user approval screen must carry the relevant `task_id`, `snapshot_id`, `base_revision`, `scope_hash`, and current capability or bundle hash.

Context drift rules:

- Workers must not read from mutable live project state after snapshot creation.
- Workers, Reviewers, Risk Auditors, and Repair Workers must consume bounded context packets, not the full Manager conversation or full project context.
- Every context packet must carry `context_packet_id`, `snapshot_id`, `scope_hash`, `context_hash`, source list, trust labels, and expiration rules.
- Context packets must be regenerated or invalidated when the snapshot, task scope, accepted artifacts, validator report, audit report, or integration candidate changes.
- Generated PRDs, generated task plans, Worker notes, and prior model messages may be included only as untrusted references, never as policy or ground truth.
- If a tool result references context outside the snapshot or current capability grant, the Worker must request Manager escalation or re-planning.
- If the active editor state, dirty baseline, git revision, asset index, validation report, audit report, or bundle hash changes after user review is presented, previous approval becomes stale and invalid.
- If the user continues editing during AI work, apply must pause for revalidation or conflict resolution before touching the active project.

Task drift rules:

- Each task and Worker brief must define objective, allowed files, allowed entities, allowed scenes, allowed assets, allowed operations, forbidden operations, acceptance criteria, expected artifacts, and scope hash.
- Each task must include a review rubric and required evidence before Worker execution begins.
- The Manager must prefer the smallest independently reviewable task. If a task crosses artifact boundaries, tool boundaries, risk classes, or unrelated gameplay goals, it should be split.
- A task cannot inherit scope from a PRD, prior conversation, or sibling task unless the inherited scope is explicitly normalized into its task brief and policy-issued capability grant.
- Any diff or operation outside the delegated scope must be rejected even if it appears beneficial.
- A Worker may report possible adjacent improvements only as findings. It must not apply them without a new Manager grant.
- Repair Workers may patch only the issue described in the repair ticket and only within the repair ticket's allowed scope.

## Command And Script Audit

Third-party scripts, importer scripts, build scripts, generated scripts, and plugin-provided execution hooks are untrusted input. They may contain malicious logic, prompt injection, or accidental unsafe behavior. The audit architecture applies three layers in strict sequence:

### Layer 1: Deterministic Static Auditor (Trusted Rust Code)

The Deterministic Static Auditor is trusted, non-AI code. It always runs first, before any model sees any script content. It cannot be skipped, and its findings cannot be overridden by any model.

It extracts:
- Syntax / AST structure (validity, complexity indicators).
- Import list (modules, stdlib, external dependencies).
- Function declarations and call graph.
- Engine API calls and their parameter shapes.
- Filesystem access patterns (read, write, delete, path construction).
- Network access indicators (sockets, HTTP, bind).
- Process execution indicators (Command, spawn, exec).
- Dynamic eval usage (eval, load, require with dynamic paths).
- Credential access patterns (env vars, config files, key stores).
- Resource usage risks (infinite loops, large allocations, recursion depth).
- Prompt-injection indicators in comments, string literals, identifier names, and metadata.
- Dependency changes (new imports, version changes, external package references).

Output: a structured audit report with risk tags, evidence references (line numbers, snippets), and a deterministic risk classification (low / medium / high / blocked).

If the Deterministic Static Auditor classifies a script as **blocked** (e.g., detected `std::process::Command`, raw socket access, dynamic eval of untrusted input), the script cannot proceed to any model for review. It is rejected with the audit report as evidence. No model, Manager, Worker, Reviewer, or user approval can override this.

### Layer 2: Script Preprocessor (Trusted Rust Code)

Before any model sees script content, the Script Preprocessor transforms raw untrusted source into a sanitized representation:
- Neutral summary (function names, signatures, purpose hints from structure).
- Symbol table and import list.
- API call list with parameter counts.
- Risk tags from the Deterministic Static Auditor.
- Small quoted evidence snippets (individual lines, not full source blocks).
- Comments and string literals are stripped of semantic meaning — they appear only as quoted data under `UNTRUSTED_SCRIPT_CONTENT` labels, never as narrative text.

The preprocessor's output is the only script representation any model receives. Raw script text is never injected into a model prompt. This prevents prompt injection through code comments, string literals, or identifier names.

### Layer 3: Risk Auditor Model (AI, Fresh Session, High-Risk Only)

The Risk Auditor is a secondary AI model invoked only for scripts that the Deterministic Static Auditor classified as **medium risk or higher**. It runs in a fresh session with only:
- The preprocessor's sanitized script representation.
- The Deterministic Static Auditor's report.
- The command capability declaration (if applicable).
- The task scope and risk policy.

It never receives raw script source, script comments, plugin metadata, or Worker/Manager prose about the script.

The Risk Auditor produces a structured advisory risk signal:
- Decision: `clear` / `review_recommended` / `confirmation_required` / `block_recommended`.
- Risk tags with evidence references.
- Recommended handling (e.g., "require user confirmation for filesystem write in line 42").

The Risk Auditor's output is **monotonic**: it can raise risk level, recommend blocking, or request human confirmation, but it can NEVER:
- Lower the risk level below the Deterministic Static Auditor's classification.
- Approve a script the Deterministic Static Auditor marked as blocked.
- Grant permissions, bypass policy, or waive validation.
- Override the Capability Issuer's decision.

### Audit Execution Model: Pipeline Steps vs. Specialized Worker

The three audit layers have different execution models:

**Layer 1 — Deterministic Static Auditor: Pipeline Step (Not a Worker)**

This is trusted Rust code that runs as part of the deterministic validation pipeline, alongside build checks, schema validators, and asset reference validators. It is not a Worker, not AI, not scheduled by the Manager. It executes automatically whenever a script file is staged in the integration candidate. The Manager cannot skip it, configure it, or override its output.

Execution: synchronous, in-process, runs on every script file in the integration candidate before Deep Review begins. Failures produce repair tickets directly, bypassing Reviewer discussion.

**Layer 2 — Script Preprocessor: Pipeline Step (Not a Worker)**

Also trusted Rust code, also runs automatically in the validation pipeline. It transforms raw script source into the sanitized representation that any subsequent model (Risk Auditor, Deep Reviewer, final report) may receive. It is the single choke point that ensures raw untrusted script text never reaches a model prompt.

Execution: synchronous, in-process, runs immediately after the Deterministic Static Auditor for any script that passed Layer 1 (i.e., was not blocked).

**Layer 3 — Risk Auditor: Specialized Worker (`audit_worker`)**

The Risk Auditor is a specialized AI Worker invoked by the Manager. It is NOT part of the deterministic pipeline — it runs as a Worker in a fresh session, with a capability grant scoped to read-only access to the audit report and preprocessed script representation.

Execution:
1. Manager receives the Deterministic Static Auditor report. Scripts classified as low-risk skip Layer 3 entirely.
2. For scripts classified medium-risk or higher, Manager creates an `audit_worker` task with a fresh session.
3. Context packet includes only: the preprocessor's sanitized script representation, the Deterministic Static Auditor report, the command capability declaration (if applicable), and the task scope.
4. Risk Auditor produces a structured advisory risk signal (clear / review_recommended / confirmation_required / block_recommended).
5. Output is attached to the integration candidate as audit metadata. The Deep Reviewer and final report consume it.

The Risk Auditor's output is advisory and monotonic (can raise risk, cannot lower, cannot override Layer 1). If the Risk Auditor recommends `block_recommended`, the Manager must either create a repair ticket with narrower scope or escalate to the user — it cannot silently dismiss the recommendation.

### Audit Sequence in Auto Mode

For any task that includes third-party or generated scripts:

1. Worker produces script → committed to task workspace.
2. **Pipeline: Deterministic Static Auditor** runs automatically on the integration candidate. Output: audit report with risk tags and classification (low / medium / high / blocked).
3. If any script is **blocked** → repair ticket created immediately. Worker must fix or task becomes unresolved issue. No model sees the blocked script.
4. **Pipeline: Script Preprocessor** runs on all non-blocked scripts. Produces sanitized representation.
5. Scripts classified **low-risk**: audit report accepted as-is. No Risk Auditor invoked.
6. Scripts classified **medium-risk or higher**: **Manager spawns `audit_worker`** in fresh session with preprocessor output + audit report.
7. Risk Auditor signal attached to integration candidate metadata.
8. **Deep Reviewer** sees: audit reports + Risk Auditor signals (if any) + preprocessed script representations — never raw script source.
9. User sees audit findings in final report. High-risk scripts require step-up confirmation before apply.

### Why Risk Auditor is a Worker, Not a Pipeline Step

The Risk Auditor uses an AI model, which means it has variable latency, variable quality, and potential for model-specific failures. Making it a Worker gives it:
- Fresh-session isolation (cannot be prompt-injected by Manager or sibling Worker context).
- Standard Worker lifecycle (retry, timeout, failure handling, trace recording).
- Manager oversight (Manager decides when to invoke, reviews output, decides escalation).
- Independent testability (can be tested with deterministic model stubs like other Workers).

The Deterministic Static Auditor and Script Preprocessor, by contrast, are fast, synchronous, deterministic Rust code. They belong in the pipeline because they are reliable infrastructure, not AI.

### Editor Commands

Editor commands are not AI-callable by default. A command may be exposed to AI only when it has an explicit capability declaration with `ai_safe=true`, parameter schema, effect kind, risk level, sandbox contract, undo or rollback contract, and source trust level. Command labels, descriptions, plugin metadata, and examples are untrusted and must not influence policy.

Every command execution request must first pass deterministic checks for command identity, AI-safe status, parameter schema, effect classification, active capability grant, task scope, sandbox contract, undo or rollback contract, and risk level. Commands that can execute processes, access the network, touch credentials, alter dependencies, modify plugin manifests, change permissions, run import scripts, or mutate settings are high-risk and require explicit policy support, audit, and step-up user confirmation.

## Reviewer Evidence Rules

Deep Reviewer and local Reviewer agents must not trust Worker self-reports, rationales, summaries, claimed test results, claimed safety properties, or claimed completion. Worker reports are untrusted metadata and may be used only as navigation hints. Reviewer decisions must be based on objective artifacts: trusted task brief, acceptance criteria, allowed scope, policy, normalized operation list, file diffs, scene diffs, asset diffs, generated script AST summaries, command execution logs from the trusted tool layer, deterministic validation output, deterministic audit output, reproducible diagnostics, and preview results.

Reviewer prompts must clearly separate trusted evidence from untrusted Worker reports and generated prose. Claimed tests passed are not evidence unless backed by deterministic validator logs. Claimed scope compliance is not evidence unless backed by scope validator output and actual diffs. Claimed completion is not evidence unless the objective artifacts satisfy the original task and acceptance criteria.

If objective artifacts are insufficient to verify completion, safety, rollback, or scope compliance, the Reviewer must return `needs_revision` or `blocked`. Reviewer approval must not override failed validators, failed audits, out-of-scope changes, stale snapshots, missing rollback contracts, or missing user approval.

## User Approval Hardening

User approval can authorize only transaction bundles that already passed deterministic policy, scope validation, sandbox validation, command and script audit, deterministic validation, review, rollback planning, and stale-context checks. User approval must never override blocked policy decisions, failed validators, unsafe command classifications, out-of-scope diffs, missing rollback contracts, missing audit evidence, stale bundle hashes, or stale snapshots.

The approval UI must show objective artifacts rather than model claims: canonical diff, scene preview, asset preview, affected files, affected entities, affected assets, command manifest, validation results, audit findings, risk tags, rollback plan, unresolved concerns, bundle hash, and operation groups. Approval text and risk summaries must be generated from trusted operation metadata, validator output, and audit output, not from model prose or Worker summaries.

High-risk bundles require step-up confirmation bound to the immutable bundle. High-risk changes include destructive operations, irreversible operations, network-capable operations, process-capable operations, credential-touching operations, dependency changes, permission changes, plugin manifest changes, editor settings changes, importer execution, build script execution, and changes to `.env`, credentials, keys, project settings, plugin manifests, dependency manifests, or build scripts. Step-up confirmation may require re-entering the project name, confirming affected artifact counts, acknowledging risk phrases, OS credential confirmation, or editor account re-authentication, depending on platform support.

The approval UI must defend against misleading presentation. It should normalize and reveal full paths, command IDs, permission changes, source plugin IDs, Unicode confusables, hidden characters, long path truncation, similar filenames, and generated descriptions. If `task_id`, `snapshot_id`, `scope_hash`, `bundle_hash`, validation report, audit report, review report, active editor state, or base revision changes after presentation, previous approval is invalid and the bundle must be revalidated or regenerated before apply.

## Implementation Decisions

- The first product surface is an Editor Copilot. Runtime NPC AI, behavior trees, inference during gameplay, and player-facing AI are separate future features.
- The feature remains gated behind editor-oriented profiles. Minimal runtime builds must not depend on model providers, network clients, or editor-only services.
- The existing AI session concept should become the orchestration boundary for one user request. It should gather context, run the Manager/Worker/Reviewer loop when enabled, validate tool calls, execute approved operations, and return an outcome.
- The Copilot surface presents two modes: **Copilot mode** (interactive single-agent, user-in-the-loop for permissions) and **Auto mode** (Agent cluster with Manager/Worker/Reviewer, automated permissions, final-report-driven approval).
- Copilot mode uses a single Agent that calls tools directly. Every write operation requires user permission approval. Auto-accept is a session-level toggle that pre-approves low-risk and medium-risk operations within declared scope; high-risk operations still require step-up confirmation.
- Auto mode spawns a Manager that decomposes the request, Workers that execute in parallel inside a git-backed task workspace, and a Deep Reviewer that inspects the integrated result. Permissions are issued automatically by policy — the user is not asked to approve individual Worker grants.
- Enterprise Auto mode should assume that useful tasks may require broad task-local permissions and command execution. The design goal is not to keep every grant tiny; it is to keep every grant task-bound, workspace-bound, time-bound, evidence-bound, risk-routed, traceable, revocable, and unable to mutate trusted state directly.
- Permission and command requests should be automatically reviewed. Deterministic policy decides whether a grant is allowed; peer agents can critique proportionality, intent, risk, and evidence; organization policy or human approval handles critical-risk escalation.
- Agent-to-agent review is advisory and monotonic. Reviewers, Risk Auditors, adversarial reviewers, and Managers may raise risk, request narrower scope, send work back for repair, or block. They must not mint grants, lower deterministic risk classification, approve blocked operations, or override the Capability Issuer.
- The single-agent session model should not assume there is only one agent message stream. It should be able to store roles, task assignments, handoffs, review decisions, and retries. Copilot mode uses a simplified subset of the same session model.
- The Manager agent owns context snapshot creation, AI task workspace creation, request decomposition, plan shape, architecture choices, worker selection, capability request creation, task sequencing, integration decisions, repair ticket creation, merge decisions, and the final user-facing report.
- Manager decisions are inputs to policy, validation, audit, review, and transaction bundling, not trusted proof. Tests and UI should assume Manager summaries can be wrong, stale, incomplete, or adversarial.
- **Problems must not be silently skipped.** Workers must report unrecoverable failures, out-of-scope needs, and ambiguous conditions to the Manager. The Manager must include every unresolved problem in the final report with severity, affected artifacts, and a suggested quick-fix action.
- **Fresh sessions** are the primary context-isolation mechanism in Auto mode. Workers, Repair Workers, the Deep Reviewer, and the Risk Auditor each execute in a fresh session carrying only their role-specific context packet. They must not receive the Manager's full conversation, sibling Worker chat, prior repair reasoning, or the user's raw prompt text.
- Fresh sessions are spawned by the session orchestrator. The Manager specifies the role, context packet ID, and task brief; the orchestrator creates the session, injects the context packet, and returns structured output. Fresh sessions have no memory of prior turns in the same task.
- The final report must include an **unresolved issues** section with quick-fix actions. Each quick-fix action describes a scoped remediation the user can trigger from the report UI. Quick-fix launches a new scoped Auto mode task limited to the unresolved issue.
- Quick-fix tasks reuse the original snapshot and task workspace where possible. They produce a mini integration candidate, pass validation and review, and append results to the original final report.
- A Planner role may be split out from the Manager when PRD/task generation becomes substantial. The Planner may create PRDs, task trees, acceptance criteria, and review rubrics, but it must not issue capabilities or apply changes.
- Generated PRDs should be stored as draft planning artifacts with version, source context packet IDs, assumptions, open questions, non-goals, risks, and acceptance criteria.
- Generated task plans should be normalized into task tickets before execution. Each ticket must have a stable `task_id`, parent PRD or request reference, scope hash, context packet IDs, acceptance criteria, review rubric, required evidence, repair policy, and retry limit.
- Task minimization should run before capability issuance. The system should reject or split task tickets that include unrelated artifacts, broad file globs, vague acceptance criteria, mixed risk levels, or permissions broader than the expected evidence requires.
- Before any Worker starts, the Manager must create an immutable task snapshot. The snapshot should include project revision, scene selection state, asset index version, script references, diagnostics baseline, and editor state metadata.
- Workers must operate against the immutable task snapshot and assigned AI task workspace. Workers must not read directly from changing live project state.
- Each AI write task should have a dedicated task identifier and isolated git worktree with a stable branch convention such as `ai/task-0001`.
- Git is a prerequisite for write-capable Agent cluster workflows. At session startup and before any AI write task, the Copilot should verify that git is installed and that the project root is inside a git repository.
- If git is not installed, write-capable workflows should be blocked with setup guidance that explains how to install git for the current platform.
- If git is installed but the project is not initialized as a repository, write-capable workflows should be blocked until the user initializes git for the project. The editor may offer an explicit "Initialize Git for this project" action.
- Worker agents are specialized executors. Initial worker types should map to trusted tool boundaries, such as `scene_worker`, `script_worker`, `asset_worker`, `diagnostics_worker`, `explain_worker`, `repair_worker`, and `audit_worker`.
- Each Worker should receive a bounded task brief with allowed tools, allowed project roots, relevant context snippets, acceptance criteria, and a trace parent ID. Workers should not be able to broaden their own permissions.
- Each Worker context packet should include only the minimum normalized facts, snippets, IDs, validator output, and task-specific evidence needed for that Worker. Full PRDs, full conversation logs, sibling Worker reports, and unrelated project files should be excluded by default.
- Worker execution should be sandboxed by default. A Worker should receive the smallest viable set of read roots, write roots, commands, feature flags, and model/tool capabilities that can realistically complete its task. For enterprise tasks this set may be broad, but broad grants require stronger evidence requirements, risk routing, review, expiration, trace logging, and rollback guarantees.
- Workers must not silently skip or paper over failures. If a Worker encounters an unrecoverable error, a missing dependency, an ambiguous requirement, or an out-of-scope need, it must stop the affected sub-task and emit a structured problem report to the Manager. The report must include the failure description, affected artifacts, severity, whether the issue is blocking, and a suggested remediation path.
- The Manager must not discard or hide Worker problem reports. Every unresolved problem must appear in the final report under an "Unresolved Issues" section with a quick-fix action. The Manager may attempt reassignment or repair before reporting, but must not silently drop a problem to make the task appear complete.
- Fresh sessions are created by the session orchestrator at the Manager's request. The Manager specifies the role type, context packet ID, task brief reference, and expected output schema. The orchestrator spawns the session, injects the context packet as the initial system state, runs the agent, and returns structured output. The fresh session has no access to prior conversation turns, Manager deliberation, or sibling Worker outputs unless explicitly included in the context packet.
- Fresh sessions are mandatory for Workers, Repair Workers, the Deep Reviewer, and the Risk Auditor in Auto mode. The Manager itself may run in the orchestration session but should not leak its full reasoning into Worker context packets.
- Copilot mode runs in a single session. Auto-accept state is a session-level flag tracked by the tool layer, not by the model.
- Worker write access should target the assigned git-backed AI task workspace. Direct active-project writes by Workers should be disallowed.
- Worker permissions should be capability-based. A Scene Worker may modify only the assigned scene scope and must not modify scripts unless the task grant explicitly includes a scene-plus-script workflow. A Script Worker may modify only the assigned script scope and must not modify the scene graph unless the task grant explicitly includes coordinated changes. Diagnostics and Explain Workers are read-only by default. A Repair Worker is limited to the repair ticket scope. Broad cross-artifact grants are allowed only when the task ticket, policy, evidence requirements, and review route explicitly model the cross-artifact work.
- Workers should create logical commits in the isolated task workspace, such as `scene-layout-update`, `enemy-spawn-fix`, `asset-import-pass`, `shader-adjustments`, or `diagnostic-cleanup`. Large monolithic commits should be discouraged.
- Every Worker output should pass local review before integration. Local review decisions should be structured as `approved`, `needs_revision`, or `blocked` and include review findings and risk tags.
- Blocked Worker outputs cannot proceed to integration. Outputs marked `needs_revision` should return to the responsible Worker when possible.
- Promotion from Worker output to the integration candidate should be an explicit Manager-controlled step after local review approval. Rejected Worker outputs remain traceable drafts and must not be merged, applied, or used as implicit project state.
- Worker outputs should be structured as proposed operations, findings, diagnostics, and open questions. Free-form prose can explain work, but executable changes must remain structured.
- The Manager should merge approved Worker outputs into an integration candidate. The integration candidate becomes the single source for deterministic validation and deep review and should be backed by the git task workspace.
- No final review should occur against individual Worker outputs. Deep review always evaluates the complete integrated result.
- A deterministic validation layer must run before Deep Review. Required validators include compile/build validation, type checking, linting, asset reference validation, GUID consistency validation, scene schema validation, editor load validation, dependency validation, and performance budget validation.
- Validator failures should create repair tickets directly and bypass reviewer discussion until the deterministic failure is repaired.
- The Deep Reviewer is a mandatory quality gate for integrated multi-agent changes before they are presented as ready to apply. It should inspect the entire integration candidate for architecture consistency, gameplay impact, scene consistency, API correctness, performance risks, regression risks, security risks, editor workflow impact, trace completeness, and whether tests or diagnostics were considered.
- Deep Reviewer decisions should be structured as `approved`, `needs_revision`, or `blocked`. `needs_revision` must create a review ticket with issue description, affected files, severity, reproduction details, expected outcome, allowed repair scope, and retry count.
- When validation or deep review fails, the Manager should create a Repair Worker scoped to the repair ticket. The Repair Worker patches the integration candidate, validators rerun, and the original Deep Reviewer verifies the original issue.
- Automatic repair is enabled only inside the existing task scope, repair ticket scope, and retry limit. Any repair that needs new product intent, new files outside scope, higher-risk authority, or broader capabilities must stop and escalate.
- The original Deep Reviewer context should be preserved across repair verification. Replacing the reviewer should be reserved for unavailable reviewer state and should be recorded as a trace risk.
- Repair verification outcomes should be structured as `fixed`, `partially_fixed`, or `not_fixed`.
- Repair loops should be capped at three cycles. If the retry limit is exceeded, the task becomes a blocked outcome and the Manager generates an escalation report for the user.
- The session should keep handoff records between agents. A handoff record should include source role, target role, task ID, context summary, accepted assumptions, produced artifacts, trace links, permission scope, and reviewer findings when applicable.
- The mainline-ready change set should mean the reviewed integration result that is eligible for user approval and editor application. Worker drafts, rejected revisions, and failed repair attempts should remain in trace history but must not be applied.
- Manager, Worker, and Reviewer prompts should share the same structured policy vocabulary so that tool permissions, sandbox boundaries, and review criteria are consistent across agents.
- The model provider should stay behind a trait. Provider implementations can be added for hosted APIs and local models without changing editor tool execution.
- The current operation enum is a useful seed, but execution should be routed through a tool layer that validates arguments and permission policy before mutating project state.
- Read operations and write operations should be explicitly separated. Read-only prompts should not need a write policy.
- Write operations should support two distinct phases: isolated git-backed AI task workspace writes for agent execution and transactional editor writes for reviewed, validated, user-approved merge application. Direct AI writes to the active project are not a supported execution mode.
- All scene mutations should go through editor command, transaction, or undo-aware services where practical. Direct scene mutation should be reduced over time.
- File reads and script writes must be sandboxed to canonical project roots and asset roots. Relative paths using parent traversal must be rejected.
- Script generation should start with Rhai because the repository already has a Rhai backend and script asset creation path. Python script support can follow once runtime/editor script integration is stronger.
- The Copilot should present a plan and proposed operations before applying write operations. The first implementation can use a single apply button for the full reviewed integration result, with partial acceptance added as the review UI matures.
- Auto mode should be a bounded execution mode, not a permission bypass. It may skip routine user interruptions, but it must not skip deterministic policy, capability checks, validators, audits, review gates, immutable bundle creation, stale-context checks, or rollback planning.
- Auto mode should expose a simple safety contract in the UI: what it may do without asking, what always requires final approval, what requires step-up approval, what is never allowed, and what residual risks remain.
- Enterprise policy should define risk routing tables for low, medium, high, and critical actions. Low and medium actions may be automatically executed inside the task workspace; high actions require stronger audit and step-up confirmation; critical actions require organization approval or explicit human approval before execution or apply.
- Before merge, the user review layer should present a diff viewer, scene preview, asset preview, risk summary, validation summary, review findings, and final report.
- User review decisions should support approve, reject, partially accept, and request further revision.
- Approved changes should enter the active project through a transactional merge using editor APIs. The editor should not switch branches directly, force checkout, or require the user to understand git to apply or undo AI work.
- Every approved merge should create a transaction bundle containing modified files, asset operations, and scene operations. Rollback should restore project state through editor affordances without requiring git knowledge.
- Partial acceptance should produce a new scoped integration candidate or transaction bundle rather than merging unreviewed fragments directly.
- Every operation should create a trace record with the tool name, input summary, result summary, and recovery hint.
- Every agent transition should create a trace record with the role, task ID, input summary, output summary, permission scope, review state, and recovery hint.
- Trace records should include manager decisions, immutable snapshots, worker executions, prompts, tool calls, local review actions, deterministic validation results, deep review actions, repair history, user decisions, and transactional merge outcomes.
- Console diagnostics should receive parse failures, validation failures, execution failures, and provider failures.
- The editor UI should expose a Copilot panel with chat history, current context summary, proposed plan, apply/reject controls, and trace.
- The editor UI should be able to show cluster progress for long-running requests: Manager planning, active Workers, Reviewer status, revision loops, and the current mainline-ready change set.
- The Manager should generate a final task report before user approval. The report should include summary, modified files, modified scenes, assets added, assets removed, logical change groups, validation results, review findings, repaired issues, unresolved concerns, risk assessment, worker actions, review history, and repair history.
- Provider credentials and model configuration should live in user/editor preferences or environment configuration, not in project manifests or example project files.
- The structured model output format should be strict enough for deterministic parsing. Free-form assistant prose should not be treated as executable.
- The system prompt should describe Aster's current component model, allowed operations, feature constraints, and output schema.
- The project context sent to the model should be concise and structured: scene hierarchy, selected entity, component summaries, asset index, relevant script snippets, diagnostics, and available commands.
- Large file reads should be deliberate tool calls, not automatic context stuffing.
- Context builders should be deterministic where practical and should produce role-specific context packets with explicit source references and trust labels.
- PRD and task-generation prompts should emit structured artifacts with objective, assumptions, non-goals, tasks, acceptance criteria, review rubric, required evidence, risks, open questions, and blocked conditions.
- Review prompts should receive the review rubric and required evidence generated for the task, then evaluate only objective artifacts. If the rubric is missing or too vague, review should return `blocked` or `needs_revision` before apply.
- AI task workspace management should use git worktrees for write-capable workflows. Read-only Copilot workflows may run without git because they do not create task workspaces or mutate project state.
- Process execution and outbound network tool calls are out of the first user-facing scope except for the model provider itself.
- Multi-agent orchestration should start in-process and deterministic. Distributed agents, background daemons, or remote worker pools are future implementation details, not requirements for the first cluster milestone.
- The trusted core (Capability Issuer, Deterministic Static Auditor, Script Preprocessor, sandbox policy, risk classifier, transaction bundle verification) must be implemented behind a strict module boundary with an IPC-ready API from the start. Accept only owned, schema-validated structs; never hold references to untrusted subsystem data; return only signed, immutable result types.
- The Policy Daemon (trusted core in a dedicated process with minimal dependencies, reached via local Unix domain socket) should be prioritized before Auto mode handles real user projects with third-party scripts or plugins. The daemon is the sole signer of capability grants and transaction bundles.
- **Hard rules and Workers serve different purposes and are not interchangeable:**
  - Hard rules (Capability Issuer, sandbox, deterministic validators, Static Auditor) prevent LLM errors — malformed output, out-of-scope tool calls, path escapes, high-risk syscalls, schema violations. They are mechanical, fast, non-negotiable, and do not need to understand intent.
  - Workers (AI agents with task context) prevent third-party intent drift — MCP servers that abuse legitimate access, skills that guide the agent off-task, outputs that are well-formed but semantically wrong. They exercise judgment that hard rules cannot.
  - MCP servers must never call tools directly. Only Workers call tools. MCPs communicate exclusively with their assigned Worker. The Worker evaluates MCP output against task context and decides whether to forward it as tool calls. The Worker's tool calls pass through hard rules as a second line of defense.
- **Wide permission is not the same as trusted permission:** Enterprise workflows may grant large task-local authority, but the grant remains bounded by task, workspace, time, evidence, risk route, trace, and rollback. No broad grant can directly apply to the active project or become reusable authority outside its signed task.

## Testing Decisions

- Tests should validate external behavior: parsed operations, accepted and rejected tool calls, sandbox boundaries, editor state changes, undo or transaction outcomes, diagnostics, and trace records.
- Use deterministic model stubs for agent session tests. No test should require a real model provider or network access.
- Parser tests should cover clean JSON, fenced JSON, surrounding text, malformed output, unknown actions, missing required fields, and unsupported component types.
- Sandbox tests should cover allowed project paths, denied parent traversal, denied absolute paths outside the project, symlink-sensitive canonicalization, allowed command prefixes, denied commands, and network disabled defaults.
- Tool validation tests should verify that read-only policy accepts read tools and rejects write tools, task workspace policy allows isolated git-backed writes, and direct active-project writes are rejected.
- Scene operation tests should cover creating objects, adding components, modifying properties, removing components, and destroying objects through the approved execution path.
- Script tool tests should cover creating a script under the asset root, updating an existing script, rejecting writes outside the asset root, and emitting diagnostics for invalid paths.
- Command execution tests should cover registered commands, unavailable commands, missing commands, and undo stack integration.
- Outcome tests should verify that completed sessions report summaries, partial failures are recorded, and trace entries are produced in operation order.
- Manager tests should cover task decomposition, worker assignment, permission scoping, handoff creation, and final plan assembly from multiple Worker outputs.
- Manager oversight tests should verify that Manager plans, scope requests, merge decisions, final reports, and claimed safety properties are rejected or downgraded when contradicted by policy, capability grants, validator output, audit output, reviewer findings, bundle hashes, stale snapshots, or actual diffs.
- PRD-generation tests should verify draft PRDs include assumptions, non-goals, risks, open questions, acceptance criteria, and source context packet IDs, and that generated PRDs cannot grant permissions or authorize execution.
- Task-generation tests should verify task tickets include objective, allowed scope, forbidden scope, acceptance criteria, review rubric, required evidence, context packet IDs, repair policy, retry limit, and scope hash.
- Task-minimization tests should verify broad tasks are split by artifact boundary, tool boundary, risk class, and unrelated user outcomes before capability issuance.
- Review-rubric tests should verify missing, vague, subjective, or artifact-free review criteria block execution or require revision.
- Snapshot tests should verify that Workers receive immutable project revision, scene selection state, asset index version, script references, diagnostics baseline, and editor state metadata, and do not query mutable live project state.
- Context-packet tests should verify role-specific packets include only scoped facts, snippets, IDs, evidence, trust labels, context hashes, and source references, and exclude unrelated PRDs, sibling Worker reports, rejected drafts, prior chat, and full project files by default.
- Task workspace tests should verify task ID creation, git availability checks, repository initialization checks, isolated worktree creation, branch naming, and unchanged active project state while Workers run.
- Worker tests should verify that workers cannot request tools outside their delegated scope, cannot mutate outside their assigned roots, and report structured outputs for success, partial failure, and blocked tasks.
- Worker isolation tests should verify that write operations land in the assigned git-backed task workspace, that direct mainline writes are rejected by default, and that task-bound scopes are enforced for reads, writes, commands, and tool calls.
- Capability tests should verify Scene Worker, Script Worker, Diagnostics Worker, Explain Worker, and Repair Worker permissions against allowed and forbidden operations.
- Capability grant tests should verify that every Worker tool call requires a valid grant hash, stale grants are rejected, missing grants are rejected, ungranted tools and commands are rejected, unauthorized read and write paths are rejected, and capability escalation requests are routed through the Manager rather than executed directly.
- Dynamic authorization tests should verify that broad task-local grants can be approved only when they are bound to task ID, workspace, expiration, evidence requirements, risk route, trace, and rollback, and that they cannot be reused by sibling Workers, later tasks, MCP servers, or active-project apply.
- Enterprise risk-routing tests should verify automatic execution for low and medium risk, audit and step-up confirmation for high risk, and organization or human approval for critical risk.
- Agent peer-review tests should verify that Reviewers, Risk Auditors, and adversarial reviewers can raise risk, request narrowing, require evidence, or block, but cannot mint permissions, lower deterministic risk classification, or override the Capability Issuer.
- Prompt-injection tests should verify that user prompts, project files, script comments, string literals, command labels, plugin metadata, Worker reports, Reviewer reports, and prior model messages cannot alter system policy, tool schemas, task scope, permission grants, validation requirements, approval requirements, or Reviewer rubric.
- Context-drift tests should verify that Worker, Reviewer, Risk Auditor, final report, and transaction bundle actions remain bound to `task_id`, `snapshot_id`, `base_revision`, `scope_hash`, `context_packet_id`, `context_hash`, and capability or bundle hash, and that stale snapshots, stale context packets, or changed active editor state invalidate prior approval.
- Task-drift tests should verify that out-of-scope diffs, adjacent improvements, unauthorized files, unauthorized entities, unauthorized assets, unauthorized commands, and unauthorized operation types are rejected even when the candidate otherwise passes validation.
- Command capability tests should verify that editor commands are not AI-callable by default, only `ai_safe` commands with schema, effect kind, sandbox contract, risk level, and undo or rollback contract are exposed, and command labels or plugin descriptions cannot grant authority.
- Script audit tests should verify deterministic extraction of AST or syntax structure, imports, API calls, filesystem access, network access, process execution, dynamic eval usage, dependency changes, credential access, resource-risk indicators, and prompt-injection indicators from third-party and generated scripts.
- Script preprocessing tests should verify that comments and string literals are treated as untrusted evidence, raw source snippets are quoted and trust-labeled, and sanitized summaries cannot become executable instructions.
- Risk Auditor tests should verify that the secondary model audit is monotonic: it can increase risk, request human confirmation, or recommend blocking, but cannot approve validator failures, bypass policy, grant permissions, or lower deterministic risk classifications.
- Local review tests should cover approved, needs-revision, and blocked Worker outputs, including review findings and risk tags.
- Integration tests should verify that only locally approved Worker outputs can merge into the integration candidate and that final review uses the integrated result rather than individual Worker drafts.
- Deterministic validation tests should cover build or compile validation, type checking, linting, asset references, GUID consistency, scene schema validation, editor load validation, dependency validation, and performance budget validation.
- Validation failure tests should verify that deterministic failures create repair tickets and bypass reviewer discussion until repaired.
- Deep Reviewer tests should cover approved work, missing acceptance criteria, unsafe operations, hidden side effects, malformed integrated output, architecture-policy violations, gameplay impact, scene consistency, API correctness, performance risks, regression risks, security risks, editor workflow impact, and rejection of Worker claims that are not supported by objective artifacts.
- Revision-loop tests should verify that Reviewer findings create scoped repair tickets, Repair Workers patch the integration candidate, validators rerun, the original Reviewer verifies the original issue, retry counts are traceable, and rejected drafts never become applyable mainline operations.
- Automatic-repair tests should verify routine failures can be fixed inside scope without user prompts, while repairs needing broader context, new product intent, higher-risk authority, or out-of-scope files become escalation or blocked outcomes.
- Retry-limit tests should verify that more than three failed repair cycles create a blocked outcome and escalation report.
- Transactional merge tests should verify that approved changes merge through editor APIs without branch checkout or force operations, transaction bundles apply atomically, post-apply verification reloads scenes and rescans assets, and failed apply or verification rolls back file, asset, scene, settings, and dirty-state changes.
- Transaction bundle tests should verify bundle hashes, before and after artifact hashes, rollback journals, logical change groups, audit reports, validation reports, review reports, user approval records, and rejection of tampered or stale bundles.
- Partial acceptance tests should verify that partial approval creates a new scoped integration candidate and transaction bundle, reruns validation and audit, and never applies unreviewed fragments from a larger bundle directly.
- User approval hardening tests should verify that user approval cannot override failed policy, failed validators, failed audits, out-of-scope diffs, missing rollback contracts, stale snapshots, stale bundle hashes, or stale validation reports.
- Step-up approval tests should verify that high-risk bundles require explicit per-risk confirmation tied to the immutable bundle and that approval UI text is derived from trusted metadata rather than model or Worker prose.
- Auto-mode tests should verify that automation can proceed through low-risk planning, Worker execution, validation, review, and repair without routine user prompts, but cannot apply to the active project without an eligible immutable bundle and required approval.
- Auto-mode fail-closed tests should verify that missing policy evidence, stale context, validator failure, audit failure, reviewer block, missing rollback plan, tampered trace, or Manager/report mismatch prevents apply and produces a blocked or review-required outcome.
- Git prerequisite tests should verify missing-git guidance, non-repository project guidance, write workflow blocking, and read-only Copilot availability before git setup is complete.
- Final report tests should verify summary, changes, logical change groups, validation results, review findings, risk assessment, traceability, repaired issues, and unresolved concerns.
- UI tests should focus on state transitions for prompt submission, plan preview, apply, reject, provider error, and operation failure. They should not assert visual implementation details.
- Cluster UI tests should focus on state transitions for Manager planning, snapshot creation, task workspace creation, Worker running, local review, integration, validation, Deep Reviewer approval, Deep Reviewer rejection, repair loop, final report, user approval, partial acceptance, rejection, and final mainline-ready plan.
- Existing smoke paths around `agent-tools` should remain and expand into a repository automation command once the feature becomes user-visible.
- Copilot mode tests should verify single-agent direct tool execution, per-operation user approval gating, auto-accept pre-approval for low-risk and medium-risk operations within scope, and step-up confirmation for high-risk operations regardless of auto-accept state.
- Auto mode tests should verify Manager decomposition, parallel Worker execution in fresh sessions, automated policy-issued capability grants, local review of Worker outputs, integration candidate assembly, deterministic validation, Deep Reviewer inspection in a fresh session, and repair loop execution.
- Problem-reporting tests should verify that unrecoverable Worker failures, out-of-scope needs, ambiguous conditions, and exhausted retries appear in the final report as unresolved issues with severity, affected artifacts, and quick-fix actions.
- Quick-fix tests should verify that triggering a quick-fix action launches a scoped Auto mode task, reuses the original snapshot and workspace, produces a mini integration candidate, and appends results to the original final report.
- Fresh-session tests should verify that Workers, Repair Workers, Deep Reviewers, and Risk Auditors receive only their role-specific context packet and are excluded from Manager conversation history, sibling Worker chat, prior repair reasoning, and raw user prompt text.
- Mode-selection tests should verify that read-only questions use the lightweight single-agent path regardless of active mode, and that the system can suggest Auto mode for requests spanning multiple artifact boundaries or risk classes.

## Third-Party Skills And MCP Servers (Future)

A future milestone will allow users to import third-party skills (agent instruction files) and MCP servers (external tool-providing processes) into the Copilot. These are fundamentally different things with different threat models and different security approaches.

### Skills Are Text, Not Code

A "skill" in this ecosystem is a text file — markdown with YAML frontmatter — containing agent instructions, tool usage guidance, and workflow descriptions. It is not executable, not compiled, not a binary. It is a prompt that gets injected into an agent's context.

The threat is not memory corruption or syscall abuse. The threat is **prompt manipulation**: a malicious or poorly-written skill that convinces the agent to ignore safety rules, skip validation, broaden its scope, exfiltrate data through tool outputs, or mislead the user.

### Skill Declaration: Frontmatter as Contract

Every third-party skill must ship with machine-parseable YAML frontmatter that serves as the system-enforceable contract:

```yaml
name: "player-controller-builder"
version: "1.2.0"
description: "Generates a player controller with input handling and camera follow"
author: "community-author"
trust_tier: "sandboxed"
required_tools:
  - scene_mutate
  - script_create
  - asset_read
allowed_paths:
  read: ["assets/scripts/player/", "assets/scenes/"]
  write: ["ai-workspace/scripts/"]
required_commands: []
network: false
resource_limits:
  max_tool_calls: 30
  max_files_read: 20
input_schema: "player_controller_request.json"
output_schema: "scene_script_bundle.json"
content_hash: "sha256:abc123..."
```

**The frontmatter is trusted by the system.** It is machine-validated: the Capability Issuer checks that each `required_tools` entry matches a known AI-safe tool, that `allowed_paths` resolve to canonical project roots, that `network: false` is consistent with the skill's trust tier. If the frontmatter fails validation, the skill is rejected before any agent ever sees its instruction body.

**The instruction body (everything below the frontmatter) is untrusted.** It is injected into the agent's context under an `UNTRUSTED_SKILL_INSTRUCTION` label. It may suggest, explain, and guide — but it cannot authorize, cannot grant tools, cannot waive validation.

### Skill Review: What Can Be Checked

Review for text-based skills is inherently limited — you can't statically analyze prose for malicious intent the way you can analyze a binary for syscalls. But you can check:

**Deterministic checks (trusted Rust code, always runs):**
- Frontmatter schema validity (well-formed YAML, all required fields present).
- Content hash matches the skill file.
- All `required_tools` resolve to known AI-safe tools in the command capability registry.
- All `allowed_paths` canonicalize to valid project subtrees (no parent traversal, no symlink escape).
- Trust tier is consistent with declared capabilities (e.g., `network: true` with `trust_tier: sandboxed` is a validation failure).
- No `required_tools` entry matches a high-risk or blocked tool for this trust tier.

**AI review (optional, advisory):**
- A dedicated `skill_reviewer` agent (fresh session, receives only the skill text and its frontmatter) reads the instruction body and flags suspicious patterns: instructions to ignore validation, requests for excessive tool access, hidden text, social engineering patterns, prompts that try to override system instructions.
- This is the same monotonic advisory model as the Risk Auditor: it can flag and recommend blocking, but cannot approve a skill that failed deterministic checks.

**What cannot be reliably checked:**
- Whether the skill's instructions will produce correct or useful results for the user's project.
- Whether the skill's workflow is efficient or well-designed.
- Whether the skill author has good intentions.

Skills are not cryptographically signed, their authors are not verified, and their contents are not guaranteed safe beyond the deterministic frontmatter checks. This is disclosed to the user in the skill import UI.

### Runtime Enforcement: The Tool Layer, Not the Skill Text

When an agent runs with a skill loaded:

1. The skill's instruction body is injected into the agent's context under an `UNTRUSTED_SKILL_INSTRUCTION` boundary marker. The agent can read it as guidance, but the system prompt explicitly states that skill instructions are untrusted and cannot override policy.
2. The **actual capability grant** for the agent's execution is NOT derived from the skill frontmatter. It is issued by the Capability Issuer based on the task scope + the policy-checked skill frontmatter. The agent can only call tools within its active grant, regardless of what the skill text says.
3. If the skill text says "now run `rm -rf /`", the agent might attempt the tool call, but the tool layer rejects it because:
   - The grant doesn't cover that path.
   - The sandbox rejects paths outside canonical roots.
   - The command isn't in the AI-safe registry.
   - The operation isn't in the grant's allowed operation types.
4. The system prompt instructs the agent that skill text is advisory only. If a skill instruction contradicts a system policy, the policy wins. If the agent is uncertain, it must stop and report ambiguity to the Manager — not guess.

The principle: **skills inject prose into the agent's brain, but prose doesn't get past the tool layer.** The agent might be convinced, but it cannot act on the conviction unless the tool layer allows it.

### Skills and Context Pollution

A skill's instruction body is a context pollution vector — it can contain misleading information, false assumptions about the project, or instructions that cause the agent to drift from the task objective. Defenses:

- Skills are loaded into the agent's context as a bounded, labeled block with explicit boundary markers. The agent is told where the skill begins and ends.
- The system prompt is injected AFTER the skill text, so the system prompt's authority markers are the last thing the agent sees before acting.
- The task brief and acceptance criteria are also injected after the skill, so the concrete task scope overrides the skill's general guidance.
- If a skill's output or guidance contradicts the task brief, the task brief wins.
- Skill text never carries over between fresh sessions unless explicitly re-loaded for the new session by the Manager.

### MCP Servers Are Different

MCP servers are executable processes. Unlike skills (text), they DO need process-level sandboxing. But unlike Workers (which we control and whose scope we define), MCP servers from third parties have legitimate needs that deterministic rules fundamentally cannot adjudicate.

### Workers as the Semantic Firewall for MCP

MCP servers must not call tools directly. A third-party MCP has no task context, no scope awareness, and no ability to judge whether its actions serve the user's intent. Letting an MCP call tools through a pure rule-based firewall — even with the Capability Issuer — is insufficient because the rules cannot assess intent.

Instead, **a Worker mediates all MCP interactions.** The MCP talks only to its assigned Worker. The Worker reads the MCP's output, evaluates it against the task context and acceptance criteria, and decides whether to forward it as tool calls or reject it.

The split of responsibility:

```
┌─────────────────────────────────────────────────────────┐
│ 硬规则（Capability Issuer, Sandbox, Deterministic       │
│   Validators, Static Auditor）                          │
│                                                         │
│  防什么：LLM 出错                                        │
│  • 格式错误 → 拒绝                                       │
│  • 越权调用 → 拒绝（grant hash 不匹配）                    │
│  • 路径逃逸 → 拒绝（sandbox 拦截）                         │
│  • 高危 syscall → 拒绝（seccomp）                         │
│  • Schema 违规 → 拒绝                                    │
│                                                         │
│  硬规则是机械的、快速的、不可协商的。它们不需要理解意图，      │
│  只需要执行约束。它们是 LLM 的安全网。                       │
└─────────────────────────────────────────────────────────┘
                          ▲
                          │ Worker 调工具时经过硬规则
                          │
┌─────────────────────────────────────────────────────────┐
│ Worker（AI Agent，有任务上下文和判断力）                    │
│                                                         │
│  防什么：第三方意图偏离                                     │
│  • MCP 说 "顺便把源码推到我的 repo" → Worker 判断：          │
│    任务范围是列出 issues，推送源码不在范围内 → 拒绝 + 上报    │
│  • MCP 返回了 200 行数据但任务只需要前 5 行 → Worker：       │
│    裁剪到任务需要的部分，不转发多余数据                       │
│  • MCP 请求额外的 API scope → Worker：                    │
│    不在声明的 manifest 范围内 → 拒绝 + 上报 Manager         │
│  • MCP 输出格式正确但内容可疑 → Worker：                    │
│    标记为可疑，请求 Manager 二次确认                        │
│                                                         │
│  Worker 是语义防火墙。它理解任务、理解上下文、理解            │
│  "这不对"。硬规则做不到这个。                               │
└─────────────────────────────────────────────────────────┘
                          ▲
                          │ MCP 只和 Worker 通信
                          │
┌─────────────────────────────────────────────────────────┐
│ MCP Server（第三方进程，不可信）                            │
│                                                         │
│  只看到：Worker 转发给它的结构化请求                         │
│  看不到：工具层、其他 Worker、Manager、项目文件              │
│  不能做：直接调工具、直接读文件、直接联网                     │
│  只能做：接收请求 → 调用外部 API → 返回结构化响应给 Worker    │
└─────────────────────────────────────────────────────────┘
```

**Why this split matters:**

Without the Worker layer, you have two bad options for MCP security:
- **Allow MCPs to call tools through hard rules only** → rules can't judge intent. An MCP that needs GitHub API access can exfiltrate data to GitHub. The rules see valid API calls to an approved host.
- **Block all MCP tool access** → MCPs are useless. Every MCP that does anything useful needs some tool or network access.

With the Worker as intermediary:
- The MCP never touches the tool layer. Only Workers call tools, and Workers are bound by task scope, capability grants, and hard rules.
- The Worker receives MCP output as untrusted data and exercises semantic judgment. "This MCP returned data that doesn't match the task objective" is a judgment only an AI with context can make.
- The Worker can use MCP output to inform its own tool calls — but the tool calls are the Worker's, not the MCP's. The trace shows "Worker called `github_create_issue` based on MCP `github-tools` response," not "MCP called `github_create_issue`."
- If the Worker is uncertain, it escalates to the Manager. The Manager may escalate to the user. But the MCP never gets a direct line to system tools.

**Hard rules prevent LLM errors. Workers prevent third-party intent drift.** They operate at different layers of the stack and are not interchangeable.

### The MCP Trust Cliff

MCP servers commonly require:
- **Network access** — to call external APIs (GitHub, Slack, databases, package registries).
- **Filesystem access** — to read project files, write outputs, or manage configuration.
- **Credentials** — API tokens, SSH keys, or service account secrets to authenticate with external services.

These are not bugs or edge cases. They are the reason the user installed the MCP. But from a syscall perspective, "this MCP is calling the GitHub API to create a valid PR" and "this MCP is exfiltrating the project source to a personal repo" are **indistinguishable**. Both connect to `api.github.com`, both send data, both use the GitHub token the user provided.

Deterministic rules hit a hard limit here. You cannot:
- Block network access (the MCP needs it to function).
- Restrict filesystem access to a narrow subtree (the MCP may legitimately need broad project access).
- Refuse to provide credentials (the MCP is useless without them).
- Statically audit the MCP binary for intent (intent is not in the binary; it's in the author's head).

This is the **MCP trust cliff**: at some point, security cannot be provided by rules. It must be provided by the user making an informed decision about whether to trust the MCP author.

### What the System CAN Do

When rules cannot guarantee safety, the system shifts from "prevent bad things" to "contain blast radius, inform the decision, and enable detection."

**1. Structured Consent at Install Time (Not Per-Call)**

When a user imports an MCP server, the system presents a single, clear consent screen — not a series of per-call prompts:

```
MCP Server: "github-tools"
Author: community-author
Trust tier: network-capable

This MCP requests:
  ✓ Network access to: api.github.com
  ✓ Read access to: entire project
  ✗ Write access: not requested
  ✓ Credential: GitHub Personal Access Token (scope: repo, workflow)

Risks:
  • This MCP can read all files in your project and send them to api.github.com.
  • It receives a GitHub token with repo scope — it can create/delete repos,
    push code, and modify workflows on your behalf.
  • The system cannot distinguish legitimate GitHub API calls from data exfiltration
    disguised as legitimate API calls.

What the system guarantees:
  • Network connections are restricted to api.github.com only.
  • The MCP runs in a seccomp-ed subprocess with no child process spawning.
  • All MCP tool calls are logged in the trace for audit.
  • The credential is stored scoped to this MCP and revocable at any time.

[Cancel] [Install with these permissions]
```

The user makes one decision: trust this author with these capabilities, or don't. The system does not ask again for the session.

**2. Credential Isolation — Scoped, Revocable, Never Raw**

MCP servers never receive the user's raw credentials. Instead:
- The user registers credentials with the Editor's credential store (OS keyring or encrypted local store).
- When installing an MCP, the user selects which credential to grant, with what scope.
- The credential store injects the credential into the MCP process's environment at launch time, scoped to that process only.
- The credential store logs every credential access: which MCP, when, what scope.
- The user can revoke an MCP's credential grant at any time from the Editor preferences. Revocation takes effect on the next MCP launch.
- MCPs cannot enumerate available credentials, request credential scope expansion at runtime, or access credentials granted to other MCPs.

**3. Network Egress Enforcement**

The MCP process's network access is enforced by seccomp-bpf and firewall rules:
- Outbound connections are restricted to the exact hosts declared in the MCP manifest.
- DNS resolution is intercepted and validated against the declared host list.
- If the MCP declared `api.github.com` and attempts to connect to `evil-server.com` or even `github-evil-lookalike.com`, the connection is blocked at the kernel level.
- IP addresses, port ranges, and protocols are similarly restricted to the declared set.

What this does NOT protect against: the MCP sending project data to `api.github.com` (its declared and approved host) in a way that exfiltrates rather than serves the user's intent. Network egress control can say "only talk to GitHub." It cannot say "only talk to GitHub for legitimate reasons."

**4. MCP Protocol Audit Log**

Every MCP tool call and its response is captured in the trace log:
- Tool name, input parameters, output size, latency, success/failure.
- For high-risk MCPs: the actual request and response payloads are logged (subject to a configurable retention policy — logs can be large).
- The audit log is human-readable and searchable from the Copilot trace panel.
- Unusual patterns (e.g., an MCP that normally returns 2KB responses suddenly returning 2MB) are flagged as anomalies in the trace UI.

This does not prevent abuse. It enables the user to detect it after the fact — and to make an informed decision about whether to keep using the MCP.

**5. Trust Tiers With Escalating Consent**

| Tier | Capabilities | Consent required | Audit level |
|------|-------------|-----------------|-------------|
| **Filesystem-only** | Read/write declared project paths, no network, no credentials | Single confirmation at install | Tool calls logged |
| **Network-declared** | Network to declared hosts, read project, no credentials | Explicit host list confirmation at install | Tool calls + response sizes logged |
| **Network + credentials** | Network + scoped credentials + filesystem read | Step-up: user must re-enter project name, confirm credential scope, acknowledge data exfiltration risk | Full request/response payload logging |
| **Arbitrary process** | Can spawn child processes | Not allowed for third-party MCPs in the initial milestone | N/A |

The consent UI for each tier must be honest about what the system cannot protect against. It must not use language like "this MCP is safe" or "verified by Aster." It must say "you are trusting the author of this MCP with these capabilities. Aster cannot prevent intentional misuse within the declared scope."

**6. The Honest Answer**

For an MCP that requires network access + file read + a GitHub token, there is no technical mechanism that can prevent the MCP author from exfiltrating the project to their own repo. The system can:
- Restrict the network to `api.github.com` ✓
- Scope the credential to `repo` scope ✓
- Log every call for post-hoc audit ✓

But it cannot:
- Tell whether that `git push` was what the user wanted or what the MCP author wanted ✗
- Inspect the MCP's binary to determine intent ✗
- Prevent a determined malicious author from abusing granted capabilities ✗

This is not a failure of the architecture. It is the nature of running third-party code with legitimate broad capabilities. The Editor's responsibility is to make the trust decision explicit, to minimize the blast radius, and to provide the tools for the user to audit and revoke. The final safety guarantee for MCP servers is: **you can always revoke the credential and uninstall the MCP, and everything it did is in the audit log.**

### Why Firecracker Is Not the Answer

Firecracker targets multi-tenant serverless isolation (AWS Lambda). For local editor extensions:
- Skills are text — they need context-level isolation, not VM isolation.
- MCP servers are subprocesses — they need seccomp/Landlock, not a full microVM with guest kernel.
- Firecracker requires KVM, adds ~100ms boot latency per invocation, and brings image management complexity disproportionate to the threat.

WASM sandboxing (previously mentioned) is also not needed — skills are text, not code. The correct primitives are context boundary markers + Capability Issuer enforcement for skills, and seccomp + Landlock for MCP processes.

## Out of Scope

- Runtime gameplay AI or NPC decision-making.
- In-game LLM inference during packaged builds.
- Autonomous background agents that modify projects without user approval.
- Remote distributed worker pools or unattended cloud agent execution.
- Direct AI writes to the active project mainline.
- Branch switching, forced checkout, or raw git merge flows as the user-facing apply mechanism.
- Arbitrary shell command execution.
- Arbitrary network browsing or asset downloading by the agent.
- Full multi-file refactoring outside the project asset and scene model.
- Marketplace, cloud collaboration, or remote team review workflows.
- Training or fine-tuning a model for Aster.
- Replacing the editor inspector, hierarchy, project panel, or script editor.
- Guaranteeing that generated gameplay code is design-perfect or production-ready.
- Third-party MCP server and skill import in the initial milestones (the architecture anticipates it; implementation is a future milestone).

## Further Notes

The current repository already points in the right direction. The main product risk is not model integration; it is uncontrolled mutation. The Copilot should therefore be built around trusted tool boundaries, task-bound dynamic permissions, preview-before-apply behavior, undo or transaction support, and traceability. Enterprise automation should support broad task-local execution when policy can bind it to workspace, evidence, risk route, and rollback.

The first milestone should prove the Copilot mode vertical slice: ask for a player controller, preview the plan, create a player object, write a `.aster` Aster Script file, run final script acceptance, attach it as a script component, show diagnostics and trace, then undo the applied changes. Include the auto-accept toggle as a session-level convenience.

The second milestone should harden the shared tool layer: canonical path sandboxing, policy enforcement, transaction boundaries, structured failures, grant hashes, risk classification, and deterministic tests. Both modes use the same tool layer.

The third milestone should make Copilot mode feel native in the editor: Copilot panel, context summary, plan review, apply/reject controls, auto-accept toggle, trace view, provider settings, and saved preferences.

The fourth milestone should introduce Auto mode behind the same visible Copilot workflow: Manager decomposes a larger authoring request, specialized Workers produce bounded outputs in fresh sessions, policy automatically issues task-bound grants, Deep Reviewer validates the assembled result, problems are reported (not skipped), and the final report includes unresolved issues with quick-fix actions before anything becomes eligible for user approval.

The fifth milestone should turn Auto mode into a local pull-request-style workflow: immutable task snapshot, isolated git-backed AI task workspace, `ai/task-*` branch convention, logical Worker commits, fresh-session isolation, automatic permission and command review, peer-agent review for risk and quality, local review, integration candidate, deterministic validation, Deep Review, repair loop, final report with quick-fix actions, user review, and transactional merge into the active project.

The sixth milestone should harden enterprise governance: risk routing tables, critical-action organization approval hooks, richer command manifests, adversarial reviewer option, revocation UI, and audit-log export.

The seventh milestone should harden the trust boundary with process isolation: extract the trusted core (Capability Issuer, Deterministic Static Auditor, Script Preprocessor, risk classifier, transaction bundle verifier) into the Policy Daemon process, communicating over a local Unix domain socket with a narrow, schema-validated IPC protocol. The daemon is the sole signer of grants and bundles.
