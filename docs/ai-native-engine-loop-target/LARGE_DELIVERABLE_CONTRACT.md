# Large Deliverable Contract

This goal run must produce a large, concrete engineering deliverable. A small
bugfix, a pretty plan, a renamed file, or a partial UI cleanup is not enough.

## Required Standard

The final result should be large enough that another engineer can compare this
branch against `origin/main` and `origin/takeover/upstream-integrated` and see a
real product/architecture difference.

The work must include meaningful progress across multiple areas:

```text
Quest/Agent execution
Frontend workbench refactor
SceneCommand/ScenePatch semantics
Physics/render/audio validation or diagnostics
Safety/apply/rollback policy
Tests/build evidence
Progress/comparison documentation
```

Do not complete only one area and stop.

## Frontend Refactor Requirement

The frontend is a first-class target, not optional polish.

Improve QuestPage/EditorPage toward a product-grade AI game editor workbench:

- reduce giant-page dumping grounds;
- extract components or hooks when it improves clarity;
- make run/review/apply states readable;
- improve artifact, diagnostics, and editor selection flows;
- make failure/validation states useful;
- keep the interface dense, professional, and fast to scan;
- run `cd editor && bun run build` after meaningful frontend changes.

Do not wait for the user to micro-design the UI. Use the repository docs and
remote branch references to make product decisions.

## Prompt Self-Improvement Rule

If the prompt pack is incomplete, outdated, or too weak, Claude may rewrite its
own execution plan before coding.

But self-rewriting has rules:

- it must not reduce scope;
- it must not turn the task into planning-only work;
- it must not remove frontend refactor requirements;
- it must not remove verification requirements;
- it must not remove remote branch comparison;
- it must produce a short executable plan and then immediately implement it.

If the prompt and code disagree, the code and verified repository state win.

## Size Expectations

A large deliverable usually includes several of these:

- new or extracted Rust modules with tests;
- meaningful Quest execution/review/apply changes;
- frontend component extraction or state cleanup;
- diagnostics or validation APIs for engine subsystems;
- focused tests or build fixes;
- evidence docs with real command output summaries;
- comparison against remote branches.

This does not mean reckless broad rewrites. It means sustained, coherent work
across the product loop.

## Stop Gate

Before stopping, confirm all of this is true:

```text
There are real code changes, not only docs.
Frontend was improved or explicitly blocked with evidence.
Quest/Agent execution moved closer to a real loop.
At least one engine semantics or validation path improved.
Verification commands were actually attempted.
progress/comparison docs were updated with evidence.
The branch is meaningfully comparable against remote references.
```

If any item is false, choose the next highest-value task and continue.

