//! Tauri backend for the Aster Editor.
//!
//! Single `rpc` command that dispatches to EditorHost methods,
//! mirroring the original stdin/stdout JSON-RPC protocol.

use std::{
    cell::UnsafeCell,
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::Mutex,
    thread::ThreadId,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::Engine as _;
use engine_ai::{AgentPlan, AgentSession};
use engine_core::{EngineConfig, EngineError, EngineResult, RuntimeProfile};
use engine_editor::agent::PermissionPolicy;
use engine_editor::{
    ConsoleEntry, ConsoleLevel, ConsoleService, DurableEditorState, EditorPreferences,
    FileEditorStore, ProjectMetadata, ThemePreference, UndoCommand,
};
use engine_editor::{EditorShell, HubState, ProjectDeletionDecision, ProjectDeletionMode};
use engine_i18n::{Locale, Translations};
use engine_render::ImageFormat;
use engine_render_wgpu::{WgpuOffscreenConfig, WgpuRenderDevice};
use runtime_min::{headless_services_from_scene, RuntimeServices};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{image::Image, utils::config::Color, Emitter, Manager, State};

mod game_window;

const APP_WINDOW_ICON: Image<'static> = tauri::include_image!("./icons/128x128.png");

const WINDOW_BACKGROUND: &str = "#181818";

fn normalize_relative_path(path: &str) -> EngineResult<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(EngineError::config("path must stay inside the project"));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(EngineError::config("path must not be empty"));
    }

    Ok(normalized)
}

fn validate_file_name(name: &str) -> EngineResult<()> {
    let mut components = Path::new(name).components();
    if matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none() {
        Ok(())
    } else {
        Err(EngineError::config(
            "file name must not contain path separators",
        ))
    }
}

fn asset_meta_path_for_source(path: &Path) -> PathBuf {
    let mut meta_path = path.to_path_buf();
    if let Some(name) = path.file_name() {
        let mut meta_name = name.to_os_string();
        meta_name.push(".meta");
        meta_path.set_file_name(meta_name);
    } else {
        meta_path.set_extension("meta");
    }
    meta_path
}

fn resolve_existing_relative_path(root: &Path, path: &str) -> EngineResult<PathBuf> {
    let relative = normalize_relative_path(path)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|source| EngineError::Filesystem {
            path: root.to_path_buf(),
            source,
        })?;
    let full_path = canonical_root.join(relative);
    let canonical = full_path
        .canonicalize()
        .map_err(|source| EngineError::Filesystem {
            path: full_path.clone(),
            source,
        })?;

    if !canonical.starts_with(&canonical_root) {
        return Err(EngineError::config("path is outside the project"));
    }

    Ok(canonical)
}

fn resolve_writable_relative_path(root: &Path, path: &str) -> EngineResult<PathBuf> {
    let relative = normalize_relative_path(path)?;
    std::fs::create_dir_all(root).map_err(|source| EngineError::Filesystem {
        path: root.to_path_buf(),
        source,
    })?;
    let canonical_root = root
        .canonicalize()
        .map_err(|source| EngineError::Filesystem {
            path: root.to_path_buf(),
            source,
        })?;
    let full_path = canonical_root.join(relative);

    if full_path.exists() {
        let canonical = full_path
            .canonicalize()
            .map_err(|source| EngineError::Filesystem {
                path: full_path.clone(),
                source,
            })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(EngineError::config("path is outside the project"));
        }
        return Ok(canonical);
    }

    let mut probe = full_path.parent().unwrap_or(&canonical_root);
    while !probe.exists() {
        probe = probe.parent().unwrap_or(&canonical_root);
    }
    let canonical_probe = probe
        .canonicalize()
        .map_err(|source| EngineError::Filesystem {
            path: probe.to_path_buf(),
            source,
        })?;
    if !canonical_probe.starts_with(&canonical_root) {
        return Err(EngineError::config("path is outside the project"));
    }

    Ok(full_path)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DesktopEnvironment {
    Gnome,
    Kde,
    Xfce,
    Cinnamon,
    Mate,
    Unknown,
}

impl DesktopEnvironment {
    fn detect() -> Self {
        let candidates = [
            std::env::var("XDG_CURRENT_DESKTOP").ok(),
            std::env::var("XDG_SESSION_DESKTOP").ok(),
            std::env::var("DESKTOP_SESSION").ok(),
            std::env::var("KDE_FULL_SESSION")
                .ok()
                .filter(|v| v == "true"),
            std::env::var("GNOME_DESKTOP_SESSION_ID").ok(),
        ];
        let desktop = candidates
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(":")
            .to_ascii_lowercase();

        if desktop.contains("kde") || desktop.contains("plasma") {
            Self::Kde
        } else if desktop.contains("gnome") {
            Self::Gnome
        } else if desktop.contains("xfce") {
            Self::Xfce
        } else if desktop.contains("cinnamon") {
            Self::Cinnamon
        } else if desktop.contains("mate") {
            Self::Mate
        } else {
            Self::Unknown
        }
    }

    fn id(&self) -> &'static str {
        match self {
            Self::Gnome => "gnome",
            Self::Kde => "kde",
            Self::Xfce => "xfce",
            Self::Cinnamon => "cinnamon",
            Self::Mate => "mate",
            Self::Unknown => "unknown",
        }
    }

    fn prefers_native_chrome(&self) -> bool {
        true
    }

    #[cfg(test)]
    fn prefers_native_chrome_for_backend(&self, native_wayland_preferred: bool) -> bool {
        let _ = native_wayland_preferred;
        true
    }
}

#[derive(Clone, Debug)]
struct DesktopIntegration {
    desktop: DesktopEnvironment,
}

impl DesktopIntegration {
    fn detect() -> Self {
        Self {
            desktop: DesktopEnvironment::detect(),
        }
    }

    fn prefers_native_chrome(&self) -> bool {
        self.desktop.prefers_native_chrome()
    }

    fn as_json(&self) -> Value {
        serde_json::json!({
            "desktop_environment": self.desktop.id(),
            "prefers_native_chrome": self.prefers_native_chrome(),
            "window_background": WINDOW_BACKGROUND,
            "window_backend": std::env::var("GDK_BACKEND").unwrap_or_else(|_| "default".to_owned()),
        })
    }
}

// ─── Credentials file (separate from durable state for security) ────────────

/// Credentials stored in a separate TOML file (not committed to projects).
#[derive(Debug, Default, Deserialize, Serialize)]
struct CredentialsFile {
    /// API key for the copilot provider.
    #[serde(default)]
    copilot_api_key: Option<String>,
    /// ChatGPT OAuth credentials for the Codex provider.
    #[serde(default)]
    codex_oauth: Option<CodexOAuthCredential>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CodexOAuthCredential {
    access_token: String,
    refresh_token: String,
    expires_at_ms: u64,
    account_id: Option<String>,
}

#[derive(Debug)]
struct PendingCodexOAuth {
    device_auth_id: String,
    user_code: String,
    interval_seconds: u64,
}

struct PreparedCopilotRequest {
    request: engine_ai::AiRequest,
    original_prompt: String,
    provider: String,
    model: String,
    api_key: Option<String>,
    endpoint: Option<String>,
    max_tokens: u32,
    codex_oauth: Option<engine_ai::providers::CodexOAuthCredentials>,
    mimo_config: Option<engine_editor::MimoConfig>,
    glm_config: Option<engine_editor::GlmConfig>,
    cached_context: engine_editor::ProjectContext,
}

struct CompletedCopilotRequest {
    original_prompt: String,
    response: Result<String, String>,
    tool_calls: Vec<engine_ai::ToolCall>,
    cached_context: engine_editor::ProjectContext,
}

#[derive(Default)]
struct CopilotRequests {
    completed: HashMap<String, CompletedCopilotRequest>,
    cancelled: std::collections::HashSet<String>,
}

#[derive(Clone, Default)]
struct CopilotRequestState {
    requests: std::sync::Arc<Mutex<CopilotRequests>>,
}

#[derive(Debug, Deserialize)]
struct CodexTokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default = "default_oauth_expires_in")]
    expires_in: u64,
    #[serde(default)]
    id_token: Option<String>,
}

fn default_oauth_expires_in() -> u64 {
    3600
}

const CODEX_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_OAUTH_ISSUER: &str = "https://auth.openai.com";

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn exchange_codex_token(form: &[(&str, &str)]) -> EngineResult<CodexTokenResponse> {
    let mut response = ureq::post(format!("{CODEX_OAUTH_ISSUER}/oauth/token"))
        .send_form(form.iter().copied())
        .map_err(|error| EngineError::other(format!("Codex token exchange failed: {error}")))?;
    response
        .body_mut()
        .read_json()
        .map_err(|error| EngineError::other(format!("invalid Codex token response: {error}")))
}

fn codex_credential_from_tokens(tokens: CodexTokenResponse) -> CodexOAuthCredential {
    let account_id = tokens
        .id_token
        .as_deref()
        .and_then(extract_codex_account_id)
        .or_else(|| extract_codex_account_id(&tokens.access_token));
    CodexOAuthCredential {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at_ms: unix_time_ms().saturating_add(tokens.expires_in.saturating_mul(1000)),
        account_id,
    }
}

fn extract_codex_account_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    claims["chatgpt_account_id"]
        .as_str()
        .or_else(|| claims["https://api.openai.com/auth"]["chatgpt_account_id"].as_str())
        .or_else(|| claims["organizations"][0]["id"].as_str())
        .map(str::to_owned)
}

// ─── Editor host state ───────────────────────────────────────────────────────

pub struct EditorHost {
    /// Hub state (project picker screen).
    hub: HubState,
    /// Editor shell (active editor when a project is open).
    shell: EditorShell,
    /// Durable state loaded from disk.
    durable_state: DurableEditorState,
    /// File-based preference store.
    store: FileEditorStore,
    /// Console service (shared between hub and shell).
    console: ConsoleService,
    /// WGPU render device for offscreen viewport rendering (lazily created).
    render_device: Option<WgpuRenderDevice>,
    /// Desktop/window integration policy detected on the Rust side.
    desktop_integration: DesktopIntegration,
    /// Cached translations for the current locale.
    translations: Translations,
    /// Monotonic version counter incremented on every scene mutation.
    /// Used by the frontend to skip viewport re-renders when nothing changed.
    scene_version: u64,
    /// Runtime snapshot used by Game View play mode.
    play_runtime: Option<RuntimeServices>,
    /// Last wall-clock frame timestamp for play mode deltas.
    play_last_frame: Option<Instant>,
    /// Monotonic version counter for simulated play frames.
    play_version: u64,
    /// Cached copilot plan awaiting user approval.
    last_copilot_plan: Option<AgentPlan>,
    /// Copilot provider configuration.
    copilot_settings: engine_editor::CopilotSettings,
    /// Persisted ChatGPT OAuth credentials for Codex.
    codex_oauth: Option<CodexOAuthCredential>,
    /// Active device authorization request.
    pending_codex_oauth: Option<PendingCodexOAuth>,
    /// Active copilot conversation history for multi-turn dialogue.
    copilot_conversation: Vec<engine_ai::ChatMessage>,
    /// Native game window handle (direct GPU surface rendering).
    game_window: Option<game_window::GameWindowHandle>,
}

/// Maximum number of messages to keep in the copilot conversation.
/// Older messages are evicted in pairs (user+assistant) to maintain context coherence.
const MAX_COPILOT_CONVERSATION_MESSAGES: usize = 40;

impl EditorHost {
    pub fn new(store: FileEditorStore) -> EngineResult<Self> {
        let durable_state = store.load().unwrap_or_default();
        let hub = HubState::from_durable_state(durable_state.clone());
        let locale = hub.preferences().locale;

        // Load copilot settings from durable state, then overlay credentials
        let mut copilot_settings = durable_state.preferences.copilot.clone();
        let mut credentials = CredentialsFile::default();
        if let Some(credentials_path) = store.path().parent().map(|p| p.join("credentials.toml")) {
            if let Ok(cred_text) = std::fs::read_to_string(&credentials_path) {
                if let Ok(creds) = toml::from_str::<CredentialsFile>(&cred_text) {
                    credentials = creds;
                }
            }
        }
        copilot_settings.api_key = credentials.copilot_api_key.clone();

        let mut host = Self {
            hub,
            shell: EditorShell::with_core_services(EditorPreferences::default()),
            durable_state,
            store,
            console: ConsoleService::default(),
            render_device: None,
            desktop_integration: DesktopIntegration::detect(),
            translations: Translations::load(locale),
            scene_version: 1,
            play_runtime: None,
            play_last_frame: None,
            play_version: 1,
            last_copilot_plan: None,
            copilot_settings,
            codex_oauth: credentials.codex_oauth,
            pending_codex_oauth: None,
            copilot_conversation: Vec::new(),
            game_window: None,
        };

        host.reopen_last_project_if_needed();
        Ok(host)
    }

