# Task 10: Editor UI And Project Hub

## Goal

Define and ship the first Aster user-facing UI surface. The UI must make project creation, project launch, editor navigation, installed engine/toolchain management, and basic editor panels usable without weakening the feature-trimmed runtime profiles.

This task is based on a deep reference pass over `../Infernux/packaging` and `../Infernux/docs`. Infernux is a workflow and product reference only. Aster must not copy its Python/PySide implementation, QSS files, database schema, process launch code, branding, or release flow.

## Reference Findings From Infernux

Infernux has two relevant UI surfaces:

| Surface | Useful Lesson For Aster | Boundary |
|:---|:---|:---|
| `packaging/launcher.py` | A small Hub can own project list, installs, navigation, runtime bootstrap, and page switching. | Do not inherit PySide/Python architecture. |
| `packaging/view/sidebar_view.py` | Persistent left navigation and theme state make the Hub feel like a tool, not a modal launcher. | Recreate in Rust UI, not QSS. |
| `packaging/ui_project_list.py` | Searchable project cards with path, date, selection, folder open, and double-click workflow are the right baseline. | Use Aster project manifests and native platform services. |
| `packaging/view/new_project_view.py` | New-project flow must validate name, location, installed version/runtime, and remember last location. | Replace Python runtime/version assumptions with Aster profiles/toolchains. |
| `packaging/view/installs_view.py` | Installed engine versions need a first-class page with background fetch/download/install progress. | Use Cargo/native packages, not wheels. |
| `packaging/style.py` | A restrained Notion/Unity-Hub-like dark/light theme is effective for dense tools. | Define Aster design tokens, not copied QSS. |
| `docs/*.html` | Static docs are useful for onboarding, but they do not replace an actual tool UI. | Keep docs separate from Hub/editor. |

## Product Shape

Aster should have two related but separate UI products:

1. **Aster Hub**
   - Lightweight native app for project management and installed versions.
   - Can launch the editor or runtime.
   - Does not require render backend/editor scene services to be loaded at startup.

2. **Aster Editor**
   - Native editor shell with dockable panels and scene/game views.
   - Depends on the `editor` profile and editor services.
   - Must remain removable from `runtime-min` and normal game runtime builds.

## Hub Requirements

- Persistent sidebar navigation.
- Projects page.
- Installs page.
- Settings page once preferences exceed theme and paths.
- Dark/light theme.
- Searchable project list.
- Project card fields:
  - Name.
  - Project path.
  - Last opened or created date.
  - Engine/toolchain version.
  - Open folder action.
  - Launch editor action.
- New project dialog:
  - Project name validation.
  - Location chooser.
  - Template selection.
  - Engine/toolchain version selection.
  - Clear error state when a required field is missing.
  - Remember last location in editor preferences.
- Project deletion flow:
  - Confirm before deleting.
  - Refuse deletion while project is open.
  - Distinguish removing from recent list from deleting files.
- Installs page:
  - List installed editor/runtime versions.
  - Install or locate local build artifacts.
  - Show download/build/install progress.
  - Remove installed versions safely.
  - Surface diagnostic output on failure.

## Editor Shell Requirements

- Main menu or command palette.
- Toolbar with play, pause, stop, reload, save, build, and layout controls.
- Dockable panels:
  - Hierarchy.
  - Inspector.
  - Project.
  - Console.
  - Scene View.
  - Game View.
  - Assets/Resources browser.
  - Performance/Frame diagnostics.
- Panel registration API so editor modules and agent tools can expose panels without hard-coded UI ownership.
- Persisted layout and theme preferences.
- Scene View texture display from the render backend.
- Game View texture display from the active runtime camera.
- Selection service shared by Hierarchy, Scene View, Inspector, and Project panels.
- Console service with filtering, levels, source, timestamp, copy, clear, and open location when available.
- Asset preview service for material, mesh, texture, and prefab previews.
- Non-blocking background tasks for import, build, package, and version operations.

## Interaction Requirements

- First screen must be the actual project Hub, not a marketing page.
- Dense tool layout is preferred over oversized hero-style screens.
- Use compact, predictable controls:
  - Icon buttons for repeated tool actions.
  - Segmented controls for modes.
  - Toggles for binary state.
  - Search field for project/assets filtering.
  - Menus for secondary actions.
- Text must fit in narrow windows and high-DPI scaling.
- Long paths must truncate visually but remain available through tooltips/copy actions.
- Error dialogs must include actionable diagnostics, not generic failure text.
- Background tasks must not freeze the UI thread.

## Architecture Requirements

- Add UI/editor crates only under the `editor` or future `hub` profile.
- Do not add egui, imgui, winit UI integration, webview, or native packaging dependencies to `runtime-min`.
- Keep the Hub project store behind a small service interface so CLI and UI can share recent project metadata.
- Editor panels must talk to editor services, not directly to ECS internals.
- Platform-specific open-folder, file dialog, and process launch behavior must live behind `engine-platform` or editor platform adapters.
- Background work must return structured progress and diagnostics.
- Agent tools may read UI/editor state through service APIs but must not automate raw widget trees as the primary integration.

## Candidate Implementation Path

1. Create `engine-editor` for editor services, panel registry, selection, commands, console, and editor preferences.
2. Create `engine-editor-ui` for the first Rust-native UI shell.
3. Evaluate egui first because it matches the accepted project decision; keep Dear ImGui/imgui-rs as fallback for dock-heavy editor ergonomics.
4. Create `engine-hub` or an `engine-editor-ui` Hub mode after the project store and preferences service exist.
5. Wire `xtask build-editor` and `xtask package --profile editor` to build the UI.
6. Add screenshot/smoke tests for Hub startup, project creation validation, theme switch, and editor panel registration.

## Design Tokens

Use a restrained tool palette derived from the Infernux lesson, not from its QSS:

| Token | Dark | Light | Purpose |
|:---|:---|:---|:---|
| Base | `#181818` | `#ffffff` | Window background |
| Surface | `#202020` | `#f7f7f5` | Inputs, rows, cards |
| Surface Hover | `#2a2a2a` | `#efefed` | Hover state |
| Border | `#303030` | `#e6e6e3` | Separators |
| Text Primary | `#d4d4d4` | `#37352f` | Main text |
| Text Secondary | `#8a8a8a` | `#787774` | Metadata |
| Accent | `#f2f2f2` | `#37352f` | Primary actions |
| Danger | `#eb5757` | `#eb5757` | Destructive actions |

These tokens are starting defaults. They can be replaced by a richer design system once a real UI crate exists.

## Deliverables

- `engine-editor` service crate.
- `engine-editor-ui` or equivalent UI crate.
- Native Hub with Projects and Installs pages.
- Editor shell with panel registration and core panels.
- Project creation and launch flow.
- Theme and layout persistence.
- Feature/profile gating so UI dependencies do not enter `runtime-min`.
- Packaging command for native editor app.

## Acceptance

- `cargo test --workspace` passes.
- `cargo build -p runtime-min --no-default-features --features runtime-min` contains no UI/editor dependencies.
- Editor profile build includes the UI crate.
- Hub starts to the Projects page.
- A project can be created, listed, selected, opened in the OS file browser, and launched into the editor.
- Missing toolchain/version errors are visible and actionable.
- Theme switch does not require restart.
- Editor opens with Hierarchy, Inspector, Project, Console, Scene View, and Game View panels registered.
- Panel registration, command routing, selection, and console services have focused tests.
