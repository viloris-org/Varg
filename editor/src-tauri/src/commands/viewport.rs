use serde::Deserialize;
use tauri::{State, utils::config::Color};

use crate::state::EditorHostState;
use crate::{
    EditorHost, editor_compositor, editor_compositor_requested,
    main_window_editor_compositor_support, native_host_window, native_panel_host, scene_window,
    wayland_embedded_compositor,
};

#[derive(Debug, Deserialize)]
pub(crate) struct EmbeddedSceneViewport {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl EmbeddedSceneViewport {
    fn into_rect(self) -> scene_window::SceneViewportRect {
        scene_window::SceneViewportRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
        .sanitized()
    }
}

fn update_native_host_layout_state(
    host: &mut EditorHost,
    scene_rect: native_host_window::NativeHostSceneRect,
    panels: Option<native_host_window::NativeHostPanelState>,
) {
    host.native_host_layout.scene_rect = Some(scene_rect);
    if let Some(panels) = panels {
        host.native_host_layout.panels = panels;
    }
    host.native_host_layout.host_root_active = true;
}

fn native_host_panel_state_from_options(
    current: native_host_window::NativeHostPanelState,
    hierarchy_open: Option<bool>,
    inspector_open: Option<bool>,
    ai_panel_open: Option<bool>,
) -> native_host_window::NativeHostPanelState {
    native_host_window::NativeHostPanelState {
        hierarchy_open: hierarchy_open.unwrap_or(current.hierarchy_open),
        inspector_open: inspector_open.unwrap_or(current.inspector_open),
        ai_panel_open: ai_panel_open.unwrap_or(current.ai_panel_open),
    }
}

fn scene_editor_view_mode(view_mode: Option<&str>) -> scene_window::SceneEditorViewMode {
    match view_mode {
        Some("2d") => scene_window::SceneEditorViewMode::TwoD,
        _ => scene_window::SceneEditorViewMode::ThreeD,
    }
}

#[tauri::command]
pub(crate) fn viewport_readback_raw(
    state: State<'_, EditorHostState>,
    width: u32,
    height: u32,
    yaw: f64,
    pitch: f64,
    distance: f64,
    target_x: f64,
    target_y: f64,
    target_z: f64,
    last_version: Option<u64>,
    play_mode: bool,
    editor_camera: bool,
    view_mode: String,
    entity_id: Option<String>,
) -> Result<Vec<u8>, String> {
    state.with_host(|host| {
        host.poll_game_window();
        let params = serde_json::json!({
            "width": width,
            "height": height,
            "yaw": yaw,
            "pitch": pitch,
            "distance": distance,
            "target_x": target_x,
            "target_y": target_y,
            "target_z": target_z,
            "last_version": last_version,
            "play_mode": play_mode,
            "editor_camera": editor_camera,
            "view_mode": view_mode,
            "entity_id": entity_id,
        });
        host.viewport_readback_raw(&params)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub(crate) fn viewport_presentation_capabilities()
-> editor_compositor::ViewportPresentationCapabilities {
    editor_compositor::presentation_capabilities(editor_compositor_requested())
}

#[tauri::command]
pub(crate) fn viewport_presentation_status() -> editor_compositor::ViewportPresentationStatus {
    editor_compositor::presentation_status(editor_compositor_requested())
}

#[tauri::command]
pub(crate) fn viewport_presentation_status_for_main_window(
    app: tauri::AppHandle,
) -> editor_compositor::ViewportPresentationStatus {
    editor_compositor::presentation_status_for(
        editor_compositor_requested(),
        main_window_editor_compositor_support(&app),
        wayland_embedded_compositor::support(),
    )
}

#[tauri::command]
pub(crate) fn sync_editor_compositor_viewport(
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
) -> Result<(), String> {
    state.with_host(|host| {
        let viewport =
            editor_compositor::EditorCompositorViewport::from_scene_rect(viewport.into_rect());
        host.editor_compositor.set_viewport(viewport);
        let _surface_viewport = host.editor_compositor.surface_viewport();
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn sync_wayland_embedded_compositor_viewport(
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
) -> Result<(), String> {
    let viewport =
        wayland_embedded_compositor::WaylandEmbeddedViewport::from_scene_rect(viewport.into_rect());
    state.with_host(|host| {
        host.wayland_embedded_compositor.set_viewport(viewport);
        if let Some(scene_window) = host.scene_window.as_ref() {
            scene_window.set_viewport(viewport.into_scene_rect())?;
        }
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn wayland_embedded_compositor_status(
    state: State<'_, EditorHostState>,
) -> Result<wayland_embedded_compositor::WaylandEmbeddedCompositorRuntimeStatus, String> {
    state.with_host(|host| Ok(host.wayland_embedded_compositor.status()))
}

#[derive(Debug, Deserialize)]
pub(crate) struct NativeHostEditorLayout {
    viewport: EmbeddedSceneViewport,
    hierarchy_open: bool,
    inspector_open: bool,
    ai_panel_open: bool,
}

#[tauri::command]
pub(crate) fn sync_native_host_editor_layout(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    layout: NativeHostEditorLayout,
) -> Result<native_host_window::NativeHostLayoutState, String> {
    native_host_window::install_host_root_on_main_thread(&app)?;
    let viewport = layout.viewport.into_rect();
    let scene_rect = native_host_window::NativeHostSceneRect::from(viewport);
    native_host_window::resize_main_window_scene_surface(app, scene_rect)?;
    state.with_host(|host| {
        host.native_host_layout = native_host_window::NativeHostLayoutState {
            scene_rect: Some(scene_rect),
            panels: native_host_window::NativeHostPanelState {
                hierarchy_open: layout.hierarchy_open,
                inspector_open: layout.inspector_open,
                ai_panel_open: layout.ai_panel_open,
            },
            host_root_active: true,
        };
        Ok(host.native_host_layout)
    })
}

#[tauri::command]
pub(crate) fn native_panel_host_status(
    state: State<'_, EditorHostState>,
) -> native_panel_host::NativePanelHostStatus {
    state.with_host(|host| host.native_panel_host.status())
}

#[tauri::command]
pub(crate) fn ensure_native_panel_host(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
) -> Result<native_panel_host::NativePanelHostStatus, String> {
    if !editor_compositor_requested() {
        return Ok(state.with_host(|host| host.native_panel_host.status()));
    }
    let support = main_window_editor_compositor_support(&app);
    if !support.available {
        return Ok(state.with_host(|host| host.native_panel_host.status()));
    }
    let window_config = app
        .config()
        .app
        .windows
        .first()
        .ok_or_else(|| "main editor window config is not available".to_owned())?;
    state.with_host(|host| {
        host.native_panel_host
            .ensure_installed(&app, window_config, false, Color(24, 24, 24, 255))
            .map_err(|error| error.to_string())?;
        Ok(host.native_panel_host.status())
    })
}

#[tauri::command]
pub(crate) fn sync_native_panel_layout(
    state: State<'_, EditorHostState>,
    layout: native_panel_host::NativePanelLayout,
) -> Result<native_panel_host::NativePanelHostStatus, String> {
    state.with_host(|host| {
        host.native_panel_host
            .apply_layout(layout)
            .map_err(|error| error.to_string())?;
        Ok(host.native_panel_host.status())
    })
}

#[tauri::command]
pub(crate) fn sync_no_cpu_readback_scene_view(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
    play_mode: Option<bool>,
    view_mode: Option<String>,
    yaw: Option<f32>,
    pitch: Option<f32>,
    distance: Option<f32>,
    target_x: Option<f32>,
    target_y: Option<f32>,
    target_z: Option<f32>,
    hierarchy_open: Option<bool>,
    inspector_open: Option<bool>,
    ai_panel_open: Option<bool>,
) -> Result<(), String> {
    let viewport = viewport.into_rect();
    let scene_rect = native_host_window::NativeHostSceneRect::from(viewport);
    native_host_window::install_host_root_on_main_thread(&app)?;
    native_host_window::resize_main_window_scene_surface(app, scene_rect)?;
    state.with_host(|host| {
        let panels = native_host_panel_state_from_options(
            host.native_host_layout.panels,
            hierarchy_open,
            inspector_open,
            ai_panel_open,
        );
        update_native_host_layout_state(host, scene_rect, Some(panels));
        let compositor_viewport =
            editor_compositor::EditorCompositorViewport::from_scene_rect(viewport);
        host.editor_compositor.set_viewport(compositor_viewport);
        host.poll_scene_window();
        let scene_window = host
            .scene_window
            .as_ref()
            .ok_or_else(|| "no-CPU-readback scene view is not running".to_owned())?;
        scene_window.set_viewport(viewport)?;
        scene_window.set_render_mode(if play_mode.unwrap_or(false) {
            scene_window::SceneRenderMode::Game
        } else {
            scene_window::SceneRenderMode::Editor
        })?;
        scene_window.set_editor_view_mode(scene_editor_view_mode(view_mode.as_deref()))?;
        if let (
            Some(yaw),
            Some(pitch),
            Some(distance),
            Some(target_x),
            Some(target_y),
            Some(target_z),
        ) = (yaw, pitch, distance, target_x, target_y, target_z)
        {
            scene_window.set_camera(scene_window::SceneCameraState {
                yaw,
                pitch,
                distance,
                target: engine_core::math::Vec3::new(target_x, target_y, target_z),
            })?;
        }
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn sync_zero_copy_scene_view(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
    yaw: Option<f32>,
    pitch: Option<f32>,
    distance: Option<f32>,
    target_x: Option<f32>,
    target_y: Option<f32>,
    target_z: Option<f32>,
) -> Result<(), String> {
    sync_no_cpu_readback_scene_view(
        app, state, viewport, None, None, yaw, pitch, distance, target_x, target_y, target_z, None,
        None, None,
    )
}

#[tauri::command]
pub(crate) fn open_no_cpu_readback_scene_view(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
    play_mode: Option<bool>,
    view_mode: Option<String>,
    yaw: f32,
    pitch: f32,
    distance: f32,
    target_x: f32,
    target_y: f32,
    target_z: f32,
) -> Result<(), String> {
    let support = main_window_editor_compositor_support(&app);
    if !editor_compositor_requested() || !support.available {
        return Err(format!(
            "no-CPU-readback scene view is unavailable on backend {}: {}",
            support.backend.id(),
            support.reason
        ));
    }
    let viewport = viewport.into_rect();
    let scene_rect = native_host_window::NativeHostSceneRect::from(viewport);
    native_host_window::install_host_root_on_main_thread(&app)?;
    native_host_window::resize_main_window_scene_surface(app.clone(), scene_rect)?;
    let target = native_host_window::main_window_scene_target(&app)?;
    tracing::info!(
        target: "editor",
        layout_mode = ?target.layout_mode,
        "opening no-CPU-readback Scene View through native host window adapter"
    );
    state.with_host(|host| {
        update_native_host_layout_state(host, scene_rect, None);
        let compositor_viewport =
            editor_compositor::EditorCompositorViewport::from_scene_rect(viewport);
        host.editor_compositor.set_viewport(compositor_viewport);
        host.poll_scene_window();
        let snapshot = host
            .create_scene_runtime_snapshot()
            .map_err(|error| error.to_string())?;
        let camera = scene_window::SceneCameraState {
            yaw,
            pitch,
            distance,
            target: engine_core::math::Vec3::new(target_x, target_y, target_z),
        };

        if host.scene_window.as_ref().is_some_and(|scene_window| {
            scene_window.kind() != scene_window::SceneWindowKind::Embedded
        }) {
            host.scene_window = None;
        }

        if let Some(scene_window) = host.scene_window.as_ref() {
            scene_window.set_viewport(viewport)?;
            scene_window.restart(snapshot, camera)?;
            scene_window.set_render_mode(if play_mode.unwrap_or(false) {
                scene_window::SceneRenderMode::Game
            } else {
                scene_window::SceneRenderMode::Editor
            })?;
            scene_window.set_editor_view_mode(scene_editor_view_mode(view_mode.as_deref()))?;
            return scene_window.show();
        }

        let mode = scene_window::SceneWindowMode::CompositorRaw {
            surface: target.surface,
            surface_width: viewport.width,
            surface_height: viewport.height,
            viewport,
        };
        let handle = scene_window::spawn_scene_window_with_mode(
            "Native Host Scene View".to_owned(),
            viewport.width,
            viewport.height,
            snapshot,
            camera,
            mode,
        );
        handle.set_render_mode(if play_mode.unwrap_or(false) {
            scene_window::SceneRenderMode::Game
        } else {
            scene_window::SceneRenderMode::Editor
        })?;
        handle.set_editor_view_mode(scene_editor_view_mode(view_mode.as_deref()))?;
        host.scene_window = Some(handle);
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn open_zero_copy_scene_view(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
    yaw: f32,
    pitch: f32,
    distance: f32,
    target_x: f32,
    target_y: f32,
    target_z: f32,
) -> Result<(), String> {
    open_no_cpu_readback_scene_view(
        app, state, viewport, None, None, yaw, pitch, distance, target_x, target_y, target_z,
    )
}

#[tauri::command]
pub(crate) fn open_editor_compositor_scene_view(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
    yaw: f32,
    pitch: f32,
    distance: f32,
    target_x: f32,
    target_y: f32,
    target_z: f32,
) -> Result<(), String> {
    open_no_cpu_readback_scene_view(
        app, state, viewport, None, None, yaw, pitch, distance, target_x, target_y, target_z,
    )
}

#[tauri::command]
pub(crate) fn open_wayland_embedded_compositor_scene_view(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
    _yaw: f32,
    _pitch: f32,
    _distance: f32,
    _target_x: f32,
    _target_y: f32,
    _target_z: f32,
) -> Result<wayland_embedded_compositor::WaylandEmbeddedCompositorRuntimeStatus, String> {
    if !editor_compositor_requested() {
        return Err("wayland-embedded-compositor is not enabled for this session".to_owned());
    }

    let viewport =
        wayland_embedded_compositor::WaylandEmbeddedViewport::from_scene_rect(viewport.into_rect());
    let scene_viewport = viewport.into_scene_rect();
    native_host_window::resize_main_window_scene_surface(
        app.clone(),
        native_host_window::NativeHostSceneRect::from(scene_viewport),
    )?;
    let host_target = native_host_window::main_window_scene_target(&app)?;
    let host_output_target = wayland_embedded_compositor::WaylandEmbeddedHostOutputTarget::new(
        host_target.surface,
        viewport,
    );
    state.with_host(|host| {
        host.wayland_embedded_compositor.set_viewport(viewport);
        host.wayland_embedded_compositor
            .set_host_output_target(host_output_target);
        let status = host.wayland_embedded_compositor.open_scene_view()?;
        let socket_name = status
            .socket_name
            .clone()
            .ok_or_else(|| "Wayland embedded compositor socket is unavailable".to_owned())?;
        host.poll_scene_window();
        let snapshot = host
            .create_scene_runtime_snapshot()
            .map_err(|error| error.to_string())?;
        let camera = scene_window::SceneCameraState {
            yaw: _yaw,
            pitch: _pitch,
            distance: _distance,
            target: engine_core::math::Vec3::new(_target_x, _target_y, _target_z),
        };

        if host.scene_window.as_ref().is_some_and(|scene_window| {
            scene_window.kind() != scene_window::SceneWindowKind::Embedded
        }) {
            host.scene_window = None;
        }

        if let Some(scene_window) = host.scene_window.as_ref() {
            scene_window.set_viewport(scene_viewport)?;
            scene_window.restart(snapshot, camera)?;
            scene_window.show()?;
            return Ok(status);
        }

        let handle = scene_window::spawn_scene_window_with_mode(
            "Wayland Embedded Scene View".to_owned(),
            scene_viewport.width,
            scene_viewport.height,
            snapshot,
            camera,
            scene_window::SceneWindowMode::WaylandEmbedded {
                socket_name,
                viewport: scene_viewport,
            },
        );
        host.scene_window = Some(handle);
        Ok(status)
    })
}