    /// Dispatch an RPC method call.
    pub fn handle(&mut self, method: &str, params: &Value) -> EngineResult<Value> {
        match method {
            // ── Hub ──
            "app/get_desktop_integration" => self.app_get_desktop_integration(params),
            "app/open_folder" => self.app_open_folder(params),
            "hub/get_state" => self.hub_get_state(params),
            "hub/get_translations" => self.hub_get_translations(params),
            "hub/list_projects" => self.hub_list_projects(params),
            "hub/open_project" => self.hub_open_project(params),
            "hub/create_project" => self.hub_create_project(params),
            "hub/delete_project" => self.hub_delete_project(params),
            "hub/set_theme" => self.hub_set_theme(params),
            "hub/set_page" => self.hub_set_page(params),
            "hub/set_locale" => self.hub_set_locale(params),

            // ── Project ──
            "project/list_assets" => self.project_list_assets(params),
            "project/import_file" => self.project_import_file(params),
            "project/create_script" => self.project_create_script(params),
            "project/rename_asset" => self.project_rename_asset(params),
            "project/delete_asset" => self.project_delete_asset(params),
            "project/reimport_asset" => self.project_reimport_asset(params),
            "project/read_file" => self.project_read_file(params),
            "project/write_file" => self.project_write_file(params),

            // ── Console ──
            "console/get_entries" => self.console_get_entries(params),
            "console/clear" => self.console_clear(params),
            "console/push_entry" => self.console_push_entry(params),

            // ── Viewport ──
            "viewport/readback" => self.viewport_readback(params),

            // ── Play mode ──
            "play/start" => self.play_start(params),
            "play/stop" => self.play_stop(params),
            "play/get_state" => self.play_get_state(params),

            // ── Copilot ──
            "copilot/plan" => self.copilot_plan(params),
            "copilot/apply" => self.copilot_apply(params),
            "copilot/allow_command" => self.copilot_allow_command(params),
            "copilot/clear_conversation" => self.copilot_clear_conversation(params),
            "copilot/get_conversation_length" => self.copilot_get_conversation_length(params),
            "app/get_copilot_settings" => self.get_copilot_settings(params),
            "app/update_copilot_settings" => self.update_copilot_settings(params),
            "app/detect_models" => self.detect_models(params),
            "app/get_model_registry" => self.get_model_registry(params),
            "app/codex_oauth_status" => self.codex_oauth_status(params),
            "app/codex_oauth_start" => self.codex_oauth_start(params),
            "app/codex_oauth_poll" => self.codex_oauth_poll(params),
            "app/codex_oauth_logout" => self.codex_oauth_logout(params),

            // ── Shell ──
            "shell/get_state" => self.shell_get_state(params),
            "shell/get_scene_tree" => self.shell_get_scene_tree(params),
            "shell/get_entity" => self.shell_get_entity(params),
            "shell/select_entity" => self.shell_select_entity(params),
            "shell/save_scene" => self.shell_save_scene(params),
            "shell/open_scene" => self.shell_open_scene(params),
            "shell/save_scene_as" => self.shell_save_scene_as(params),
            "shell/update_transform" => self.shell_update_transform(params),
            "shell/add_component" => self.shell_add_component(params),
            "shell/update_component" => self.shell_update_component(params),
            "shell/remove_component" => self.shell_remove_component(params),
            "shell/undo" => self.shell_undo(params),
            "shell/redo" => self.shell_redo(params),
            "shell/create_object" => self.shell_create_object(params),
            "shell/delete_object" => self.shell_delete_object(params),
            "shell/rename_object" => self.shell_rename_object(params),
            "shell/duplicate_object" => self.shell_duplicate_object(params),
            "shell/reparent_object" => self.shell_reparent_object(params),
            "shell/close_project" => self.shell_close_project(params),

            // ── Scene Guides ──
            "scene/get_guides" => self.scene_get_guides(params),

            _ => Err(EngineError::config(format!("unknown method: {method}"))),
        }
    }

    fn app_get_desktop_integration(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(self.desktop_integration.as_json())
    }

    fn app_open_folder(&mut self, params: &Value) -> EngineResult<Value> {
        use std::process::Command;

        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        #[cfg(target_os = "linux")]
        {
            Command::new("xdg-open")
                .arg(path)
                .spawn()
                .map_err(|e| EngineError::other(format!("failed to open folder: {e}")))?;
        }
        #[cfg(target_os = "macos")]
        {
            Command::new("open")
                .arg(path)
                .spawn()
                .map_err(|e| EngineError::other(format!("failed to open folder: {e}")))?;
        }
        #[cfg(target_os = "windows")]
        {
            Command::new("explorer")
                .arg(path)
                .spawn()
                .map_err(|e| EngineError::other(format!("failed to open folder: {e}")))?;
        }

        Ok(serde_json::json!({ "opened": true }))
    }

    // ── Hub handlers ──

