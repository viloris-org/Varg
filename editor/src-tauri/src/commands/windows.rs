use tauri::State;

use crate::state::EditorHostState;
use crate::{game_window, native_host_window, scene_window};

#[tauri::command]
pub(crate) fn open_game_view(
    _app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
) -> Result<(), String> {
    state.with_host(|host| {
        host.poll_game_window();
        let snapshot = host
            .create_game_runtime_snapshot()
            .map_err(|e| e.to_string())?;

        if let Some(game_window) = host.game_window.as_ref() {
            game_window.restart(snapshot)?;
            return game_window.show();
        }

        let handle = game_window::spawn_game_window("Game View".to_string(), 1280, 720, snapshot);
        host.game_window = Some(handle);
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn set_game_render_scaling(
    settings: engine_render::RenderScalingSettings,
    state: State<'_, EditorHostState>,
) -> Result<(), String> {
    state.with_host(|host| {
        let game_window = host
            .game_window
            .as_ref()
            .ok_or_else(|| "game window is not running".to_owned())?;
        game_window.set_render_scaling(settings)
    })
}

#[tauri::command]
pub(crate) fn open_native_scene_view(
    state: State<'_, EditorHostState>,
    yaw: f32,
    pitch: f32,
    distance: f32,
    target_x: f32,
    target_y: f32,
    target_z: f32,
) -> Result<(), String> {
    tracing::info!(target: "editor", "opening floating Scene View via winit window");
    state.with_host(|host| {
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
            scene_window.kind() != scene_window::SceneWindowKind::Floating
        }) {
            host.scene_window = None;
        }

        if let Some(scene_window) = host.scene_window.as_ref() {
            scene_window.restart(snapshot, camera)?;
            return scene_window.show();
        }

        let handle =
            scene_window::spawn_scene_window("Scene View".to_owned(), 1280, 720, snapshot, camera);
        host.scene_window = Some(handle);
        Ok(())
    })
}

#[tauri::command]
pub(crate) fn close_native_scene_view(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
) -> Result<(), String> {
    native_host_window::hide_main_window_scene_surface(app)?;
    state.with_host(|host| {
        host.native_host_layout.host_root_active = false;
        host.poll_scene_window();
        if let Some(scene_window) = host.scene_window.as_ref() {
            scene_window.hide()?;
        }
        Ok(())
    })
}
