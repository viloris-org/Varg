# Quest Workflow UI Reference

This document captures design observations from peer AI workbench references and translates them into Aster Quest UI direction. It complements `docs/ui-design-guidelines.md` and `docs/ai-editor-quest-prd.md`; it does not replace their product safety or state-machine requirements.

## Core Direction

Quest should feel like an AI workbench for durable game-making tasks, not a dashboard of agent internals.

The strongest reference pattern is:

- left: Quest queue and lightweight workspace navigation;
- center: agent workflow, current decision, and user instruction input;
- right: object-based artifact workspace for specs, files, knowledge, validation, and review evidence.

The center column answers "what is happening and what do I need to decide now?" The right column answers "what artifact or evidence am I inspecting?"

## Reference Lessons

### Execution Suggestion

Before execution starts, the AI can present a compact decision card such as:

- run directly;
- plan/spec first;
- ask clarifying questions;
- request permission.

The card should explain why the suggested path is appropriate. For broad, ambiguous, multi-file, or risky work, the primary action should usually be spec or plan first. For narrow low-risk work, the primary action can be run directly.

After the user chooses, the card should collapse into a small durable record, for example `Execution suggestion · Plan first`. This preserves the decision without keeping a large card in the feed.

### Agent Activity Feed

Running work should be represented as a readable activity feed, not as raw trace output.

Good feed layers:

- user request card;
- collapsed prior decisions;
- short `Thought` rhythm markers;
- natural-language progress statements;
- grouped tool activity;
- generated artifacts;
- required decisions.

Tool calls should be grouped by purpose. For example, a single `Explore rendering pipeline` group can contain memory search, file search, and file reads. Individual tool rows should be low-contrast metadata unless they are active or require attention.

### Active Execution Animation

The active tool group should have a subtle sense of motion so the user can tell work is still running.

Recommended animation:

- one active row or group at a time;
- flowing gradient on a left rail, top border, or background sheen;
- subtle icon pulse or rotation for the active step;
- newly appended sub-steps fade in and move up 2px to 4px;
- completed groups stop animating and become muted;
- `prefers-reduced-motion` disables shimmer and movement, keeping only a static active color.

Avoid animating historical rows. Motion is for current execution state, not decoration.

### Clarifying Questions

When the agent needs scope, priority, or constraints, Quest should show structured questions as an in-feed decision card rather than a modal or ordinary chat message.

The card should support:

- single choice;
- multi choice;
- free text;
- file or context references;
- recommended choices;
- pagination such as `1 / 2` for multiple questions;
- disabled continue action until required answers are complete.

The agent should explain why it is asking before the card appears. Choices should be specific to inspected project evidence, not generic options.

After submission, the question card should collapse into a `Questions Answers` evidence card that shows each prompt and answer. The next agent message should restate the understood scope and constraints before continuing.

Clarifications are not temporary chat. They are durable Quest evidence and should be referenced by generated specs and reviews.

### Temporary Agent To-Dos

Exploration may create short-lived to-dos. These are the agent's scratchpad, not user-managed Quest tasks.

Temporary to-dos should:

- render inside the relevant tool group or activity feed;
- show current and completed internal steps;
- collapse after completion into a summary such as `Updated to-dos · 4/5 done`;
- not populate the main Quest progress surface;
- not be treated as user tasks requiring review.

Formal Progress or Quest tasks should appear only after a stable plan, spec, review bundle, or user-facing task artifact exists.

### Artifact Workspace

The right pane should be object-based, not feature-tab-based.

Prefer temporary object tabs such as:

- `Spec.md`;
- `uniforms.rs`;
- `Knowledge: Tech Stack`;
- `Validation Log`;
- `Review Bundle`;
- `Changed File Diff`.

Avoid permanent right-pane tabs for implementation categories such as Overview, Intent, Spec, Review, Knowledge, Trace, Checkpoint, and Validation. Those are artifact types or evidence, not primary navigation.

Artifacts should have structured headers:

- title;
- type or category;
- status, maturity, risk, or validation state when relevant;
- source path or provenance;
- related Quest or project scope.

Artifact bodies should be rendered according to kind. Use structured layouts for knowledge, validation, findings, diffs, and review bundles. Raw JSON should be behind `View raw`, not the default presentation.

### Knowledge References

Knowledge references should be inspectable artifacts.

A useful knowledge artifact can show:

- scope;
- keywords as chips;
- scenarios;
- content;
- source path;
- maturity or reference status.

When an activity feed row references memory or knowledge, clicking it should open that artifact in the right pane.

## Aster Layout Model

### Left Rail

The left rail is a Quest inbox.