    fn hub_get_state(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({
            "page": match self.hub.page() {
                engine_editor::ui_state::HubPage::Projects => "projects",
                engine_editor::ui_state::HubPage::Installs => "installs",
                engine_editor::ui_state::HubPage::Settings => "settings",
            },
            "theme": match self.hub.preferences().theme {
                ThemePreference::Dark => "dark",
                ThemePreference::Light => "light",
                ThemePreference::System => "system",
            },
            "recent_projects": self.hub.filtered_projects().iter().map(|p| serde_json::json!({
                "name": p.name,
                "path": p.path.to_string_lossy(),
                "last_touched": p.last_touched,
                "toolchain_version": p.toolchain_version,
            })).collect::<Vec<_>>(),
            "locale": match self.hub.preferences().locale {
                engine_i18n::Locale::Zh => "zh",
                engine_i18n::Locale::Ja => "ja",
                engine_i18n::Locale::Ko => "ko",
                engine_i18n::Locale::Es => "es",
                engine_i18n::Locale::ZhHant => "zh_hant",
                _ => "en",
            },
            "installs": self.hub.installs().iter().map(|i| serde_json::json!({
                "version": i.version,
                "path": i.path.to_string_lossy(),
                "editor_available": i.editor_available,
                "runtime_available": i.runtime_available,
            })).collect::<Vec<_>>(),
            "open_project": self.shell.project().map(|p| p.root.to_string_lossy()),
            "desktop_integration": self.desktop_integration.as_json(),
        }))
    }

    fn hub_list_projects(&mut self, _params: &Value) -> EngineResult<Value> {
        let projects: Vec<Value> = self
            .hub
            .filtered_projects()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "path": p.path.to_string_lossy(),
                    "last_touched": p.last_touched,
                    "toolchain_version": p.toolchain_version,
                })
            })
            .collect();
        Ok(serde_json::json!({ "projects": projects }))
    }

    fn hub_open_project(&mut self, params: &Value) -> EngineResult<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path' parameter"))?;
        let project_path = PathBuf::from(path);

        // Load the project into the editor shell
        self.shell.open_project(&project_path)?;

        // Mark as recent
        let name = self
            .shell
            .project()
            .map(|p| p.name().to_owned())
            .unwrap_or_else(|| {
                project_path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        let metadata = ProjectMetadata::new(&name, &project_path, timestamp_now(), "0.1.0");
        self.hub.upsert_project(metadata);

        // Persist state
        self.hub.mark_project_open(project_path.clone());
        self.sync_durable_state();

        // Forward console entries from shell open
        self.drain_shell_console();

        Ok(serde_json::json!({
            "name": name,
            "path": project_path.to_string_lossy(),
        }))
    }

    fn hub_create_project(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name' parameter"))?;
        let location = params
            .get("location")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'location' parameter"))?;

        let request = engine_editor::NewProjectRequest {
            name: name.to_owned(),
            location: Some(PathBuf::from(location)),
            template_id: params
                .get("template_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned()),
            toolchain_version: params
                .get("toolchain_version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned()),
        };

        let plan = self.hub.create_project_plan(&request)?;
        self.hub.create_project_files(&plan)?;

        let metadata = ProjectMetadata::new(
            &plan.name,
            &plan.path,
            timestamp_now(),
            &plan.toolchain_version,
        );
        self.hub.upsert_project(metadata);
        self.sync_durable_state();

        Ok(serde_json::json!({
            "name": plan.name,
            "path": plan.path.to_string_lossy(),
        }))
    }

    fn hub_delete_project(&mut self, params: &Value) -> EngineResult<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path' parameter"))?;
        let confirmed = params
            .get("confirmed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let project_path = PathBuf::from(path);
        let decision = self.hub.request_project_deletion(
            &project_path,
            ProjectDeletionMode::RemoveRecent,
            confirmed,
        );

        match decision {
            ProjectDeletionDecision::RemovedFromRecent { .. } => {
                self.sync_durable_state();
                Ok(serde_json::json!({ "status": "removed" }))
            }
            ProjectDeletionDecision::NeedsConfirmation { .. } => {
                Ok(serde_json::json!({ "status": "needs_confirmation" }))
            }
            ProjectDeletionDecision::RefusedOpenProject { .. } => {
                Err(EngineError::config("cannot delete an open project"))
            }
            ProjectDeletionDecision::DeleteFilesApproved { .. } => Err(EngineError::config(
                "file deletion not supported through IPC",
            )),
        }
    }

    fn hub_set_theme(&mut self, params: &Value) -> EngineResult<Value> {
        let theme = params
            .get("theme")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'theme' parameter"))?;
        let pref = match theme {
            "light" => ThemePreference::Light,
            "dark" => ThemePreference::Dark,
            _ => ThemePreference::System,
        };
        self.hub.set_theme(pref);
        self.sync_durable_state();
        Ok(serde_json::json!({ "theme": theme }))
    }

    fn hub_set_page(&mut self, params: &Value) -> EngineResult<Value> {
        let page = params
            .get("page")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'page' parameter"))?;
        use engine_editor::ui_state::HubPage;
        let p = match page {
            "installs" => HubPage::Installs,
            "settings" => HubPage::Settings,
            _ => HubPage::Projects,
        };
        self.hub.set_page(p);
        self.sync_durable_state();
        Ok(serde_json::json!({ "page": page }))
    }

    fn hub_get_translations(&mut self, _params: &Value) -> EngineResult<Value> {
        let entries: Vec<serde_json::Value> = self
            .translations
            .entries()
            .into_iter()
            .map(|(k, v)| serde_json::json!({ "key": k, "value": v }))
            .collect();
        Ok(serde_json::json!({
            "locale": match self.translations.locale() {
                Locale::En => "en",
                Locale::Zh => "zh",
                Locale::Ja => "ja",
                Locale::Ko => "ko",
                Locale::Es => "es",
                Locale::ZhHant => "zh_hant",
            },
            "entries": entries,
        }))
    }

    fn hub_set_locale(&mut self, params: &Value) -> EngineResult<Value> {
        let locale_str = params
            .get("locale")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'locale' parameter"))?;
        let locale = match locale_str {
            "zh" => Locale::Zh,
            "ja" => Locale::Ja,
            "ko" => Locale::Ko,
            "es" => Locale::Es,
            "zh_hant" => Locale::ZhHant,
            _ => Locale::En,
        };
        self.hub.set_locale(locale);
        // Reload translations for the new locale
        self.translations = Translations::load(locale);
        self.sync_durable_state();
        Ok(serde_json::json!({ "locale": locale_str }))
    }

    // ── Project handlers ──

    fn project_list_assets(&mut self, _params: &Value) -> EngineResult<Value> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let entries: Vec<Value> = project
            .database
            .iter_entries()
            .map(|entry| {
                serde_json::json!({
                    "guid": entry.guid.to_string(),
                    "path": entry.path.to_string_lossy(),
                    "kind": format!("{:?}", entry.kind),
                })
            })
            .collect();

        // Also get assets from ProjectContext.sorted_assets() for richer metadata
        let assets: Vec<Value> = project
            .sorted_assets()
            .iter()
            .map(|meta| {
                serde_json::json!({
                    "guid": meta.guid.to_string(),
                    "source_path": meta.source_path.to_string_lossy(),
                    "kind": format!("{:?}", meta.kind),
                    "importer": meta.importer,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "entries": entries,
            "assets": assets,
        }))
    }

    fn project_import_file(&mut self, params: &Value) -> EngineResult<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        project.import_file(std::path::PathBuf::from(path))?;
        self.console.push(engine_editor::ConsoleEntry {
            timestamp: "now".into(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: None,
                line: None,
            },
            message: format!("Imported file: {path}"),
        });

        Ok(serde_json::json!({"imported": path}))
    }

    fn project_create_script(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        let backend = params
            .get("backend")
            .and_then(|v| v.as_str())
            .unwrap_or("rhai");

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        // Use the asset root relative to project root
        let asset_root = project.root.join(&project.manifest.asset_root);
        std::fs::create_dir_all(&asset_root).map_err(|source| EngineError::Filesystem {
            path: asset_root.clone(),
            source,
        })?;

        let ext = if backend == "python" { "py" } else { "rhai" };
        let script_path = format!("scripts/{name}.{ext}");
        let full_path = asset_root.join(&script_path);

        let template = match backend {
            "python" => {
                r#"# Auto-generated script
# Use this file to implement custom game logic

def on_start(entity):
    pass

def on_update(entity, dt):
    pass
"#
            }
            _ => {
                r#"// Auto-generated script
// Use this file to implement custom game logic

fn on_start(entity) {
    // Called when the entity is first activated
}

fn on_update(entity, dt) {
    // Called every frame with delta time
}
"#
            }
        };

        // Check if parent directory exists
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        std::fs::write(&full_path, template).map_err(|source| EngineError::Filesystem {
            path: full_path.clone(),
            source,
        })?;

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: "now".into(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: Some(full_path.clone()),
                line: None,
            },
            message: format!("Created script: {}", full_path.display()),
        });

        Ok(serde_json::json!({
            "path": script_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    fn project_rename_asset(&mut self, params: &Value) -> EngineResult<Value> {
        let old_path_str = params
            .get("old_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'old_path'"))?;
        let new_name = params
            .get("new_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'new_name'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        validate_file_name(new_name)?;
        let asset_root = project.root.join(&project.manifest.asset_root);
        let old_path = resolve_existing_relative_path(&asset_root, old_path_str)?;
        let parent = old_path
            .parent()
            .ok_or_else(|| EngineError::config("cannot rename root directory"))?;
        let ext = old_path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let new_path = parent.join(format!("{}{}", new_name, ext));
        let canonical_asset_root =
            asset_root
                .canonicalize()
                .map_err(|source| EngineError::Filesystem {
                    path: asset_root.clone(),
                    source,
                })?;
        if !new_path.starts_with(&canonical_asset_root) {
            return Err(EngineError::config("path is outside the project"));
        }

        std::fs::rename(&old_path, &new_path).map_err(|source| EngineError::Filesystem {
            path: old_path.clone(),
            source,
        })?;

        // Also rename the .meta file if it exists
        let old_meta = asset_meta_path_for_source(&old_path);
        if old_meta.exists() {
            let new_meta = asset_meta_path_for_source(&new_path);
            std::fs::rename(&old_meta, &new_meta).ok();
        }

        // Rescan to update the database
        project.rescan_assets()?;

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: timestamp_now(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: Some(new_path.clone()),
                line: None,
            },
            message: format!("Renamed asset: {} → {}", old_path_str, new_path.display()),
        });

        Ok(serde_json::json!({ "new_path": new_path.to_string_lossy() }))
    }

    fn project_delete_asset(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let asset_root = project.root.join(&project.manifest.asset_root);
        let path = resolve_existing_relative_path(&asset_root, path_str)?;

        // Delete the file
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|source| EngineError::Filesystem {
                path: path.clone(),
                source,
            })?;
        } else {
            std::fs::remove_file(&path).map_err(|source| EngineError::Filesystem {
                path: path.clone(),
                source,
            })?;
            // Also delete the .meta file
            let meta_path = asset_meta_path_for_source(&path);
            if meta_path.exists() {
                std::fs::remove_file(&meta_path).ok();
            }
        }

        // Rescan to update the database
        project.rescan_assets()?;

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: timestamp_now(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: None,
                line: None,
            },
            message: format!("Deleted asset: {path_str}"),
        });

        Ok(serde_json::json!({ "deleted": true }))
    }

    fn project_reimport_asset(&mut self, params: &Value) -> EngineResult<Value> {
        let reimport_all = params.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
        if reimport_all {
            let Some(project) = self.shell.project_mut() else {
                return Err(EngineError::config("no project open"));
            };

            let asset_root = project.root.join(&project.manifest.asset_root);
            let mut stack = vec![asset_root.clone()];
            while let Some(path) = stack.pop() {
                let entries = match std::fs::read_dir(&path) {
                    Ok(entries) => entries,
                    Err(source) => {
                        return Err(EngineError::Filesystem { path, source });
                    }
                };
                for entry in entries {
                    let entry = entry.map_err(|source| EngineError::Filesystem {
                        path: asset_root.clone(),
                        source,
                    })?;
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        stack.push(entry_path);
                    } else if entry_path.extension().is_some_and(|ext| ext == "meta") {
                        std::fs::remove_file(&entry_path).ok();
                    }
                }
            }

            project.rescan_assets()?;
            self.console.push(engine_editor::ConsoleEntry {
                timestamp: timestamp_now(),
                level: engine_editor::ConsoleLevel::Info,
                source: engine_editor::ConsoleSource {
                    subsystem: "editor".into(),
                    file: None,
                    line: None,
                },
                message: "Reimported all assets".into(),
            });

            return Ok(serde_json::json!({ "reimported": true }));
        }

        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        // Delete existing meta file to force reimport
        let asset_root = project.root.join(&project.manifest.asset_root);
        let path = resolve_existing_relative_path(&asset_root, path_str)?;
        let meta_path = asset_meta_path_for_source(&path);
        if meta_path.exists() {
            std::fs::remove_file(&meta_path).ok();
        }

        project.rescan_assets()?;

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: timestamp_now(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".into(),
                file: None,
                line: None,
            },
            message: format!("Reimported asset: {path_str}"),
        });

        Ok(serde_json::json!({ "reimported": true }))
    }

    fn project_read_file(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let asset_root = project.root.join(&project.manifest.asset_root);
        let full_path = resolve_existing_relative_path(&asset_root, path_str)?;

        let content =
            std::fs::read_to_string(&full_path).map_err(|source| EngineError::Filesystem {
                path: full_path.clone(),
                source,
            })?;

        Ok(serde_json::json!({ "content": content }))
    }

    fn project_write_file(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'content'"))?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        let asset_root = project.root.join(&project.manifest.asset_root);
        let full_path = resolve_writable_relative_path(&asset_root, path_str)?;

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        std::fs::write(&full_path, content).map_err(|source| EngineError::Filesystem {
            path: full_path.clone(),
            source,
        })?;

        Ok(serde_json::json!({ "saved": true }))
    }

    // ── Console handlers ──

    fn console_get_entries(&mut self, _params: &Value) -> EngineResult<Value> {
        let entries: Vec<Value> = self
            .console
            .entries()
            .iter()
            .map(|e| {
                serde_json::json!({
                    "timestamp": e.timestamp,
                    "level": format!("{:?}", e.level).to_lowercase(),
                    "subsystem": e.source.subsystem,
                    "file": e.source.file.as_ref().map(|f| f.to_string_lossy()),
                    "line": e.source.line,
                    "message": e.message,
                })
            })
            .collect();
        Ok(serde_json::json!({ "entries": entries }))
    }

    fn console_clear(&mut self, _params: &Value) -> EngineResult<Value> {
        self.console.clear();
        Ok(serde_json::json!({}))
    }

    fn console_push_entry(&mut self, params: &Value) -> EngineResult<Value> {
        let level = match params
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
        {
            "trace" => ConsoleLevel::Trace,
            "debug" => ConsoleLevel::Debug,
            "warn" => ConsoleLevel::Warn,
            "error" => ConsoleLevel::Error,
            _ => ConsoleLevel::Info,
        };
        let message = params
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let subsystem = params
            .get("subsystem")
            .and_then(|v| v.as_str())
            .unwrap_or("editor")
            .to_owned();
        self.console.push(ConsoleEntry {
            timestamp: timestamp_now(),
            level,
            source: engine_editor::ConsoleSource {
                subsystem,
                file: params
                    .get("file")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from),
                line: params
                    .get("line")
                    .and_then(|v| v.as_u64())
                    .map(|l| l as u32),
            },
            message,
        });
        Ok(serde_json::json!({}))
    }

    /// Increment the scene version counter so the frontend can skip redundant renders.
    fn bump_scene_version(&mut self) {
        self.scene_version = self.scene_version.wrapping_add(1);
    }

    // ── Viewport handlers ──

    /// Render the current scene to an offscreen buffer and return raw RGBA pixels.
    /// Returns `(width, height, rgba_bytes)`.
    /// If `last_version` param matches the current `scene_version`, skips rendering
    /// and returns `(0, 0, empty_vec)` as a no-change signal.
    fn render_viewport(&mut self, params: &Value) -> EngineResult<(u32, u32, Vec<u8>)> {
        use engine_core::math::{Transform, Vec3};
        use engine_render::{RenderCamera, RenderProjection};
        use runtime_min::extract_render_world;

        let play_mode = params
            .get("play_mode")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Lazy rendering: if the scene version hasn't changed, skip the full pipeline
        if !play_mode {
            if let Some(last_ver) = params.get("last_version").and_then(|v| v.as_u64()) {
                if last_ver == self.scene_version {
                    return Ok((0, 0, Vec::new()));
                }
            }
        } else if let Some(last_ver) = params.get("last_version").and_then(|v| v.as_u64()) {
            if last_ver == self.play_version {
                return Ok((0, 0, Vec::new()));
            }
        }

        let (width, height) = (
            params.get("width").and_then(|v| v.as_u64()).unwrap_or(640) as u32,
            params.get("height").and_then(|v| v.as_u64()).unwrap_or(480) as u32,
        );

        tracing::debug!(
            target: "editor",
            width, height, play_mode,
            "render_viewport start"
        );

        // Extract render world from the scene
        let mut world = if play_mode {
            self.tick_play_runtime()?;
            let Some(runtime) = self.play_runtime.as_ref() else {
                return Err(EngineError::config("play mode is not running"));
            };
            extract_render_world(&runtime.scene)
        } else {
            let Some(project) = self.shell.project() else {
                return Err(EngineError::config("no project open"));
            };
            extract_render_world(&project.scene)
        };

        tracing::debug!(
            target: "editor",
            objects = world.objects.len(),
            lights = world.lights.len(),
            has_camera = world.camera.is_some(),
            "render world extracted"
        );

        // Scene View always uses an editor-controlled camera. Game View keeps
        // the camera extracted from the scene, including Camera2D.
        // If entity_id is provided, render from that entity's camera perspective.
        // If editor_camera is true (inline preview), use editor orbit camera on the game scene.
        let entity_id_str = params.get("entity_id").and_then(|v| v.as_str());
        let editor_camera = params
            .get("editor_camera")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !play_mode || editor_camera {
            let camera_yaw = params.get("yaw").and_then(|v| v.as_f64()).unwrap_or(-0.5) as f32;
            let camera_pitch = params.get("pitch").and_then(|v| v.as_f64()).unwrap_or(0.3) as f32;
            let camera_dist = params
                .get("distance")
                .and_then(|v| v.as_f64())
                .unwrap_or(6.0) as f32;
            let target_x = params
                .get("target_x")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            let target_y = params
                .get("target_y")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            let target_z = params
                .get("target_z")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            let target = Vec3::new(target_x, target_y, target_z);
            let view_mode = params
                .get("view_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("3d");

            // If entity_id is provided, try to use that entity's camera component
            let use_entity_camera = if let Some(id_str) = entity_id_str {
                if let Some(project) = self.shell.project() {
                    let entity_id = engine_core::EntityId::from_u128(
                        u128::from_str_radix(id_str, 16).unwrap_or(0),
                    );
                    if let Some(entity) = project.scene.find_by_id(entity_id) {
                        if let Some(obj) = project.scene.object(entity) {
                            let has_camera = obj
                                .components
                                .iter()
                                .any(|c| matches!(c, engine_ecs::ComponentData::Camera(_)));
                            if has_camera {
                                let transform =
                                    project.scene.transforms().world(entity).unwrap_or_default();
                                let cam_comp = obj.components.iter().find_map(|c| {
                                    if let engine_ecs::ComponentData::Camera(cam) = c {
                                        Some(cam)
                                    } else {
                                        None
                                    }
                                });
                                if let Some(cam) = cam_comp {
                                    let object = world
                                        .camera
                                        .as_ref()
                                        .map(|camera| camera.object)
                                        .unwrap_or_else(|| engine_core::EntityId::from_u128(0));
                                    world.camera = Some(RenderCamera {
                                        object,
                                        transform: Transform {
                                            translation: transform.translation,
                                            rotation: transform.rotation,
                                            ..Transform::IDENTITY
                                        },
                                        projection: RenderProjection::Perspective,
                                        vertical_fov_degrees: cam.vertical_fov_degrees,
                                        near: cam.near,
                                        far: cam.far,
                                        look_at_target: None,
                                    });
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if !use_entity_camera {
                let object = world
                    .camera
                    .as_ref()
                    .map(|camera| camera.object)
                    .unwrap_or_else(|| engine_core::EntityId::from_u128(0));
                let (translation, projection) = if view_mode == "2d" {
                    (
                        Vec3::new(target_x, target_y, target_z + camera_dist),
                        RenderProjection::Orthographic {
                            vertical_size: camera_dist * 2.0,
                        },
                    )
                } else {
                    (
                        Vec3::new(
                            target_x + camera_dist * camera_pitch.cos() * camera_yaw.sin(),
                            target_y + camera_dist * camera_pitch.sin(),
                            target_z + camera_dist * camera_pitch.cos() * camera_yaw.cos(),
                        ),
                        RenderProjection::Perspective,
                    )
                };
                world.camera = Some(RenderCamera {
                    object,
                    transform: Transform {
                        translation,
                        ..Transform::IDENTITY
                    },
                    projection,
                    vertical_fov_degrees: 60.0,
                    near: 0.01,
                    far: 1000.0,
                    look_at_target: Some(target),
                });
            }
        }

        // Lazily create the wgpu render device (with proper error handling)
        if self.render_device.is_none() {
            tracing::info!(target: "engine", width, height, "creating wgpu offscreen device");
            let config = WgpuOffscreenConfig {
                width: width.max(1),
                height: height.max(1),
                format: ImageFormat::Rgba8Srgb,
            };
            self.render_device = Some(WgpuRenderDevice::new_offscreen(config).map_err(|e| {
                tracing::error!(target: "engine", error = %e, "wgpu device creation failed");
                EngineError::other(format!("failed to create wgpu device: {e}"))
            })?);
        }
        let device = self.render_device.as_mut().unwrap();

        // Resize if needed
        let (cur_w, cur_h) = device.default_target_size();
        if cur_w != width || cur_h != height {
            device
                .resize_default_target(width.max(1), height.max(1))
                .map_err(|e| EngineError::other(format!("resize failed: {e}")))?;
        }
        let (cur_gw, cur_gh) = device.game_target_size();
        if cur_gw != width || cur_gh != height {
            device
                .resize_game_target(width.max(1), height.max(1))
                .map_err(|e| EngineError::other(format!("game resize failed: {e}")))?;
        }

        if play_mode {
            // Render to game target, readback from game target
            if let Err(e) = device.render_world_offscreen_game(&world) {
                tracing::error!(target: "engine", error = %e, "game render failed");
                return Err(e);
            }
            let (w, h, rgba) = device.readback_game_target()?;
            tracing::debug!(target: "editor", w, h, bytes = rgba.len(), "game readback ok");
            Ok((w, h, rgba))
        } else {
            // Render to default (scene) target
            if let Err(e) = device.render_world_offscreen(&world) {
                tracing::error!(target: "engine", error = %e, "scene render failed");
                return Err(e);
            }
            let (w, h, rgba) = device.readback_default_target()?;
            tracing::debug!(target: "editor", w, h, bytes = rgba.len(), "scene readback ok");
            Ok((w, h, rgba))
        }
    }

    /// Legacy JSON viewport readback — encodes as PNG + base64.
    /// Prefer `viewport_readback_raw` for performance.
    fn viewport_readback(&mut self, params: &Value) -> EngineResult<Value> {
        let (width, height, rgba) = self.render_viewport(params)?;

        // Encode as PNG
        use image::EncodableLayout;
        let img = image::RgbaImage::from_raw(width.max(1), height.max(1), rgba)
            .ok_or_else(|| EngineError::other("failed to create RGBA image"))?;
        let mut png_bytes = Vec::new();
        {
            use image::codecs::png::PngEncoder;
            use image::ImageEncoder;
            let encoder = PngEncoder::new(&mut png_bytes);
            encoder
                .write_image(
                    img.as_bytes(),
                    img.width(),
                    img.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| EngineError::other(format!("PNG encode failed: {e}")))?;
        }
        let b64 = base64_encode(&png_bytes);

        Ok(serde_json::json!({
            "width": width,
            "height": height,
            "png_base64": b64,
        }))
    }

    /// Binary viewport readback — returns raw RGBA bytes with
    /// [width: u32 LE][height: u32 LE][pixels...] layout.
    /// Frontend receives this as ArrayBuffer via Tauri binary IPC.
    fn viewport_readback_raw(&mut self, params: &Value) -> EngineResult<Vec<u8>> {
        let (width, height, rgba) = self.render_viewport(params)?;

        // Prepend dimensions as u32 LE headers, then raw RGBA pixels
        let mut result = Vec::with_capacity(8 + rgba.len());
        result.extend_from_slice(&(width as u32).to_le_bytes());
        result.extend_from_slice(&(height as u32).to_le_bytes());
        result.extend_from_slice(&rgba);
        Ok(result)
    }

    // ── Shell handlers ──

    // ── Copilot handlers ──

    fn build_agent_context(
        &self,
        scene: engine_ecs::Scene,
    ) -> EngineResult<engine_editor::ProjectContext> {
        use engine_assets::AssetDatabase;

        let project = self
            .shell
            .project()
            .ok_or_else(|| EngineError::config("no project open"))?;

        let manifest = project.manifest.clone();
        let asset_root = project.root.join(&project.manifest.asset_root);
        let builtin_root = project.root.join("builtin");
        let database = AssetDatabase::new(asset_root, builtin_root);

        Ok(engine_editor::ProjectContext {
            scene,
            manifest,
            database,
            registry: engine_assets::AssetRegistry::default(),
            assets: Vec::new(),
            asset_imports: Vec::new(),
            scene_dirty: false,
            root: project.root.clone(),
            scene_path: project.scene_path.clone(),
        })
    }

    fn scene_clone_for_agent(&self) -> EngineResult<engine_ecs::Scene> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        // Round-trip clone via JSON
        let scene_json = project.scene.to_json(project.name())?;
        engine_ecs::Scene::from_json(&scene_json)
    }

    fn get_copilot_settings(&self, _params: &Value) -> EngineResult<Value> {
        let mut value = serde_json::to_value(&self.copilot_settings).unwrap_or_default();
        value["has_api_key"] = serde_json::json!(self.copilot_settings.api_key.is_some());
        Ok(value)
    }

    fn update_copilot_settings(&mut self, params: &Value) -> EngineResult<Value> {
        let mut settings: engine_editor::CopilotSettings =
            serde_json::from_value(params.clone())
                .map_err(|e| EngineError::config(format!("invalid copilot settings: {e}")))?;

        // Preserve existing API key when not explicitly provided in the request
        if !params
            .as_object()
            .map_or(false, |m| m.contains_key("api_key"))
        {
            settings.api_key = self.copilot_settings.api_key.clone();
        }
        if !params
            .as_object()
            .map_or(false, |m| m.contains_key("allowed_commands"))
        {
            settings.allowed_commands = self.copilot_settings.allowed_commands.clone();
        }
        if !settings.provider.endpoint_configurable() {
            settings.api_endpoint = None;
        }

        // Persist non-secret settings into durable state
        let mut settings_for_state = settings.clone();
        settings_for_state.api_key = None; // Never store key in main state file
        self.durable_state.preferences.copilot = settings_for_state;
        self.sync_durable_state();

        self.copilot_settings = settings;
        self.persist_credentials()?;
        Ok(Value::Null)
    }

    fn copilot_allow_command(&mut self, params: &Value) -> EngineResult<Value> {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .ok_or_else(|| EngineError::config("missing 'command'"))?;

        if !self
            .copilot_settings
            .allowed_commands
            .iter()
            .any(|allowed| allowed == command)
        {
            self.copilot_settings
                .allowed_commands
                .push(command.to_owned());
            self.copilot_settings.allowed_commands.sort();
            self.copilot_settings.allowed_commands.dedup();

            let mut settings_for_state = self.copilot_settings.clone();
            settings_for_state.api_key = None;
            self.durable_state.preferences.copilot = settings_for_state;
            self.sync_durable_state();
        }

        Ok(serde_json::json!({ "allowed": true, "command": command }))
    }

    fn persist_credentials(&self) -> EngineResult<()> {
        let Some(path) = self
            .store
            .path()
            .parent()
            .map(|parent| parent.join("credentials.toml"))
        else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let text = toml::to_string_pretty(&CredentialsFile {
            copilot_api_key: self.copilot_settings.api_key.clone(),
            codex_oauth: self.codex_oauth.clone(),
        })
        .map_err(|error| EngineError::other(format!("failed to encode credentials: {error}")))?;
        std::fs::write(&path, text).map_err(|source| EngineError::Filesystem {
            path: path.clone(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
                |source| EngineError::Filesystem {
                    path: path.clone(),
                    source,
                },
            )?;
        }
        Ok(())
    }

    fn codex_oauth_status(&self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({
            "connected": self.codex_oauth.is_some(),
            "account_id": self.codex_oauth.as_ref().and_then(|auth| auth.account_id.as_deref()),
        }))
    }

    fn codex_oauth_start(&mut self, _params: &Value) -> EngineResult<Value> {
        let mut response = ureq::post(format!(
            "{CODEX_OAUTH_ISSUER}/api/accounts/deviceauth/usercode"
        ))
        .header("Content-Type", "application/json")
        .header("User-Agent", concat!("aster/", env!("CARGO_PKG_VERSION")))
        .send_json(serde_json::json!({ "client_id": CODEX_OAUTH_CLIENT_ID }))
        .map_err(|error| {
            EngineError::other(format!("failed to start Codex authorization: {error}"))
        })?;
        let json: Value = response.body_mut().read_json().map_err(|error| {
            EngineError::other(format!("invalid Codex authorization response: {error}"))
        })?;
        let pending = PendingCodexOAuth {
            device_auth_id: json["device_auth_id"]
                .as_str()
                .ok_or_else(|| EngineError::other("Codex authorization omitted device_auth_id"))?
                .to_owned(),
            user_code: json["user_code"]
                .as_str()
                .ok_or_else(|| EngineError::other("Codex authorization omitted user_code"))?
                .to_owned(),
            interval_seconds: json["interval"]
                .as_str()
                .and_then(|value| value.parse().ok())
                .or_else(|| json["interval"].as_u64())
                .unwrap_or(5)
                .max(1),
        };
        let result = serde_json::json!({
            "url": format!("{CODEX_OAUTH_ISSUER}/codex/device"),
            "user_code": pending.user_code,
            "interval_seconds": pending.interval_seconds,
        });
        self.pending_codex_oauth = Some(pending);
        Ok(result)
    }

    fn codex_oauth_poll(&mut self, _params: &Value) -> EngineResult<Value> {
        let pending = self
            .pending_codex_oauth
            .as_ref()
            .ok_or_else(|| EngineError::config("no Codex authorization is currently pending"))?;
        let response = ureq::post(format!(
            "{CODEX_OAUTH_ISSUER}/api/accounts/deviceauth/token"
        ))
        .header("Content-Type", "application/json")
        .header("User-Agent", concat!("aster/", env!("CARGO_PKG_VERSION")))
        .send_json(serde_json::json!({
            "device_auth_id": pending.device_auth_id,
            "user_code": pending.user_code,
        }));

        let mut response = match response {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(403 | 404)) => {
                return Ok(serde_json::json!({ "status": "pending" }));
            }
            Err(error) => {
                return Err(EngineError::other(format!(
                    "Codex authorization polling failed: {error}"
                )));
            }
        };
        let authorization: Value = response.body_mut().read_json().map_err(|error| {
            EngineError::other(format!("invalid Codex authorization result: {error}"))
        })?;
        let authorization_code = authorization["authorization_code"]
            .as_str()
            .ok_or_else(|| EngineError::other("Codex authorization omitted authorization_code"))?;
        let code_verifier = authorization["code_verifier"]
            .as_str()
            .ok_or_else(|| EngineError::other("Codex authorization omitted code_verifier"))?;

        let tokens = exchange_codex_token(&[
            ("grant_type", "authorization_code"),
            ("code", authorization_code),
            (
                "redirect_uri",
                "https://auth.openai.com/deviceauth/callback",
            ),
            ("client_id", CODEX_OAUTH_CLIENT_ID),
            ("code_verifier", code_verifier),
        ])?;
        self.codex_oauth = Some(codex_credential_from_tokens(tokens));
        self.pending_codex_oauth = None;
        self.persist_credentials()?;
        Ok(serde_json::json!({ "status": "connected" }))
    }

    fn codex_oauth_logout(&mut self, _params: &Value) -> EngineResult<Value> {
        self.codex_oauth = None;
        self.pending_codex_oauth = None;
        self.persist_credentials()?;
        Ok(serde_json::json!({ "connected": false }))
    }

    fn ensure_codex_oauth(&mut self) -> EngineResult<engine_ai::providers::CodexOAuthCredentials> {
        let needs_refresh = self
            .codex_oauth
            .as_ref()
            .map(|auth| auth.expires_at_ms <= unix_time_ms().saturating_add(60_000))
            .unwrap_or(false);
        if needs_refresh {
            let refresh_token = self
                .codex_oauth
                .as_ref()
                .map(|auth| auth.refresh_token.clone())
                .unwrap_or_default();
            let tokens = exchange_codex_token(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh_token),
                ("client_id", CODEX_OAUTH_CLIENT_ID),
            ])?;
            self.codex_oauth = Some(codex_credential_from_tokens(tokens));
            self.persist_credentials()?;
        }
        let auth = self.codex_oauth.as_ref().ok_or_else(|| {
            EngineError::config("Codex OAuth is not connected. Sign in with ChatGPT first.")
        })?;
        Ok(engine_ai::providers::CodexOAuthCredentials {
            access_token: auth.access_token.clone(),
            account_id: auth.account_id.clone(),
        })
    }

    fn detect_models(&self, params: &Value) -> EngineResult<Value> {
        let provider_str = params
            .get("provider")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'provider'"))?;
        let provider_kind = match provider_str {
            "anthropic" => engine_ai::registry::ProviderKind::Anthropic,
            "openai" | "open_a_i" => engine_ai::registry::ProviderKind::OpenAI,
            "codex_oauth" => engine_ai::registry::ProviderKind::CodexOAuth,
            "gemini" => engine_ai::registry::ProviderKind::Gemini,
            "ollama" => engine_ai::registry::ProviderKind::Ollama,
            "custom" => engine_ai::registry::ProviderKind::Custom,
            "mimo" => engine_ai::registry::ProviderKind::Mimo,
            "deepseek" => engine_ai::registry::ProviderKind::DeepSeek,
            "glm" => engine_ai::registry::ProviderKind::Glm,
            other => {
                return Err(EngineError::config(format!(
                    "unknown provider for detection: {other}"
                )));
            }
        };

        let config = model_detection_config(params, &self.copilot_settings, &provider_kind);

        let models = engine_ai::registry::detect_available_models(&provider_kind, &config)?;
        Ok(serde_json::to_value(&models).unwrap_or_default())
    }

    fn get_model_registry(&self, params: &Value) -> EngineResult<Value> {
        let registry = engine_ai::registry::ModelRegistry::new();

        let result = if let Some(provider_str) = params.get("provider").and_then(|v| v.as_str()) {
            let provider_kind = match provider_str {
                "anthropic" => engine_ai::registry::ProviderKind::Anthropic,
                "openai" | "open_a_i" => engine_ai::registry::ProviderKind::OpenAI,
                "codex_oauth" => engine_ai::registry::ProviderKind::CodexOAuth,
                "gemini" => engine_ai::registry::ProviderKind::Gemini,
                "ollama" => engine_ai::registry::ProviderKind::Ollama,
                "custom" => engine_ai::registry::ProviderKind::Custom,
                "mimo" => engine_ai::registry::ProviderKind::Mimo,
                "deepseek" => engine_ai::registry::ProviderKind::DeepSeek,
                "glm" => engine_ai::registry::ProviderKind::Glm,
                _ => {
                    return Ok(serde_json::json!({ "models": [] }));
                }
            };
            let models: Vec<_> = registry.builtin_for(&provider_kind).into_iter().collect();
            serde_json::json!({ "models": models })
        } else {
            // Return all providers and their builtin models
            let all: Vec<_> = engine_ai::registry::ProviderKind::builtin_providers()
                .iter()
                .map(|p| {
                    let models: Vec<_> = registry.builtin_for(p).into_iter().collect();
                    serde_json::json!({
                        "provider": p,
                        "display_name": p.display_name(),
                        "requires_api_key": p.requires_api_key(),
                        "requires_endpoint": p.requires_endpoint(),
                        "endpoint_configurable": p.endpoint_configurable(),
                        "default_endpoint": p.default_endpoint(),
                        "models": models,
                    })
                })
                .collect();
            serde_json::json!({ "providers": all })
        };

        Ok(result)
    }

    fn copilot_plan(&mut self, params: &Value) -> EngineResult<Value> {
        self.copilot_plan_streaming(params, &mut |_| {})
    }

    fn copilot_plan_streaming(
        &mut self,
        params: &Value,
        on_delta: &mut dyn FnMut(engine_ai::AiStreamDelta),
    ) -> EngineResult<Value> {
        let prepared = self.prepare_copilot_request(params)?;
        let model = engine_ai::providers::create_provider(
            &prepared.provider,
            &prepared.model,
            prepared.api_key.as_deref(),
            prepared.endpoint.as_deref(),
            prepared.max_tokens,
            prepared.codex_oauth,
            prepared.mimo_config.as_ref(),
            prepared.glm_config.as_ref(),
        )?;
        let response = model.chat_stream(prepared.request, on_delta)?;
        self.finish_copilot_response_with_tools(
            &prepared.original_prompt,
            &response.content,
            &response.tool_calls,
            prepared.cached_context,
        )
    }

    fn prepare_copilot_request(&mut self, params: &Value) -> EngineResult<PreparedCopilotRequest> {
        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'prompt'"))?;

        // Update copilot settings if provided in the request
        if let Some(settings) = params.get("settings") {
            if let Ok(parsed) =
                serde_json::from_value::<engine_editor::CopilotSettings>(settings.clone())
            {
                self.copilot_settings = parsed;
            }
        }

        // Parse thinking_effort from request
        let thinking_effort = params.get("thinking_effort").and_then(|v| {
            let s = v.as_str()?;
            match s {
                "off" => Some(engine_ai::ThinkingEffort::Off),
                "low" => Some(engine_ai::ThinkingEffort::Low),
                "medium" => Some(engine_ai::ThinkingEffort::Medium),
                "high" => Some(engine_ai::ThinkingEffort::High),
                _ => None,
            }
        });

        // Build enriched prompt with selected entity context
        let enriched_prompt = if let Some(entity) = params.get("selected_entity") {
            format!(
                "{}\n\n[Selected Entity Context]\n{}",
                prompt,
                serde_json::to_string_pretty(entity).unwrap_or_default()
            )
        } else {
            prompt.to_string()
        };

        let scene = self.scene_clone_for_agent()?;
        let ctx = self.build_agent_context(scene)?;

        let session = AgentSession::new(ctx)?;

        // Create the AI model from settings, falling back to a helpful error message
        let provider_str = match self.copilot_settings.provider {
            engine_editor::CopilotProvider::Anthropic => "anthropic",
            engine_editor::CopilotProvider::Ollama => "ollama",
            engine_editor::CopilotProvider::OpenAI => "openai",
            engine_editor::CopilotProvider::CodexOAuth => "codex_oauth",
            engine_editor::CopilotProvider::Gemini => "gemini",
            engine_editor::CopilotProvider::Custom => "custom",
            engine_editor::CopilotProvider::Mimo => "mimo",
            engine_editor::CopilotProvider::DeepSeek => "deepseek",
            engine_editor::CopilotProvider::Glm => "glm",
            engine_editor::CopilotProvider::Stub => {
                return Err(EngineError::config(
                    "Copilot is in stub mode. Go to Settings → Copilot to configure a real provider.",
                ));
            }
        };

        let codex_oauth = if provider_str == "codex_oauth" {
            Some(self.ensure_codex_oauth()?)
        } else {
            None
        };
        Ok(PreparedCopilotRequest {
            request: session.prepare_request(
                &enriched_prompt,
                &self.copilot_conversation,
                thinking_effort,
            ),
            original_prompt: prompt.to_string(),
            provider: provider_str.to_owned(),
            model: self.copilot_settings.model.clone(),
            api_key: self.copilot_settings.api_key.clone(),
            endpoint: if self.copilot_settings.provider.endpoint_configurable() {
                self.copilot_settings.api_endpoint.clone()
            } else {
                None
            },
            max_tokens: self.copilot_settings.max_tokens,
            codex_oauth,
            mimo_config: if provider_str == "mimo" {
                Some(self.copilot_settings.mimo_config.clone())
            } else {
                None
            },
            glm_config: if provider_str == "glm" {
                Some(self.copilot_settings.glm_config.clone())
            } else {
                None
            },
            cached_context: session.context,
        })
    }

    fn finish_copilot_response_with_tools(
        &mut self,
        original_prompt: &str,
        response: &str,
        tool_calls: &[engine_ai::ToolCall],
        cached_context: engine_editor::ProjectContext,
    ) -> EngineResult<Value> {
        let mut session = AgentSession::new(cached_context)?;

        let mut plan = if !tool_calls.is_empty() {
            session.plan_from_tool_calls(
                tool_calls,
                response,
                PermissionPolicy::transactional_write(),
            )?
        } else {
            session.plan_from_response(response, PermissionPolicy::transactional_write())?
        };

        let assistant_message = plan
            .operations
            .iter()
            .find_map(|planned| match &planned.operation {
                engine_ai::AgentOperation::Complete { summary } => summary.clone(),
                _ => None,
            })
            .unwrap_or_default();
        plan.operations.retain(|planned| {
            !matches!(
                &planned.operation,
                engine_ai::AgentOperation::Complete { .. }
            )
        });
        plan.read_only = plan.operations.iter().all(|op| !op.requires_write);
        plan.requires_write = plan.operations.iter().any(|op| op.requires_write);

        let operations: Vec<serde_json::Value> = plan
            .operations
            .iter()
            .enumerate()
            .map(|(i, op)| {
                let command = match &op.operation {
                    engine_ai::AgentOperation::ExecuteCommand { command, .. } => {
                        Some(command.as_str())
                    }
                    _ => None,
                };
                let permission_kind = if command.is_some() {
                    "command"
                } else if op.requires_write {
                    "write"
                } else {
                    "read"
                };
                let permanently_allowed = command.is_some_and(|command| {
                    self.copilot_settings
                        .allowed_commands
                        .iter()
                        .any(|allowed| allowed == command)
                });
                serde_json::json!({
                    "index": i,
                    "preview": op.preview,
                    "requires_write": op.requires_write,
                    "permission_kind": permission_kind,
                    "command": command,
                    "permanently_allowed": permanently_allowed,
                })
            })
            .collect();

        self.copilot_conversation
            .push(engine_ai::ChatMessage::user(original_prompt));

        let history_message = if assistant_message.is_empty() {
            let plan_summary: Vec<String> = plan
                .operations
                .iter()
                .map(|op| op.preview.clone())
                .collect();
            format!(
                "Proposed {} operation(s):\n{}",
                plan.operations.len(),
                plan_summary.join("\n")
            )
        } else {
            assistant_message.clone()
        };
        self.copilot_conversation
            .push(engine_ai::ChatMessage::assistant(history_message));

        // Trim conversation to prevent unbounded growth
        while self.copilot_conversation.len() > MAX_COPILOT_CONVERSATION_MESSAGES {
            self.copilot_conversation.remove(0);
        }

        self.last_copilot_plan = Some(plan);

        Ok(serde_json::json!({
            "message": assistant_message,
            "operations": operations,
            "read_only": operations.iter().all(|o| !o["requires_write"].as_bool().unwrap_or(true)),
            "requires_write": operations.iter().any(|o| o["requires_write"].as_bool().unwrap_or(false)),
        }))
    }

    fn copilot_apply(&mut self, params: &Value) -> EngineResult<Value> {
        let approved_indices: Vec<usize> = params
            .get("approved_indices")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64())
                    .map(|i| i as usize)
                    .collect()
            })
            .ok_or_else(|| EngineError::config("missing 'approved_indices' array"))?;

        let plan = self.last_copilot_plan.take().ok_or_else(|| {
            EngineError::config("no pending copilot plan — call copilot/plan first")
        })?;

        // Filter the plan to only approved operations
        let filtered_ops: Vec<_> = plan
            .operations
            .into_iter()
            .enumerate()
            .filter(|(i, _)| approved_indices.contains(i))
            .map(|(_, op)| op)
            .collect();
        let applied_read_only = filtered_ops.iter().all(|op| !op.requires_write);

        if filtered_ops.is_empty() {
            return Ok(serde_json::json!({
                "operations_performed": 0,
                "completed": false,
                "trace_entries": [],
                "console_entries": [],
                "summary": null
            }));
        }

        let scene = self.scene_clone_for_agent()?;
        let ctx = self.build_agent_context(scene)?;

        let mut session = AgentSession::new(ctx)?;

        let apply_plan = AgentPlan {
            operations: filtered_ops,
            read_only: false,
            requires_write: true,
            policy: PermissionPolicy::transactional_write(),
        };

        let outcome = session.apply_plan(&apply_plan)?;

        // Write the modified scene back to the real project
        if let Some(project) = self.shell.project_mut() {
            project.scene = session.context.scene;
            project.scene_dirty = true;
            project.asset_imports.extend(session.context.asset_imports);
            for entry in session.console.entries().iter() {
                self.console.push(entry.clone());
            }
        }

        self.bump_scene_version();

        // Record execution results in conversation history so the model has
        // context about what happened when the user follows up.
        let console_results: Vec<String> = session
            .console
            .entries()
            .iter()
            .filter(|entry| entry.source.subsystem == "ai-agent")
            .map(|entry| entry.message.clone())
            .collect();
        let trace_statuses: Vec<String> = outcome
            .trace_entries
            .iter()
            .map(|t| format!("{}: {}", t.tool, t.result))
            .collect();
        let execution_summary = copilot_execution_summary(
            outcome.operations_performed,
            outcome.summary.as_deref(),
            &trace_statuses,
            &console_results,
        );
        self.copilot_conversation
            .push(engine_ai::ChatMessage::assistant(execution_summary));

        // Trim conversation to prevent unbounded growth
        while self.copilot_conversation.len() > MAX_COPILOT_CONVERSATION_MESSAGES {
            self.copilot_conversation.remove(0);
        }

        let trace_entries: Vec<serde_json::Value> = outcome
            .trace_entries
            .iter()
            .map(|t| {
                serde_json::json!({
                    "tool": t.tool,
                    "result": t.result,
                    "recovery_hint": t.recovery_hint,
                })
            })
            .collect();

        let console_entries: Vec<serde_json::Value> = outcome
            .console_entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "level": format!("{:?}", e.level).to_lowercase(),
                    "message": e.message,
                    "subsystem": e.source.subsystem,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "operations_performed": outcome.operations_performed,
            "completed": outcome.completed,
            "summary": outcome.summary,
            "trace_entries": trace_entries,
            "console_entries": console_entries,
            "needs_continuation": should_continue_copilot(applied_read_only, outcome.completed),
        }))
    }

    fn copilot_clear_conversation(&mut self, _params: &Value) -> EngineResult<Value> {
        self.copilot_conversation.clear();
        self.last_copilot_plan = None;
        Ok(Value::Null)
    }

    fn copilot_get_conversation_length(&self, _params: &Value) -> EngineResult<Value> {
        // Return the number of turns (pairs) in the conversation
        let turns = self.copilot_conversation.len() / 2;
        Ok(serde_json::json!({ "turns": turns, "messages": self.copilot_conversation.len() }))
    }

    fn shell_get_state(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({
            "has_project": self.shell.project().is_some(),
            "project_name": self.shell.project().map(|p| p.name()),
            "scene_dirty": self.shell.is_scene_dirty(),
            "can_undo": self.shell.undo_stack().can_undo(),
            "can_redo": self.shell.undo_stack().can_redo(),
            "scene_version": self.scene_version,
            "desktop_integration": self.desktop_integration.as_json(),
        }))
    }

    fn shell_get_scene_tree(&mut self, _params: &Value) -> EngineResult<Value> {
        let Some(project) = self.shell.project() else {
            return Ok(serde_json::json!({ "objects": [] }));
        };
        let objects: Vec<Value> = project
            .scene
            .objects()
            .iter()
            .map(|(entity, obj)| {
                let transform = project
                    .scene
                    .transforms()
                    .world(*entity)
                    .unwrap_or_default();
                let parent = project.scene.transforms().parent(*entity);
                let parent_id = parent
                    .and_then(|p| project.scene.object(p))
                    .map(|o| format!("{:032x}", o.id.as_u128()));
                serde_json::json!({
                    "id": format!("{:032x}", obj.id.as_u128()),
                    "name": obj.name,
                    "tag": obj.tag,
                    "parent_id": parent_id,
                    "position": [
                        transform.translation.x,
                        transform.translation.y,
                        transform.translation.z,
                    ],
                })
            })
            .collect();
        Ok(serde_json::json!({ "objects": objects }))
    }

    fn shell_get_entity(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id' parameter"))?;
        let entity_id_val = u128::from_str_radix(id_str, 16)
            .map_err(|_| EngineError::config("invalid entity id"))?;
        let entity_id = engine_core::EntityId::from_u128(entity_id_val);

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;
        let Some(obj) = project.scene.object(entity) else {
            return Err(EngineError::config("entity not found"));
        };
        let transform = project.scene.transforms().world(entity).unwrap_or_default();
        let components: Vec<Value> = obj
            .components
            .iter()
            .filter_map(|c| {
                serde_json::to_value(c).ok().map(|val| {
                    let comp_type = val
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let data = val.get("data").cloned().unwrap_or(serde_json::Value::Null);
                    serde_json::json!({
                        "type": comp_type,
                        "data": data,
                    })
                })
            })
            .collect();

        Ok(serde_json::json!({
            "id": id_str,
            "name": obj.name,
            "tag": obj.tag,
            "transform": {
                "position": [transform.translation.x, transform.translation.y, transform.translation.z],
                "rotation": [transform.rotation.x, transform.rotation.y, transform.rotation.z, transform.rotation.w],
                "scale": [transform.scale.x, transform.scale.y, transform.scale.z],
            },
            "components": components,
        }))
    }

    fn shell_select_entity(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params.get("id").and_then(|v| v.as_str());
        match id_str {
            Some(id) => {
                self.shell
                    .select_entity_id(engine_core::EntityId::from_u128(
                        u128::from_str_radix(id, 16)
                            .map_err(|_| EngineError::config("invalid entity id"))?,
                    ));
                Ok(serde_json::json!({ "selected": id }))
            }
            None => {
                self.shell.selection_mut().clear();
                Ok(serde_json::json!({ "selected": null }))
            }
        }
    }

    fn shell_save_scene(&mut self, _params: &Value) -> EngineResult<Value> {
        let path = self.shell.save_scene()?;
        self.drain_shell_console();
        Ok(serde_json::json!({ "path": path }))
    }

    /// Open a scene from an arbitrary JSON file path.
    /// Reads the file, parses it as a scene, and replaces the current project's scene.
    fn shell_open_scene(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let path = std::path::PathBuf::from(path_str);

        let text = std::fs::read_to_string(&path).map_err(|e| EngineError::Filesystem {
            path: path.clone(),
            source: e,
        })?;
        let new_scene = engine_ecs::Scene::from_json(&text)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        project.scene = new_scene;
        project.scene_path = path.clone();
        project.scene_dirty = false;
        self.bump_scene_version();

        self.console.push(engine_editor::ConsoleEntry {
            timestamp: timestamp_now(),
            level: engine_editor::ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".to_string(),
                file: None,
                line: None,
            },
            message: format!("opened scene {}", path.display()),
        });

        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
        }))
    }

    /// Save the scene to a specified path (Save As).
    fn shell_save_scene_as(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let path = std::path::PathBuf::from(path_str);

        let display_path = self.shell.save_scene_as(&path)?;
        self.drain_shell_console();
        self.bump_scene_version();

        Ok(serde_json::json!({ "path": display_path }))
    }

    fn shell_undo(&mut self, _params: &Value) -> EngineResult<Value> {
        let ok = self.shell.undo_scene_command()?;
        self.drain_shell_console();
        self.bump_scene_version();
        Ok(serde_json::json!({ "applied": ok }))
    }

    fn shell_redo(&mut self, _params: &Value) -> EngineResult<Value> {
        let ok = self.shell.redo_scene_command()?;
        self.drain_shell_console();
        self.bump_scene_version();
        Ok(serde_json::json!({ "applied": ok }))
    }

    fn shell_close_project(&mut self, _params: &Value) -> EngineResult<Value> {
        self.stop_play_runtime();
        self.shell.close_project();
        self.durable_state = self.hub.durable_state();
        self.durable_state.last_open_project = None;
        self.hub = HubState::from_durable_state(self.durable_state.clone());
        self.persist_state();
        Ok(serde_json::json!({}))
    }

    // ── Scene Guides ──

    fn scene_get_guides(&mut self, _params: &Value) -> EngineResult<Value> {
        let Some(project) = self.shell.project() else {
            return Ok(serde_json::json!({ "guides": [] }));
        };

        let mut guides: Vec<Value> = Vec::new();

        for (entity, obj) in project.scene.objects() {
            let transform = project.scene.transforms().world(entity).unwrap_or_default();

            for comp in &obj.components {
                match comp {
                    engine_ecs::ComponentData::Camera(cam) => {
                        guides.push(serde_json::json!({
                            "id": format!("{:032x}", obj.id.as_u128()),
                            "position": [
                                transform.translation.x,
                                transform.translation.y,
                                transform.translation.z,
                            ],
                            "rotation": [0.0_f32, 0.0, 0.0],
                            "componentType": "Camera",
                            "fov": cam.vertical_fov_degrees,
                        }));
                    }
                    engine_ecs::ComponentData::Light(light) => {
                        guides.push(serde_json::json!({
                            "id": format!("{:032x}", obj.id.as_u128()),
                            "position": [
                                transform.translation.x,
                                transform.translation.y,
                                transform.translation.z,
                            ],
                            "rotation": [0.0_f32, 0.0, 0.0],
                            "componentType": "Light",
                            "lightKind": light.kind.as_str(),
                            "lightColor": light.color,
                        }));
                    }
                    _ => {}
                }
            }
        }

        Ok(serde_json::json!({ "guides": guides }))
    }

    // ── Scene CRUD ──

    fn shell_create_object(&mut self, params: &Value) -> EngineResult<Value> {
        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        // Optional parent lookup
        let parent_entity = params
            .get("parent_id")
            .and_then(|v| v.as_str())
            .map(|id_str| {
                let pid = engine_core::EntityId::from_u128(
                    u128::from_str_radix(id_str, 16)
                        .map_err(|_| EngineError::config("invalid parent id"))?,
                );
                project
                    .scene
                    .find_by_id(pid)
                    .ok_or_else(|| EngineError::config("parent entity not found"))
            })
            .transpose()?;

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("New Object");

        let entity = project.scene.create_object(name)?;

        if let Some(parent) = parent_entity {
            project.scene.set_parent(entity, Some(parent))?;
        }

        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Create Object", "", before, after));
        self.bump_scene_version();

        let project = self.shell.project().unwrap();
        let obj = project.scene.object(entity).unwrap();
        let transform = project.scene.transforms().world(entity).unwrap_or_default();

        Ok(serde_json::json!({
            "id": format!("{:032x}", obj.id.as_u128()),
            "name": obj.name,
            "tag": obj.tag,
            "position": [
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ],
        }))
    }

    fn shell_rename_object(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let new_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        if let Some(obj) = project.scene.object_mut(entity) {
            obj.name = new_name.to_owned();
            project.scene_dirty = true;
        }

        self.bump_scene_version();
        Ok(serde_json::json!({ "renamed": id_str, "name": new_name }))
    }

    fn shell_delete_object(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        project.scene.destroy_deferred(entity)?;
        project.scene.process_deferred_destroy()?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Delete Object", id_str, before, after));
        self.bump_scene_version();
        Ok(serde_json::json!({ "deleted": true }))
    }

    fn shell_duplicate_object(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        let new_entity = project.scene.clone_object(entity)?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Duplicate Object", id_str, before, after));
        self.bump_scene_version();

        let project = self.shell.project().unwrap();
        let obj = project.scene.object(new_entity).unwrap();
        let transform = project
            .scene
            .transforms()
            .world(new_entity)
            .unwrap_or_default();

        Ok(serde_json::json!({
            "id": format!("{:032x}", obj.id.as_u128()),
            "name": obj.name,
            "tag": obj.tag,
            "position": [
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ],
        }))
    }

    fn shell_reparent_object(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let parent_id_str = params.get("parent_id").and_then(|v| v.as_str());

        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        let parent_entity = match parent_id_str {
            Some(pid) => {
                let parent_eid = engine_core::EntityId::from_u128(
                    u128::from_str_radix(pid, 16)
                        .map_err(|_| EngineError::config("invalid parent id"))?,
                );
                Some(
                    project
                        .scene
                        .find_by_id(parent_eid)
                        .ok_or_else(|| EngineError::config("parent entity not found"))?,
                )
            }
            None => None,
        };

        project.scene.set_parent(entity, parent_entity)?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Reparent Object", id_str, before, after));
        self.bump_scene_version();
        Ok(serde_json::json!({ "reparented": true }))
    }

    fn shell_update_transform(&mut self, params: &Value) -> EngineResult<Value> {
        use engine_core::math::{Quat, Transform, Vec3};

        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        // Read current transform as starting point
        let current = project.scene.transforms().local(entity).unwrap_or_default();

        let mut t = Transform {
            translation: current.translation,
            rotation: current.rotation,
            scale: current.scale,
        };

        if let Some(pos) = params.get("position").and_then(|v| v.as_array()) {
            let x = pos
                .get(0)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.translation.x as f64) as f32;
            let y = pos
                .get(1)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.translation.y as f64) as f32;
            let z = pos
                .get(2)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.translation.z as f64) as f32;
            t.translation = Vec3::new(x, y, z);
        }
        if let Some(rot) = params.get("rotation").and_then(|v| v.as_array()) {
            let x = rot
                .get(0)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.rotation.x as f64) as f32;
            let y = rot
                .get(1)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.rotation.y as f64) as f32;
            let z = rot
                .get(2)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.rotation.z as f64) as f32;
            let w = rot
                .get(3)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.rotation.w as f64) as f32;
            t.rotation = Quat { x, y, z, w };
        }
        if let Some(scl) = params.get("scale").and_then(|v| v.as_array()) {
            let x = scl
                .get(0)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.scale.x as f64) as f32;
            let y = scl
                .get(1)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.scale.y as f64) as f32;
            let z = scl
                .get(2)
                .and_then(|v| v.as_f64())
                .unwrap_or(t.scale.z as f64) as f32;
            t.scale = Vec3::new(x, y, z);
        }

        project.scene.transforms_mut().set_local(entity, t);
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        if before != after {
            self.shell
                .push_undo(UndoCommand::new("Update Transform", id_str, before, after));
        }
        self.bump_scene_version();
        Ok(serde_json::json!({ "updated": true }))
    }

    fn shell_add_component(&mut self, params: &Value) -> EngineResult<Value> {
        use engine_ecs::ComponentData;

        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let comp_type = params
            .get("component_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'component_type'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        let component = match comp_type {
            "Camera" => ComponentData::Camera(Default::default()),
            "Light" => ComponentData::Light(Default::default()),
            "MeshRenderer" => ComponentData::MeshRenderer(Default::default()),
            "Rigidbody" => ComponentData::Rigidbody(Default::default()),
            "Collider" => ComponentData::Collider(Default::default()),
            "AudioSource" => ComponentData::AudioSource(Default::default()),
            "AudioListener" => ComponentData::AudioListener(Default::default()),
            "AcousticMaterial" => ComponentData::AcousticMaterial(Default::default()),
            "AcousticGeometry" => ComponentData::AcousticGeometry(Default::default()),
            "AcousticRoom" => ComponentData::AcousticRoom(Default::default()),
            "AcousticPortal" => ComponentData::AcousticPortal(Default::default()),
            "AudioZone" => ComponentData::AudioZone(Default::default()),
            "Script" => ComponentData::Script(engine_ecs::ScriptComponentProxy {
                backend: "rhai".to_owned(),
                script: String::new(),
                state_json: None,
                pending_recovery: false,
            }),
            _ => {
                return Err(EngineError::config(format!(
                    "unknown component type: {comp_type}"
                )))
            }
        };

        project.scene.upsert_component(entity, component)?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Add Component", id_str, before, after));
        self.bump_scene_version();
        Ok(serde_json::json!({ "added": comp_type }))
    }

    fn shell_update_component(&mut self, params: &Value) -> EngineResult<Value> {
        use engine_ecs::ComponentData;

        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let comp_type = params
            .get("component_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'component_type'"))?;
        let field_data = params
            .get("data")
            .ok_or_else(|| EngineError::config("missing 'data'"))?;

        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        // Get the current component, merge with new data, and upsert
        let components = project
            .scene
            .components(entity)
            .ok_or_else(|| EngineError::config("entity has no components"))?;

        let current = components
            .iter()
            .find(|c| c.type_id() == comp_type)
            .ok_or_else(|| EngineError::config(format!("entity has no {comp_type} component")))?;

        // Serialize current data, merge fields, deserialize back
        let mut current_val =
            serde_json::to_value(current).map_err(|e| EngineError::other(e.to_string()))?;

        // Merge the new data into the existing component data
        if let Some(obj) = current_val.as_object_mut() {
            if let Some(data_obj) = obj.get_mut("data").and_then(|d| d.as_object_mut()) {
                if let Some(fields) = field_data.as_object() {
                    for (key, value) in fields {
                        data_obj.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        let component: ComponentData = serde_json::from_value(current_val)
            .map_err(|e| EngineError::config(format!("invalid component data: {e}")))?;

        project.scene.upsert_component(entity, component)?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Update Component", id_str, before, after));
        self.bump_scene_version();
        Ok(serde_json::json!({ "updated": comp_type }))
    }

    fn shell_remove_component(&mut self, params: &Value) -> EngineResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'id'"))?;
        let comp_type = params
            .get("component_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'component_type'"))?;
        let entity_id = engine_core::EntityId::from_u128(
            u128::from_str_radix(id_str, 16)
                .map_err(|_| EngineError::config("invalid entity id"))?,
        );

        let before = self.scene_snapshot()?;
        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        let entity = project
            .scene
            .find_by_id(entity_id)
            .ok_or_else(|| EngineError::config("entity not found"))?;

        project.scene.remove_component(entity, comp_type)?;
        project.scene_dirty = true;
        let after = self.scene_snapshot()?;
        self.shell
            .push_undo(UndoCommand::new("Remove Component", id_str, before, after));
        self.bump_scene_version();
        Ok(serde_json::json!({ "removed": comp_type }))
    }

    // ── Play handlers ──

    fn play_start(&mut self, _params: &Value) -> EngineResult<Value> {
        self.start_play_runtime()?;
        Ok(serde_json::json!({
            "playing": true,
            "play_version": self.play_version,
        }))
    }

    fn play_stop(&mut self, _params: &Value) -> EngineResult<Value> {
        self.stop_play_runtime();
        Ok(serde_json::json!({ "playing": false }))
    }

    fn play_get_state(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({
            "playing": self.play_runtime.is_some(),
            "play_version": self.play_version,
        }))
    }

    // ── Helpers ──

    fn sync_durable_state(&mut self) {
        // HubState owns the general editor preferences, while copilot settings are
        // updated through their own RPC. Preserve them when rebuilding durable state.
        let copilot_settings = self.durable_state.preferences.copilot.clone();
        self.durable_state = self.hub.durable_state();
        self.durable_state.preferences.copilot = copilot_settings;
        if let Some(project) = self.shell.project() {
            self.durable_state.last_open_project = Some(project.root.clone());
        }
        self.persist_state();
    }

    fn persist_state(&self) {
        self.store.save(&self.durable_state).ok();
    }

    fn reopen_last_project_if_needed(&mut self) {
        if !self.hub.preferences().reopen_last_project {
            return;
        }
        let Some(path) = self.durable_state.last_open_project.clone() else {
            return;
        };
        if self.shell.open_project(&path).is_ok() {
            self.hub.mark_project_open(path);
            self.drain_shell_console();
        }
    }

    fn scene_snapshot(&self) -> EngineResult<String> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        project.scene.to_json(project.name())
    }

    fn create_play_runtime(&self) -> EngineResult<RuntimeServices> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let config = EngineConfig::new(
            project.name().to_owned(),
            project.root.clone(),
            RuntimeProfile::RuntimeGame,
        );
        let mut runtime =
            headless_services_from_scene(config, project.root.clone(), &project.scene)?;
        runtime.load_project_assets(project.root.join(&project.manifest.asset_root))?;
        Ok(runtime)
    }

    fn create_game_runtime_snapshot(&self) -> EngineResult<game_window::GameRuntimeSnapshot> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let config = EngineConfig::new(
            project.name().to_owned(),
            project.root.clone(),
            RuntimeProfile::RuntimeGame,
        );
        Ok(game_window::GameRuntimeSnapshot::new(
            config,
            project.root.clone(),
            project.root.join(&project.manifest.asset_root),
            project.scene.to_json(project.name())?,
        ))
    }

    fn start_play_runtime(&mut self) -> EngineResult<()> {
        self.play_runtime = Some(self.create_play_runtime()?);
        self.play_last_frame = Some(Instant::now());
        self.play_version = self.play_version.wrapping_add(1);
        Ok(())
    }

    fn stop_play_runtime(&mut self) {
        self.play_runtime = None;
        self.play_last_frame = None;
        self.play_version = self.play_version.wrapping_add(1);
    }

    fn tick_play_runtime(&mut self) -> EngineResult<()> {
        if self.play_runtime.is_none() {
            self.start_play_runtime()?;
        }
        let now = Instant::now();
        let delta = self
            .play_last_frame
            .map(|last| now.saturating_duration_since(last))
            .unwrap_or_else(|| Duration::from_secs_f32(1.0 / 60.0));
        self.play_last_frame = Some(now);
        if let Some(runtime) = self.play_runtime.as_mut() {
            runtime.tick_game_frame(delta.min(Duration::from_millis(100)), false)?;
            self.play_version = self.play_version.wrapping_add(1);
        }
        Ok(())
    }

    /// Polls events from the native game window and handles close/error.
    fn poll_game_window(&mut self) {
        let Some(gw) = self.game_window.as_ref() else {
            return;
        };

        for event in gw.poll_events() {
            match event {
                game_window::GameEvent::Closed => {
                    tracing::debug!(target: "editor", "game window hidden");
                }
                game_window::GameEvent::Error(msg) => {
                    tracing::error!(target: "editor", "game window error: {msg}");
                }
            }
        }
    }

    /// Forward console entries from the shell's console service to our shared one.
    fn drain_shell_console(&mut self) {
        for entry in self.shell.console().entries().iter() {
            self.console.push(entry.clone());
        }
    }
}

