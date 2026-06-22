# ADR 0001: Replace embedded child-surface Scene View with a native host window seam

Date: 2026-06-22

## Status

Accepted

## Context

Aster's editor needs a high-quality, low-latency Scene View. The current native embedding experiment creates a GTK/GDK child surface, presents WGPU directly into that surface, and moves it to match the React Scene View element measured through `getBoundingClientRect()`.

That experiment preserves zero-copy presentation, but it is not deterministic enough to be the default editor viewport. The WebView and the native child surface are owned by different compositor stacks. Resize, DPI conversion, Wayland/X11 behavior, GTK allocation, WebView layout, and React updates can race. When that race loses, the Scene View surface can drift over the inspector or bottom panels.

Using canvas readback avoids the compositor race because the image is composed entirely inside the WebView, but it is not the desired final solution. Readback copies GPU output through CPU memory and IPC, so it cannot satisfy the zero-copy performance goal.

Project language:

- The Scene View consumes a **Render World**.
- The renderer executes a **Frame Pipeline**.
- The editor viewport needs predictable **Render Scaling**.
- Presentation must become a real **seam**, not a set of scattered React calls that directly open and move platform child surfaces.

## Decision

Treat the GTK/GDK child-surface Scene View as an experimental adapter only.

Introduce an editor viewport presentation seam with these adapters:

- `canvas-readback`: stable fallback, composed by the WebView, not zero-copy.
- `embedded-native-experimental`: current GTK/GDK child-surface path, disabled by default and available only through an explicit diagnostic environment variable.
- `native-host-window`: canonical zero-copy path where the native host owns the root window and embeds Web UI.
- `editor-compositor`: legacy mode name for the target native-host-window zero-copy architecture.

The target architecture is **Native host window owns the editor root**:

1. A native host window owns the top-level editor window and the renderer lifecycle.
2. The engine Scene View is a native WGPU-rendered region owned by that host.
3. Web UI is embedded into the host as panels, overlays, dock views, and input layers.
4. React reports layout and editor intent, but it does not own the root presentation surface.
5. The host composes native Scene View regions and Web UI views without moving an operating-system child surface to follow a DOM element.

This keeps zero-copy presentation while removing both known races: child-surface positioning and WebView root-window ownership. Linux is the forcing function here because GTK/WebKit/Wayland/X11 make child-surface ownership especially visible, but the same host-window model is the cross-platform target.

## Consequences

Positive:

- The final Scene View path remains zero-copy.
- The native surface no longer has to track a DOM rectangle as an OS child window.
- Viewport placement becomes part of the renderer's Frame Pipeline/presentation state.
- The presentation seam gives tests and future adapters a stable interface.

Negative:

- The native host window requires deeper Tauri/WebView integration than the current child-surface experiment.
- Transparent WebView behavior must be validated per platform.
- The fallback path remains canvas readback until the compositor adapter is implemented.

## Implementation plan

1. Keep `canvas-readback` as the stable fallback when native host presentation is not requested or unavailable.
2. Make `native-host-window` the default zero-copy presentation when `ASTER_EDITOR_COMPOSITOR=1` requests the host-owned editor path.
3. Keep `embedded-native-experimental` behind `ASTER_ENABLE_EXPERIMENTAL_CHILD_SURFACE=1` for diagnostics only.
4. Add backend/frontend presentation-mode APIs around viewport ownership.
5. Implement the native host window on Linux first:
   - create the top-level native host window before/around WebView creation;
   - let the host own the WGPU Scene View region;
   - embed Web UI as dock/panel/overlay views inside the host;
   - keep React responsible for editor UI state and input intent, not root composition.
6. Extend the same host-window seam for Windows/macOS/iOS/Android native view composition.

## Rejected alternatives

### Keep tuning GTK child-surface movement

Rejected because the failure mode is architectural. More sync calls, rounding changes, or resize observers can lower the probability but cannot make two independent compositor layers deterministic.

### Use canvas readback as the final path

Rejected because it copies rendered pixels through CPU memory and IPC. It is useful as a stable fallback, not as the performance path.

### Use a separate floating native Scene View

Rejected as the primary UX because it is reliable and zero-copy but visually splits the editor.