Recommended groups:

- Needs action;
- Running;
- Recent;
- Archived.

Quest items should show a compact state badge such as `Action Required`, `Running`, `Review`, `Blocked`, or `Done`. The rail should stay within the width constraints in `docs/ui-design-guidelines.md` and collapse on narrow screens.

### Center Surface

The center surface is state-driven.

Recommended states:

- Draft: request input and execution suggestion.
- Clarifying: structured question card.
- Running or validating: activity feed, current tool group, Stop, add instruction.
- Waiting for permission: permission request and Approve/Deny.
- Ready for review: outcome summary, risk, validation, changed files, Apply/Request revision/Discard.
- Blocked or failed: cause, recovery action, evidence link.
- Completed: accepted result, affected surfaces, restore/rollback if valid.

The activity feed should remain useful, but the main decision for the current Quest status must be visible without opening a secondary tab.

### Right Artifact Pane

The right pane is an artifact workspace. It may be closed by default when no decision depends on evidence.

Opening rules:

- spec written: open the spec artifact;
- memory reference clicked: open the knowledge artifact;
- validation failed: open validation evidence;
- review ready: open review bundle or first changed file;
- file row clicked: open the diff or source artifact;
- blocker clicked: open relevant evidence.

On narrow screens, the right pane should become an overlay or temporary focused surface.

### Bottom Input

The bottom input adapts to state:

- Draft: `Describe the outcome you want`.
- Running: `Add instruction or context`.
- Clarifying: optional free-form addition while structured answers are required.
- Review: `Ask for revision or explain what to change`.
- Blocked: `Provide missing info`.
- Completed: hidden or converted into a follow-up Quest action.

Context tokens such as `@file` and command tokens such as `/command` can remain, but they should not overpower the state-specific prompt.

## Data Model Implications

Quest UI should distinguish several record types.

```ts
type QuestClarification = {
  id: string;
  questions: Array<{
    id: string;
    prompt: string;
    kind: 'single_choice' | 'multi_choice' | 'free_text' | 'file_reference';
    options?: Array<{
      id: string;
      label: string;
      detail?: string;
      recommended?: boolean;
    }>;
    answers: string[];
    required: boolean;
  }>;
  summary: string;
  timestamp_ms: number;
};

type QuestActivityGroup = {
  id: string;
  label: string;
  summary?: string;
  status: 'running' | 'completed' | 'failed';
  rows: QuestActivityRow[];
  artifact_refs: string[];
};

type QuestEphemeralTodoUpdate = {
  id: string;
  group_id: string;
  items: Array<{
    label: string;
    status: 'pending' | 'running' | 'done';
  }>;
  durable: false;
};
```

Ephemeral to-dos should not be stored as formal Quest tasks unless the orchestrator promotes them into a user-facing task artifact.

## Visual Design Notes

Use a restrained workbench style:

- light or neutral editor surfaces;
- thin borders;
- low shadow;
- dense but readable spacing;
- state color only for badges, active execution, and primary CTAs;
- consistent icon set;
- visible keyboard focus states;
- no decorative motion unrelated to execution.

Recommended motion budget:

- 150ms to 220ms for row entry;
- 150ms to 300ms for expand/collapse;
- slow flowing gradient for active execution only;
- no layout-shifting hover effects;
- reduced-motion fallback for all animated states.

## Implementation Priorities

1. Replace permanent Quest right-panel feature tabs with object-based artifact tabs or an artifact drawer.
2. Add a state-driven center renderer for Draft, Clarifying, Running, Waiting, Review, Blocked, Failed, Completed.
3. Move review decisions into the center surface for `ready_for_review`.
4. Add structured clarification cards and persisted clarification evidence.
5. Render temporary agent to-dos inside activity groups instead of formal Quest progress.
6. Add active execution animation to the current tool group only.
7. Render knowledge, validation, findings, and diffs as structured artifacts instead of raw JSON.
8. Add responsive behavior: collapsible queue, overlay artifact pane, and no cramped three-column layout on narrow screens.

## Acceptance Criteria

- A new Quest can show an execution suggestion before running.
- A broad Quest can ask structured clarifying questions and persist answers.
- User answers collapse into durable evidence and influence the generated spec.
- Running work shows grouped activity with one animated active group.
- Temporary to-dos do not appear in formal progress unless promoted.
- Clicking activity evidence opens an artifact in the right pane.
- `ready_for_review` exposes Apply, Request revision, and Discard in the center surface.
- Raw traces and JSON are not the default artifact presentation.
- Reduced-motion users do not see shimmer, flowing gradients, or slide animations.