fn model_detection_config(
    params: &Value,
    settings: &engine_editor::CopilotSettings,
    provider: &engine_ai::registry::ProviderKind,
) -> engine_ai::registry::ProviderConfig {
    engine_ai::registry::ProviderConfig {
        api_key: params
            .get("api_key")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| settings.api_key.clone()),
        endpoint: match provider {
            engine_ai::registry::ProviderKind::Mimo => Some(
                engine_ai::registry::MimoEndpoints::base_url(
                    &settings.mimo_config.billing,
                    &settings.mimo_config.region,
                )
                .to_owned(),
            ),
            engine_ai::registry::ProviderKind::Glm => Some(
                engine_ai::registry::GlmEndpoints::base_url(
                    &settings.glm_config.billing,
                    &settings.glm_config.region,
                )
                .to_owned(),
            ),
            _ if provider.endpoint_configurable() => params
                .get("endpoint")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| settings.api_endpoint.clone()),
            _ => None,
        },
    }
}

fn should_continue_copilot(applied_read_only: bool, completed: bool) -> bool {
    applied_read_only && !completed
}

fn copilot_execution_summary(
    operations_performed: usize,
    summary: Option<&str>,
    trace_statuses: &[String],
    tool_results: &[String],
) -> String {
    let mut text = if let Some(summary) = summary {
        format!("Executed {operations_performed} operation(s). Result: {summary}")
    } else {
        format!(
            "Executed {operations_performed} operation(s).\n{}",
            trace_statuses.join("\n")
        )
    };
    if !tool_results.is_empty() {
        text.push_str("\nTool results:\n");
        text.push_str(&tool_results.join("\n"));
    }
    text
}

