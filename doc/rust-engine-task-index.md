# Rust Engine Task Index

Source: `../Infernux/doc/rust-engine-new-project-requirements.md`

This directory splits the Rust native game engine requirements into executable task files. The split follows the implementation phases, with cross-cutting concerns extracted into dedicated build, test, agent, and release tasks.

## Task Files

1. [task-00-project-decisions.md](task-00-project-decisions.md)
2. [task-01-workspace-core.md](task-01-workspace-core.md)
3. [task-02-scene-ecs-project-model.md](task-02-scene-ecs-project-model.md)
4. [task-03-assets-resources.md](task-03-assets-resources.md)
5. [task-04-rendering-backend.md](task-04-rendering-backend.md)
6. [task-05-physics-audio-editor.md](task-05-physics-audio-editor.md)
7. [task-06-agent-tools.md](task-06-agent-tools.md)
8. [task-07-build-packaging-ci.md](task-07-build-packaging-ci.md)
9. [task-08-testing-performance-acceptance.md](task-08-testing-performance-acceptance.md)
10. [task-09-release-docs.md](task-09-release-docs.md)

## Priority Map

| Priority | Tasks |
|:---|:---|
| P0 | task-00, task-01, task-07, initial design in task-06 |
| P1 | task-02, task-03, task-04, read-only agent tools in task-06 |
| P2 | task-05, write-capable agent tools in task-06, packaging stabilization in task-07 |
| P3 | additional render backends, mobile/Web support, deeper optimization |

## P0 Outputs

- [Project decisions and scope](project-decisions-and-scope.md)

## P1 Outputs

- Task 02 scene/ECS/project model implementation lives in `engine-ecs`.
- Example project formats live under `examples/project/`.
