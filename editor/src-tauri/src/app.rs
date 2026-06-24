use std::path::PathBuf;

use engine_editor::FileEditorStore;
use tauri::{Manager, WebviewWindowBuilder, image::Image, utils::config::Color};

use crate::state::EditorHostState;
use crate::{
    CopilotRequestState, DesktopIntegration, EditorHost, QuestAiRequestState,
    QuestExecutionRequestState, editor_compositor_requested, main_window_editor_compositor_support,
};

const APP_WINDOW_ICON: Image<'static> = tauri::include_image!("./icons/128x128.png");

fn dirs_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".config"))
            })
            .map(|p| p.join("varg"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Library/Application Support/varg"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .ok()
            .map(PathBuf::from)
            .map(|h| h.join("varg"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Some(PathBuf::from(".varg-config"))
    }
}

fn dirs_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_DATA_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".local/share"))
            })
            .map(|p| p.join("varg"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Library/varg"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA")
            .ok()
            .or_else(|| std::env::var("APPDATA").ok())
            .map(PathBuf::from)
            .map(|h| h.join("varg"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Some(PathBuf::from(".varg-data"))
    }
}

fn apply_desktop_window_adaptations(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let desktop = DesktopIntegration::detect();
    if let Some(window) = app.get_window("main") {
        window.set_icon(APP_WINDOW_ICON.clone())?;
        window.set_background_color(Some(Color(24, 24, 24, 255)))?;
        window.set_decorations(desktop.prefers_native_chrome())?;
    }
    Ok(())
}

fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    let Some(window_config) = app.config().app.windows.first() else {
        return Ok(());
    };

    let background = Color(24, 24, 24, 255);
    let window = WebviewWindowBuilder::from_config(app, window_config)?
        .transparent(false)
        .background_color(background)
        .on_page_load(|_webview, payload| {
            tracing::info!(
                target: "editor",
                "webview page load {:?}: {}",
                payload.event(),
                payload.url()
            );
        })
        .icon(APP_WINDOW_ICON.clone())?
        .build()?;
    window.set_background_color(Some(background))?;

    tracing::info!(target: "editor", "main editor WebView window created");
    window.set_focus()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_pre_gtk_desktop_environment() {
    // Linux native-host-window presentation depends on X11 handles. On Wayland
    // sessions this asks GTK/WebKit/Winit to run through Xwayland.
    unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    unsafe { std::env::set_var("WINIT_UNIX_BACKEND", "x11") };
}

#[cfg(not(target_os = "linux"))]
fn apply_pre_gtk_desktop_environment() {}

pub fn run() {
    let log_dir = dirs_data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("varg-editor")
        .join("logs");
    let file_appender = tracing_appender::rolling::daily(&log_dir, "varg.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    std::mem::forget(_guard);

    use tracing_subscriber::{EnvFilter, Registry, fmt, layer::SubscriberExt};
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = fmt::layer()
        .with_target(true)
        .compact()
        .with_writer(std::io::stderr);
    let file_layer = fmt::layer()
        .with_target(true)
        .compact()
        .with_ansi(false)
        .with_writer(non_blocking);
    let subscriber = Registry::default()
        .with(env_filter)
        .with(fmt_layer)
        .with(file_layer);
    tracing::subscriber::set_global_default(subscriber).expect("failed to set tracing subscriber");

    tracing::info!(target: "editor", "logging initialized -> {:?}", log_dir);

    apply_pre_gtk_desktop_environment();

    let config_dir = dirs_config_dir().unwrap_or_else(|| PathBuf::from("."));
    let store_path = config_dir.join("varg-editor-state.toml");
    let store = FileEditorStore::new(&store_path);

    let quest_root = dirs_data_dir()
        .unwrap_or_else(|| config_dir.clone())
        .join("quests");
    let host = match EditorHost::new_with_quest_root(store, quest_root) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("FATAL: failed to initialize editor host: {e}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(EditorHostState::new(host))
        .manage(CopilotRequestState::default())
        .manage(QuestAiRequestState::default())
        .manage(QuestExecutionRequestState::default())
        .invoke_handler(tauri::generate_handler![
            crate::commands::rpc::rpc,
            crate::commands::rpc::create_openai_realtime_transcription_session,
            crate::commands::copilot::start_copilot_plan,
            crate::commands::copilot::finish_copilot_plan,
            crate::commands::copilot::cancel_copilot_plan,
            crate::commands::quest_ai::start_quest_ai_request,
            crate::commands::quest_ai::finish_quest_ai_request,
            crate::commands::quest_ai::cancel_quest_ai_request,
            crate::commands::quest_execution::start_quest_execution,
            crate::commands::quest_execution::finish_quest_execution,
            crate::commands::quest_execution::cancel_quest_execution,
            crate::commands::windows::open_game_view,
            crate::commands::windows::set_game_render_scaling,
            crate::commands::windows::open_native_scene_view,
            crate::commands::windows::close_native_scene_view,
            crate::commands::viewport::viewport_presentation_capabilities,
            crate::commands::viewport::viewport_presentation_status,
            crate::commands::viewport::viewport_presentation_status_for_main_window,
            crate::commands::viewport::sync_native_host_editor_layout,
            crate::commands::viewport::native_panel_host_status,
            crate::commands::viewport::ensure_native_panel_host,
            crate::commands::viewport::sync_native_panel_layout,
            crate::commands::viewport::sync_editor_compositor_viewport,
            crate::commands::viewport::sync_wayland_embedded_compositor_viewport,
            crate::commands::viewport::wayland_embedded_compositor_status,
            crate::commands::viewport::open_no_cpu_readback_scene_view,
            crate::commands::viewport::sync_no_cpu_readback_scene_view,
            crate::commands::viewport::open_zero_copy_scene_view,
            crate::commands::viewport::sync_zero_copy_scene_view,
            crate::commands::viewport::open_wayland_embedded_compositor_scene_view,
            crate::commands::viewport::open_editor_compositor_scene_view,
            crate::commands::dialogs::select_project_location,
            crate::commands::viewport::viewport_readback_raw,
            crate::commands::dialogs::open_scene_dialog,
            crate::commands::dialogs::import_asset_dialog,
            crate::commands::dialogs::save_scene_as_dialog
        ])
        .setup(|app| {
            create_main_window(app)?;
            apply_desktop_window_adaptations(app)?;
            if editor_compositor_requested() {
                let support = main_window_editor_compositor_support(app.handle());
                if !support.available {
                    tracing::info!(
                        target: "editor",
                        backend = support.backend.id(),
                        reason = support.reason,
                        "skipping native host root; canvas readback remains active"
                    );
                } else {
                    tracing::info!(
                        target: "editor",
                        backend = support.backend.id(),
                        "native host window root available; deferred until Scene View layout is ready"
                    );
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