// ─── Thread-safe wrapper ─────────────────────────────────────────────────────

/// Thread-safe wrapper for `EditorHost`.
///
/// `EditorHost` transitively contains `rhai::Engine` (`!Send`) via
/// `RuntimeServices`, so it cannot be made `Send`. This wrapper uses
/// `UnsafeCell` + `Mutex<()>` to provide exclusive access while
/// recording the creating thread ID at construction.
///
/// # Safety
///
/// Tauri synchronous `#[tauri::command]` functions always execute on
/// the main thread, ensuring thread affinity. `with_host()` verifies at
/// runtime that the caller is the creating thread. An `unsafe impl Send`
/// + `Sync` is required because `State<'_, T>` needs `T: Send + Sync`,
/// but access is checked on every invocation.
pub struct EditorHostState {
    host: UnsafeCell<EditorHost>,
    lock: Mutex<()>,
    thread_id: ThreadId,
}

// SAFETY: `with_host()` asserts the calling thread matches `thread_id`
// at runtime. Mutex provides exclusive access. Tauri sync commands run
// on the main thread, upholding the thread-affinity invariant.
unsafe impl Send for EditorHostState {}
unsafe impl Sync for EditorHostState {}

impl EditorHostState {
    pub fn new(host: EditorHost) -> Self {
        Self {
            host: UnsafeCell::new(host),
            lock: Mutex::new(()),
            thread_id: std::thread::current().id(),
        }
    }

    /// Access the inner `EditorHost` under lock.
    ///
    /// # Panics
    ///
    /// Panics if called from a thread other than the one that created
    /// this instance (catches cross-thread `!Send` access in debug
    /// builds — release builds still check).
    pub fn with_host<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut EditorHost) -> R,
    {
        let current_id = std::thread::current().id();
        assert_eq!(
            current_id, self.thread_id,
            "EditorHostState accessed from thread {:?} but was created on {:?}",
            current_id, self.thread_id
        );
        let _guard = self.lock.lock().expect("poisoned lock");
        // SAFETY: Thread-affinity assertion + mutex guarantee exclusive
        // mutable access from the correct thread.
        f(unsafe { &mut *self.host.get() })
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

#[tauri::command]
fn start_copilot_plan(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    requests: State<'_, CopilotRequestState>,
    request_id: String,
    params: Value,
) -> Result<(), String> {
    let prepared = state
        .with_host(|host| host.prepare_copilot_request(&params))
        .map_err(|error| error.to_string())?;
    let requests = requests.requests.clone();
    std::thread::spawn(move || {
        let original_prompt = prepared.original_prompt.clone();
        let cached_context = prepared.cached_context;
        let result = engine_ai::providers::create_provider(
            &prepared.provider,
            &prepared.model,
            prepared.api_key.as_deref(),
            prepared.endpoint.as_deref(),
            prepared.max_tokens,
            prepared.codex_oauth,
            prepared.mimo_config.as_ref(),
            prepared.glm_config.as_ref(),
        )
        .and_then(|model| {
            model.chat_stream(prepared.request, &mut |delta| {
                if requests
                    .lock()
                    .expect("poisoned lock")
                    .cancelled
                    .contains(&request_id)
                {
                    return;
                }
                let delta_payload = match &delta {
                    engine_ai::AiStreamDelta::ToolCallDelta(tc) => {
                        serde_json::to_string(tc).unwrap_or_default()
                    }
                    _ => delta.text().to_owned(),
                };
                let _ = app.emit(
                    "copilot-stream",
                    serde_json::json!({
                        "request_id": request_id,
                        "kind": delta.kind(),
                        "delta": delta_payload,
                    }),
                );
            })
        });
        let mut request_state = requests.lock().expect("poisoned lock");
        if request_state.cancelled.remove(&request_id) {
            drop(request_state);
            let _ = app.emit(
                "copilot-stream-complete",
                serde_json::json!({ "request_id": request_id }),
            );
            return;
        }
        let (content_result, tool_calls) = match result {
            Ok(response) => (Ok(response.content), response.tool_calls),
            Err(e) => (Err(e.to_string()), Vec::new()),
        };
        request_state.completed.insert(
            request_id.clone(),
            CompletedCopilotRequest {
                original_prompt,
                response: content_result,
                tool_calls,
                cached_context,
            },
        );
        drop(request_state);
        let _ = app.emit(
            "copilot-stream-complete",
            serde_json::json!({ "request_id": request_id }),
        );
    });
    Ok(())
}

#[tauri::command]
fn finish_copilot_plan(
    state: State<'_, EditorHostState>,
    requests: State<'_, CopilotRequestState>,
    request_id: String,
) -> Result<Value, String> {
    let completed = requests
        .requests
        .lock()
        .expect("poisoned lock")
        .completed
        .remove(&request_id)
        .ok_or_else(|| "copilot request has not completed".to_owned())?;
    let response = completed.response?;
    state
        .with_host(|host| {
            host.finish_copilot_response_with_tools(
                &completed.original_prompt,
                &response,
                &completed.tool_calls,
                completed.cached_context,
            )
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_copilot_plan(
    requests: State<'_, CopilotRequestState>,
    request_id: String,
) -> Result<(), String> {
    requests
        .requests
        .lock()
        .expect("poisoned lock")
        .cancelled
        .insert(request_id);
    Ok(())
}

#[tauri::command]
fn rpc(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    method: String,
    params: Value,
) -> Result<Value, String> {
    state.with_host(|host| {
        let result = if method == "copilot/plan" {
            let request_id = params
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            host.copilot_plan_streaming(&params, &mut |delta| {
                let delta_payload = match &delta {
                    engine_ai::AiStreamDelta::ToolCallDelta(tc) => {
                        serde_json::to_string(tc).unwrap_or_default()
                    }
                    _ => delta.text().to_owned(),
                };
                let _ = app.emit(
                    "copilot-stream",
                    serde_json::json!({
                        "request_id": request_id,
                        "kind": delta.kind(),
                        "delta": delta_payload,
                    }),
                );
            })
        } else {
            host.handle(&method, &params)
        };
        result.map_err(|error| error.to_string())
    })
}

/// Binary viewport readback — returns raw RGBA pixels as ArrayBuffer.
/// Response layout: [width: u32 LE][height: u32 LE][RGBA pixels...]
#[tauri::command]
fn viewport_readback_raw(
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

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Minimal base64 encoding (no external crate needed for this).
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", d.as_secs(), d.subsec_millis())
}

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
            .map(|p| p.join("aster"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Library/Application Support/aster"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .ok()
            .map(|h| PathBuf::from(h).join("aster"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Some(PathBuf::from(".aster-config"))
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
            .map(|p| p.join("aster"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Library/aster"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA")
            .ok()
            .or_else(|| std::env::var("APPDATA").ok())
            .map(|h| PathBuf::from(h).join("aster"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Some(PathBuf::from(".aster-data"))
    }
}

// ─── App entry point ─────────────────────────────────────────────────────────

#[tauri::command]
fn open_game_view(_app: tauri::AppHandle, state: State<'_, EditorHostState>) -> Result<(), String> {
    state.with_host(|host| {
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
async fn select_project_location() -> Result<Option<String>, String> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Select Project Location")
        .pick_folder()
        .await;

    Ok(folder.map(|f| f.path().to_string_lossy().into_owned()))
}

// ─── Scene file dialogs (cross-platform native dialogs) ─────────────────

#[tauri::command]
async fn open_scene_dialog() -> Result<Option<String>, String> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Scene JSON", &["json", "scene"])
        .pick_file()
        .await;

    Ok(file.map(|f| f.path().to_string_lossy().into_owned()))
}

#[tauri::command]
async fn save_scene_as_dialog() -> Result<Option<String>, String> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Scene JSON", &["json", "scene"])
        .set_file_name("scene.json")
        .save_file()
        .await;

    Ok(file.map(|f| f.path().to_string_lossy().into_owned()))
}

#[tauri::command]
async fn import_asset_dialog() -> Result<Option<String>, String> {
    let file = rfd::AsyncFileDialog::new()
        .set_title("Import Asset")
        .pick_file()
        .await;

    Ok(file.map(|f| f.path().to_string_lossy().into_owned()))
}

fn apply_desktop_window_adaptations(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let desktop = DesktopIntegration::detect();
    if let Some(window) = app.get_webview_window("main") {
        window.set_icon(APP_WINDOW_ICON.clone())?;
        window.set_background_color(Some(Color(24, 24, 24, 255)))?;
        window.set_decorations(desktop.prefers_native_chrome())?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_pre_gtk_desktop_environment() {
    let has_wayland_display = std::env::var("WAYLAND_DISPLAY").is_ok();
    let backend_already_selected = std::env::var("GDK_BACKEND").is_ok();

    if has_wayland_display && !backend_already_selected {
        // Ask GTK/WebKit/Tao to try native Wayland first, while keeping X11 as a
        // fallback for systems where the Wayland backend is unavailable at runtime.
        std::env::set_var("GDK_BACKEND", "wayland,x11");
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_pre_gtk_desktop_environment() {}

pub fn run() {
    // Initialize layered logging: engine / game / editor targets
    // Logs go to: ~/.local/share/aster-editor/logs/ (Linux)
    //             ~/Library/Logs/aster-editor/        (macOS)
    //             %APPDATA%/aster-editor/logs/        (Windows)
    // RUST_LOG=engine=debug,game=info,editor=warn (default: info for all)
    let log_dir = dirs_data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aster-editor")
        .join("logs");
    let file_appender = tracing_appender::rolling::daily(&log_dir, "aster.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // Keep _guard alive for the entire process lifetime so logs are flushed.
    // We intentionally leak it since run() never returns.
    std::mem::forget(_guard);

    use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};
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

    tracing::info!(target: "editor", "logging initialized → {:?}", log_dir);

    apply_pre_gtk_desktop_environment();

    let config_dir = dirs_config_dir().unwrap_or_else(|| PathBuf::from("."));
    let store_path = config_dir.join("aster-editor-state.toml");
    let store = FileEditorStore::new(&store_path);

    let host = match EditorHost::new(store) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("FATAL: failed to initialize editor host: {e}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(EditorHostState::new(host))
        .manage(CopilotRequestState::default())
        .invoke_handler(tauri::generate_handler![
            rpc,
            start_copilot_plan,
            finish_copilot_plan,
            cancel_copilot_plan,
            open_game_view,
            select_project_location,
            viewport_readback_raw,
            open_scene_dialog,
            import_asset_dialog,
            save_scene_as_dialog
        ])
        .setup(|app| {
            apply_desktop_window_adaptations(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        asset_meta_path_for_source, copilot_execution_summary, extract_codex_account_id,
        model_detection_config, normalize_relative_path, resolve_existing_relative_path,
        resolve_writable_relative_path, should_continue_copilot, DesktopEnvironment, EditorHost,
    };
    use base64::Engine as _;
    use engine_editor::{CopilotProvider, FileEditorStore};
    use std::fs;
    use std::path::Path;

    #[test]
    fn kde_uses_native_chrome_when_native_wayland_is_preferred() {
        assert!(DesktopEnvironment::Kde.prefers_native_chrome_for_backend(true));
    }

    #[test]
    fn kde_keeps_native_chrome_when_using_x11_backend() {
        assert!(DesktopEnvironment::Kde.prefers_native_chrome_for_backend(false));
    }

    #[test]
    fn relative_paths_reject_parent_traversal() {
        assert!(normalize_relative_path("../../outside.txt").is_err());
        assert!(normalize_relative_path("/tmp/outside.txt").is_err());
    }

    #[test]
    fn asset_meta_paths_append_meta_to_full_file_name() {
        assert_eq!(
            asset_meta_path_for_source(Path::new("textures/player.png")),
            Path::new("textures/player.png.meta")
        );
    }

    #[test]
    fn existing_asset_paths_resolve_under_asset_root() {
        let temp = tempfile::tempdir().unwrap();
        let asset_root = temp.path().join("assets");
        let script_path = asset_root.join("scripts/player.rhai");
        fs::create_dir_all(script_path.parent().unwrap()).unwrap();
        fs::write(&script_path, "fn on_start() {}").unwrap();

        let resolved = resolve_existing_relative_path(&asset_root, "scripts/player.rhai").unwrap();

        assert_eq!(resolved, script_path.canonicalize().unwrap());
    }

    #[test]
    fn writable_asset_paths_reject_new_file_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let asset_root = temp.path().join("assets");
        fs::create_dir_all(&asset_root).unwrap();

        assert!(resolve_writable_relative_path(&asset_root, "../../outside.txt").is_err());
    }

    #[test]
    fn writable_asset_paths_allow_new_nested_files_inside_asset_root() {
        let temp = tempfile::tempdir().unwrap();
        let asset_root = temp.path().join("assets");
        fs::create_dir_all(&asset_root).unwrap();

        let resolved =
            resolve_writable_relative_path(&asset_root, "scripts/new_script.rhai").unwrap();

        assert_eq!(
            resolved,
            asset_root
                .canonicalize()
                .unwrap()
                .join("scripts/new_script.rhai")
        );
    }

    #[test]
    fn codex_account_id_is_extracted_from_namespaced_jwt_claim() {
        let claims = serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "account-123"
            }
        });
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).unwrap());
        let token = format!("header.{payload}.signature");

        assert_eq!(
            extract_codex_account_id(&token).as_deref(),
            Some("account-123")
        );
    }

    #[test]
    fn model_detection_uses_saved_credentials_when_request_omits_them() {
        let settings = engine_editor::CopilotSettings {
            provider: CopilotProvider::Custom,
            api_key: Some("saved-key".to_owned()),
            api_endpoint: Some("https://provider.example/v1".to_owned()),
            ..Default::default()
        };

        let config = model_detection_config(
            &serde_json::json!({}),
            &settings,
            &engine_ai::registry::ProviderKind::Custom,
        );

        assert_eq!(config.api_key.as_deref(), Some("saved-key"));
        assert_eq!(
            config.endpoint.as_deref(),
            Some("https://provider.example/v1")
        );
    }

    #[test]
    fn model_detection_ignores_saved_endpoint_for_fixed_provider() {
        let settings = engine_editor::CopilotSettings {
            provider: CopilotProvider::OpenAI,
            api_endpoint: Some("https://provider.example/v1".to_owned()),
            ..Default::default()
        };

        let config = model_detection_config(
            &serde_json::json!({}),
            &settings,
            &engine_ai::registry::ProviderKind::OpenAI,
        );

        assert_eq!(config.endpoint, None);
    }

    #[test]
    fn model_detection_resolves_mimo_and_glm_configured_endpoints() {
        let mimo_settings = engine_editor::CopilotSettings {
            provider: CopilotProvider::Mimo,
            mimo_config: engine_editor::MimoConfig {
                billing: engine_editor::BillingMode::Subscription,
                region: engine_editor::MimoRegion::Singapore,
            },
            ..Default::default()
        };
        let mimo = model_detection_config(
            &serde_json::json!({}),
            &mimo_settings,
            &engine_ai::registry::ProviderKind::Mimo,
        );
        assert_eq!(
            mimo.endpoint.as_deref(),
            Some("https://token-plan-sgp.xiaomimimo.com/v1")
        );

        let glm_settings = engine_editor::CopilotSettings {
            provider: CopilotProvider::Glm,
            glm_config: engine_editor::GlmConfig {
                billing: engine_editor::BillingMode::Subscription,
                region: engine_editor::GlmRegion::Zai,
            },
            ..Default::default()
        };
        let glm = model_detection_config(
            &serde_json::json!({}),
            &glm_settings,
            &engine_ai::registry::ProviderKind::Glm,
        );
        assert_eq!(
            glm.endpoint.as_deref(),
            Some("https://api.z.ai/api/coding/paas/v4")
        );
    }

    #[test]
    fn fixed_provider_clears_custom_endpoint_when_settings_are_updated() {
        let temp = tempfile::tempdir().unwrap();
        let store = FileEditorStore::new(temp.path().join("aster-editor-state.toml"));
        let mut host = EditorHost::new(store).unwrap();

        host.update_copilot_settings(&serde_json::json!({
            "provider": "openai",
            "model": "gpt-4.1",
            "api_endpoint": "https://provider.example/v1",
            "max_tokens": 4096
        }))
        .unwrap();

        assert_eq!(host.copilot_settings.api_endpoint, None);
    }

    #[test]
    fn ollama_preserves_custom_endpoint_when_settings_are_updated() {
        let temp = tempfile::tempdir().unwrap();
        let store = FileEditorStore::new(temp.path().join("aster-editor-state.toml"));
        let mut host = EditorHost::new(store).unwrap();

        host.update_copilot_settings(&serde_json::json!({
            "provider": "ollama",
            "model": "qwen3",
            "api_endpoint": "http://192.168.1.20:11434",
            "max_tokens": 4096
        }))
        .unwrap();

        assert_eq!(
            host.copilot_settings.api_endpoint.as_deref(),
            Some("http://192.168.1.20:11434")
        );
    }

    #[test]
    fn copilot_settings_survive_host_restart() {
        let temp = tempfile::tempdir().unwrap();
        let state_path = temp.path().join("aster-editor-state.toml");

        {
            let store = FileEditorStore::new(&state_path);
            let mut host = EditorHost::new(store).unwrap();
            host.update_copilot_settings(&serde_json::json!({
                "provider": "custom",
                "model": "aster-test-model",
                "api_endpoint": "https://provider.example/v1",
                "api_key": "secret-test-key",
                "max_tokens": 8192
            }))
            .unwrap();
        }

        let state_text = fs::read_to_string(&state_path).unwrap();
        assert!(state_text.contains("aster-test-model"));
        assert!(!state_text.contains("secret-test-key"));

        let store = FileEditorStore::new(&state_path);
        let host = EditorHost::new(store).unwrap();
        assert_eq!(host.copilot_settings.provider, CopilotProvider::Custom);
        assert_eq!(host.copilot_settings.model, "aster-test-model");
        assert_eq!(
            host.copilot_settings.api_endpoint.as_deref(),
            Some("https://provider.example/v1")
        );
        assert_eq!(host.copilot_settings.max_tokens, 8192);
        assert_eq!(
            host.copilot_settings.api_key.as_deref(),
            Some("secret-test-key")
        );
    }

    #[test]
    fn permanently_allowed_command_survives_host_restart() {
        let temp = tempfile::tempdir().unwrap();
        let state_path = temp.path().join("aster-editor-state.toml");

        {
            let store = FileEditorStore::new(&state_path);
            let mut host = EditorHost::new(store).unwrap();
            host.copilot_allow_command(&serde_json::json!({
                "command": "gameobject.create_empty"
            }))
            .unwrap();
        }

        let store = FileEditorStore::new(&state_path);
        let host = EditorHost::new(store).unwrap();
        assert_eq!(
            host.copilot_settings.allowed_commands,
            vec!["gameobject.create_empty"]
        );
    }

    #[test]
    fn read_only_inspection_requests_an_agent_continuation() {
        assert!(should_continue_copilot(true, false));
        assert!(!should_continue_copilot(false, false));
        assert!(!should_continue_copilot(true, true));
    }

    #[test]
    fn copilot_execution_summary_includes_tool_results() {
        let summary = copilot_execution_summary(
            1,
            None,
            &["query_scene_semantic: applied".to_owned()],
            &["Found 3 entities:\n0:1 - Camera\n1:1 - Player".to_owned()],
        );

        assert!(summary.contains("query_scene_semantic: applied"));
        assert!(summary.contains("Tool results:"));
        assert!(summary.contains("1:1 - Player"));
    }
}
