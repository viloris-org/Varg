//! Tauri backend for the Varg Editor.
//!
//! Single `rpc` command that dispatches to EditorHost methods,
//! mirroring the original stdin/stdout JSON-RPC protocol.

use std::{
    collections::{BTreeMap, HashMap},
    hash::{DefaultHasher, Hash, Hasher},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::Engine as _;
use engine_ai::{AgentOutcome, AgentPlan, AgentSession};
use engine_core::{EngineConfig, EngineError, EngineResult, RuntimeProfile};
use engine_editor::agent::{PermissionPolicy, TraceEntry};
use engine_editor::{
    ConsoleEntry, ConsoleLevel, ConsoleService, DurableEditorState, EditorPreferences,
    FileEditorStore, ProjectMetadata, ThemePreference, UndoCommand,
};
use engine_editor::{EditorShell, HubState, ProjectDeletionDecision, ProjectDeletionMode};
use engine_i18n::{Locale, Translations};
use engine_packager::{
    PackageChannel, PackageFormat, PackageRequest, PackageTarget, package_project,
};
use engine_quest::{
    GeneratedQuestSpec, GeneratedQuestionCard, collect_workspace_snapshot,
    diff_workspace_snapshots, parse_generated_quest_response, prepare_isolated_workspace,
    quest_creation_tool_definitions, quest_validation_command_specs, restore_rollback_files,
    snapshot_rollback_file, validate_quest_workspace,
};
use engine_render::ImageFormat;
use engine_render_wgpu::{WgpuOffscreenConfig, WgpuRenderDevice};
use runtime_min::{RuntimeServices, headless_services_from_scene};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::Manager;

mod app;
mod commands;
mod editor_compositor;
mod game_window;
mod host;
mod native_host_window;
mod native_panel_host;
mod quest;
mod scene_window;
mod state;
mod wayland_embedded_compositor;

#[cfg(test)]
fn claim_native_event_loop_test_slot(test_name: &str) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};

    static CLAIMED: AtomicBool = AtomicBool::new(false);
    if CLAIMED.swap(true, Ordering::SeqCst) {
        eprintln!("skipping {test_name}: native event loop test slot is already in use");
        return false;
    }
    true
}

use quest::{
    QuestExplorationAttempt, QuestMode, QuestModelConfig, QuestProject, QuestReview,
    QuestReviewAction, QuestReviewFinding, QuestReviewMetrics, QuestStatus, QuestStore, QuestTask,
    ValidationResult, transaction_groups_from_changed_files,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CopilotApprovalMode {
    Ask,
    AutoSafe,
    FullAccess,
}

impl CopilotApprovalMode {
    fn from_params(params: &Value) -> Self {
        match params.get("approval_mode").and_then(Value::as_str) {
            Some("auto-safe") => Self::AutoSafe,
            Some("full-access") => Self::FullAccess,
            _ => Self::Ask,
        }
    }

    fn planning_policy(self) -> PermissionPolicy {
        PermissionPolicy::full_access()
    }

    fn apply_policy(self) -> PermissionPolicy {
        match self {
            Self::FullAccess => PermissionPolicy::full_access(),
            Self::Ask | Self::AutoSafe => PermissionPolicy::transactional_write(),
        }
    }

    fn auto_approves_write(self) -> bool {
        matches!(self, Self::AutoSafe | Self::FullAccess)
    }

    fn auto_approves_command(self) -> bool {
        matches!(self, Self::FullAccess)
    }
}

const WINDOW_BACKGROUND: &str = "#181818";
const SOLO_REPAIR_LIMIT: usize = 1;

fn editor_compositor_requested() -> bool {
    match std::env::var("ASTER_EDITOR_COMPOSITOR") {
        Ok(value) => match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => false,
        },
        Err(_) => default_editor_compositor_requested(),
    }
}

fn default_editor_compositor_requested() -> bool {
    let support = editor_compositor::platform_support();
    let wayland_support = wayland_embedded_compositor::support();
    default_editor_compositor_requested_for(support, wayland_support)
}

fn default_editor_compositor_requested_for(
    support: editor_compositor::EditorCompositorSupport,
    _wayland_support: wayland_embedded_compositor::WaylandEmbeddedCompositorSupport,
) -> bool {
    support.available
}

fn main_window_editor_compositor_support(
    app: &tauri::AppHandle,
) -> editor_compositor::EditorCompositorSupport {
    use raw_window_handle::HasWindowHandle;

    let Some(window) = app.get_window("main") else {
        return editor_compositor::platform_support();
    };
    let Ok(handle) = window.window_handle() else {
        return editor_compositor::platform_support();
    };
    editor_compositor::platform_support_for_window_handle(handle.as_raw())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuestApplyDecision {
    AutoApply,
    NeedsReview,
    Blocked,
}

struct QuestApplyPolicy;

impl QuestApplyPolicy {
    fn classify(review: &QuestReview, autonomy: &quest::QuestAutonomyPolicy) -> QuestApplyDecision {
        let has_failed_validation = review
            .validations
            .iter()
            .any(|validation| validation.status != "passed" && validation.status != "skipped");
        if has_failed_validation || !review.unresolved_issues.is_empty() {
            return QuestApplyDecision::Blocked;
        }
        if review.risk == "low"
            && !review.changed_files.is_empty()
            && !autonomy.active_project_apply_requires_approval
        {
            return QuestApplyDecision::AutoApply;
        }
        QuestApplyDecision::NeedsReview
    }

    fn as_str(decision: QuestApplyDecision) -> &'static str {
        match decision {
            QuestApplyDecision::AutoApply => "auto_apply",
            QuestApplyDecision::NeedsReview => "needs_review",
            QuestApplyDecision::Blocked => "blocked",
        }
    }
}

struct AgentCommandPolicy;

impl AgentCommandPolicy {
    fn validation_registry_summary() -> Vec<Value> {
        quest_validation_command_specs()
            .into_iter()
            .map(|command| {
                serde_json::json!({
                    "id": command.id,
                    "program": command.program,
                    "args": command.args,
                    "destructive": false,
                    "sandbox": "workspace",
                    "approval": "allowlisted_registry",
                })
            })
            .collect()
    }
}

struct SoloQuestRunner;

impl SoloQuestRunner {
    const REPAIR_LIMIT: usize = SOLO_REPAIR_LIMIT;

    fn initial_prompt(spec: &str, knowledge_context: &str) -> String {
        format!(
            "Execute this Quest inside the isolated workspace only. \
             Produce concrete Varg editor operations. Do not request shell commands. \
             When done, emit a complete operation with a concise summary.\n\n{}{}",
            spec, knowledge_context
        )
    }

    fn repair_prompt(spec: &str, validations: &[ValidationResult], attempt: usize) -> String {
        let failures = validation_failure_summaries(validations).join("\n\n");
        format!(
            "Continue this Solo Quest inside the isolated workspace only. \
             Repair the validation failures below without broad unrelated changes. \
             Do not request shell commands. Emit a complete operation when finished.\n\n\
             Repair attempt: {attempt}/{}\n\n\
             Quest spec:\n{spec}\n\n\
             Validation failures:\n{failures}",
            Self::REPAIR_LIMIT
        )
    }
}

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

fn required_string<'a>(params: &'a Value, key: &str) -> EngineResult<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::config(format!("missing '{key}' parameter")))
}

fn copilot_provider_str(provider: &engine_editor::CopilotProvider) -> EngineResult<&'static str> {
    match provider {
        engine_editor::CopilotProvider::Anthropic => Ok("anthropic"),
        engine_editor::CopilotProvider::Ollama => Ok("ollama"),
        engine_editor::CopilotProvider::OpenAI => Ok("openai"),
        engine_editor::CopilotProvider::CodexOAuth => Ok("codex_oauth"),
        engine_editor::CopilotProvider::Gemini => Ok("gemini"),
        engine_editor::CopilotProvider::Custom => Ok("custom"),
        engine_editor::CopilotProvider::Mimo => Ok("mimo"),
        engine_editor::CopilotProvider::DeepSeek => Ok("deepseek"),
        engine_editor::CopilotProvider::Glm => Ok("glm"),
        engine_editor::CopilotProvider::Stub => Err(EngineError::config(
            "Quest execution requires a configured AI provider.",
        )),
    }
}

fn parse_quest_mode(value: Option<&Value>) -> EngineResult<QuestMode> {
    match value.and_then(Value::as_str).unwrap_or("solo") {
        "solo" => Ok(QuestMode::Solo),
        "extra" => Ok(QuestMode::Extra),
        other => Err(EngineError::config(format!("unknown Quest mode: {other}"))),
    }
}

fn parse_thinking_effort(value: &str) -> Option<engine_ai::ThinkingEffort> {
    match value {
        "off" => Some(engine_ai::ThinkingEffort::Off),
        "low" => Some(engine_ai::ThinkingEffort::Low),
        "medium" => Some(engine_ai::ThinkingEffort::Medium),
        "high" => Some(engine_ai::ThinkingEffort::High),
        _ => None,
    }
}

fn parse_locale(value: Option<&str>) -> Locale {
    match value {
        Some("zh") => Locale::Zh,
        Some("ja") => Locale::Ja,
        Some("ko") => Locale::Ko,
        Some("es") => Locale::Es,
        Some("zh_hant") => Locale::ZhHant,
        _ => Locale::En,
    }
}

fn locale_code(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "en",
        Locale::Zh => "zh",
        Locale::Ja => "ja",
        Locale::Ko => "ko",
        Locale::Es => "es",
        Locale::ZhHant => "zh_hant",
    }
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

fn format_varg_diagnostics(
    path: &str,
    diagnostics: &[engine_script_varg::VargDiagnostic],
) -> String {
    let details = diagnostics
        .iter()
        .map(|diagnostic| {
            let location = match (diagnostic.line, diagnostic.column) {
                (Some(line), Some(column)) => format!("{path}:{line}:{column}"),
                (Some(line), None) => format!("{path}:{line}"),
                _ => path.to_owned(),
            };
            format!(
                "{} {}: {} Expected: {} Suggestion: {}",
                diagnostic.code,
                location,
                diagnostic.message,
                diagnostic.expected,
                diagnostic.suggestion
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Varg validation failed with {} diagnostic(s):\n{details}",
        diagnostics.len()
    )
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
            "editor_compositor_requested": editor_compositor_requested(),
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
    state: String,
    code_verifier: String,
    listener: TcpListener,
}

pub(crate) struct PreparedCopilotRequest {
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
    knowledge_entries_used: usize,
    approval_mode: CopilotApprovalMode,
}

pub(crate) struct CompletedCopilotRequest {
    original_prompt: String,
    response: Result<String, String>,
    tool_calls: Vec<engine_ai::ToolCall>,
    cached_context: engine_editor::ProjectContext,
    knowledge_entries_used: usize,
    approval_mode: CopilotApprovalMode,
}

pub(crate) struct PreparedQuestModelRequest {
    request: engine_ai::AiRequest,
    provider: String,
    model: String,
    api_key: Option<String>,
    endpoint: Option<String>,
    max_tokens: u32,
    codex_oauth: Option<engine_ai::providers::CodexOAuthCredentials>,
    mimo_config: Option<engine_editor::MimoConfig>,
    glm_config: Option<engine_editor::GlmConfig>,
}

pub(crate) struct PreparedQuestCreateRequest {
    model_request: PreparedQuestModelRequest,
    title: String,
    goal: String,
    project: QuestProject,
    mode: QuestMode,
    model_config: QuestModelConfig,
}

pub(crate) enum PreparedQuestAiRequest {
    Create(PreparedQuestCreateRequest),
    Rewrite(PreparedQuestModelRequest),
}

pub(crate) enum CompletedQuestAiRequest {
    Create {
        generated: Result<GeneratedQuestSpec, String>,
        title: String,
        goal: String,
        project: QuestProject,
        mode: QuestMode,
        model_config: QuestModelConfig,
    },
    Rewrite(Result<String, String>),
}

pub(crate) struct PreparedQuestExecution {
    quest_store: QuestStore,
    quest_id: String,
    model_provider: PreparedQuestModelRequest,
}

#[derive(Default)]
pub(crate) struct QuestExecutionRequests {
    completed: HashMap<String, Result<Value, String>>,
    cancelled: std::collections::HashSet<String>,
}

#[derive(Clone, Default)]
pub(crate) struct QuestExecutionRequestState {
    requests: std::sync::Arc<Mutex<QuestExecutionRequests>>,
}

#[derive(Default)]
pub(crate) struct QuestAiRequests {
    completed: HashMap<String, CompletedQuestAiRequest>,
    cancelled: std::collections::HashSet<String>,
}

#[derive(Clone, Default)]
pub(crate) struct QuestAiRequestState {
    requests: std::sync::Arc<Mutex<QuestAiRequests>>,
}

#[derive(Default)]
pub(crate) struct CopilotRequests {
    completed: HashMap<String, CompletedCopilotRequest>,
    cancelled: std::collections::HashSet<String>,
}

#[derive(Clone, Default)]
pub(crate) struct CopilotRequestState {
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
const CODEX_OAUTH_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const CODEX_OAUTH_CALLBACK_BIND: &str = "127.0.0.1:1455";
const CODEX_OAUTH_SCOPE: &str = "openid profile email offline_access";

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn elapsed_millis(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis() as u64
}

fn random_urlsafe_string(len: usize) -> EngineResult<String> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut bytes = vec![0_u8; len];
    getrandom::fill(&mut bytes)
        .map_err(|error| EngineError::other(format!("secure random generation failed: {error}")))?;
    Ok(bytes
        .into_iter()
        .map(|byte| CHARS[(byte as usize) % CHARS.len()] as char)
        .collect())
}

fn random_hex_string(bytes_len: usize) -> EngineResult<String> {
    let mut bytes = vec![0_u8; bytes_len];
    getrandom::fill(&mut bytes)
        .map_err(|error| EngineError::other(format!("secure random generation failed: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn codex_pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn codex_authorize_url(code_challenge: &str, state: &str) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", CODEX_OAUTH_CLIENT_ID),
        ("redirect_uri", CODEX_OAUTH_REDIRECT_URI),
        ("scope", CODEX_OAUTH_SCOPE),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "codex_cli_rs"),
    ];
    let query = params
        .into_iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{CODEX_OAUTH_ISSUER}/oauth/authorize?{query}")
}

fn read_codex_oauth_callback(
    stream: &mut TcpStream,
    expected_state: &str,
) -> EngineResult<Option<String>> {
    let mut buffer = [0_u8; 8192];
    let read = stream
        .read(&mut buffer)
        .map_err(|error| EngineError::other(format!("failed to read OAuth callback: {error}")))?;
    if read == 0 {
        return Ok(None);
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let Some(request_line) = request.lines().next() else {
        return Ok(None);
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" {
        write_oauth_callback_response(stream, 405, "Method Not Allowed", "Method not allowed")?;
        return Ok(None);
    }
    let Some((path, query)) = target.split_once('?') else {
        write_oauth_callback_response(stream, 404, "Not Found", "Not found")?;
        return Ok(None);
    };
    if path != "/auth/callback" {
        write_oauth_callback_response(stream, 404, "Not Found", "Not found")?;
        return Ok(None);
    }

    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_description = None;
    for (key, value) in query.split('&').filter_map(|pair| pair.split_once('=')) {
        let key = urlencoding::decode(key)
            .map_err(|error| EngineError::other(format!("invalid OAuth callback query: {error}")))?
            .into_owned();
        let value = urlencoding::decode(value)
            .map_err(|error| EngineError::other(format!("invalid OAuth callback query: {error}")))?
            .into_owned();
        match key.as_str() {
            "code" => code = Some(value.to_owned()),
            "state" => state = Some(value.to_owned()),
            "error" => error = Some(value.to_owned()),
            "error_description" => error_description = Some(value.to_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        let message = error_description.unwrap_or(error);
        write_oauth_callback_response(stream, 400, "Bad Request", &message)?;
        return Err(EngineError::other(format!(
            "Codex authorization failed: {message}"
        )));
    }
    if state.as_deref() != Some(expected_state) {
        write_oauth_callback_response(stream, 400, "Bad Request", "State mismatch")?;
        return Err(EngineError::other(
            "Codex authorization failed because the OAuth state did not match",
        ));
    }
    let code = code.ok_or_else(|| EngineError::other("Codex authorization omitted code"))?;
    write_oauth_callback_response(
        stream,
        200,
        "OK",
        "Authorization successful. You can close this window and return to Varg.",
    )?;
    Ok(Some(code))
}

fn write_oauth_callback_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    message: &str,
) -> EngineResult<()> {
    let body = format!(
        "<!doctype html><html><head><title>Varg Codex OAuth</title></head><body><h1>{}</h1><p>{}</p></body></html>",
        reason, message
    );
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(|error| EngineError::other(format!("failed to write OAuth callback: {error}")))
}

fn context_relevance_score(detail: &quest::QuestDetail) -> f32 {
    let mut score: f32 = 0.45;
    if !detail.intent.trim().is_empty() {
        score += 0.15;
    }
    if !detail.spec.trim().is_empty() {
        score += 0.2;
    }
    if !detail.attached_knowledge.is_empty() {
        score += 0.1;
    }
    if !detail.events.is_empty() {
        score += 0.1;
    }
    score.min(1.0)
}

fn failed_action_recovery_rate(trace_entries: &[TraceEntry]) -> f32 {
    let failed = trace_entries
        .iter()
        .filter(|entry| entry.result != "applied")
        .count();
    if failed == 0 {
        return 1.0;
    }
    let recoverable = trace_entries
        .iter()
        .filter(|entry| entry.result != "applied" && !entry.recovery_hint.trim().is_empty())
        .count();
    recoverable as f32 / failed as f32
}

fn review_evidence_quality_score(
    has_changes: bool,
    validations: &[ValidationResult],
    has_failed_validation: bool,
) -> f32 {
    let mut score: f32 = 0.25;
    if has_changes {
        score += 0.25;
    }
    if !validations.is_empty() {
        score += 0.25;
    }
    if validations
        .iter()
        .any(|validation| validation.status == "passed")
    {
        score += 0.15;
    }
    if has_failed_validation {
        score -= 0.15;
    }
    score.clamp(0.0, 1.0)
}

fn quest_review_actions_for_result(
    unresolved_issues: &[String],
    has_failed_validation: bool,
    has_no_changes: bool,
) -> Vec<QuestReviewAction> {
    let mut actions = Vec::new();
    if has_failed_validation {
        for (index, issue) in unresolved_issues.iter().enumerate() {
            actions.push(QuestReviewAction::with_target(
                format!("quick-fix-{index}"),
                "Request quick fix",
                "quick_fix",
                issue.clone(),
            ));
        }
        actions.push(QuestReviewAction::new(
            "revise-spec",
            "Revise Quest spec",
            "revise",
        ));
        actions.push(QuestReviewAction::new(
            "retry-validation",
            "Retry execution",
            "retry",
        ));
        actions.push(QuestReviewAction::new(
            "continue-quest",
            "Continue Quest",
            "continue",
        ));
    } else if has_no_changes {
        actions.push(QuestReviewAction::with_target(
            "inspect-no-changes",
            "Inspect no-change finding",
            "open_review_finding",
            unresolved_issues
                .first()
                .cloned()
                .unwrap_or_else(|| "Quest produced no file changes".to_owned()),
        ));
        actions.push(QuestReviewAction::new(
            "revise-spec",
            "Revise Quest spec",
            "revise",
        ));
        actions.push(QuestReviewAction::new(
            "continue-quest",
            "Continue Quest",
            "continue",
        ));
        actions.push(QuestReviewAction::new(
            "archive-quest",
            "Archive Quest",
            "archive",
        ));
    } else {
        actions.push(QuestReviewAction::new(
            "apply-selected",
            "Apply selected changes",
            "apply_selected",
        ));
        actions.push(QuestReviewAction::new(
            "request-revision",
            "Request revision",
            "revise",
        ));
        actions.push(QuestReviewAction::new(
            "branch-result",
            "Branch from result",
            "branch",
        ));
        actions.push(QuestReviewAction::new(
            "continue-quest",
            "Continue Quest",
            "continue",
        ));
        actions.push(QuestReviewAction::new(
            "archive-quest",
            "Archive Quest",
            "archive",
        ));
        actions.push(QuestReviewAction::new(
            "discard-selected",
            "Discard selected changes",
            "discard_selected",
        ));
    }
    actions
}

fn validations_failed(validations: &[ValidationResult]) -> bool {
    validations
        .iter()
        .any(|validation| validation.status != "passed" && validation.status != "skipped")
}

fn validation_failure_summaries(validations: &[ValidationResult]) -> Vec<String> {
    validations
        .iter()
        .filter(|validation| validation.status == "failed")
        .map(|validation| format!("{}: {}", validation.name, validation.summary))
        .collect()
}

fn append_validation_events(
    quest_store: &QuestStore,
    id: &str,
    validations: &[ValidationResult],
    repair_attempt: Option<usize>,
) -> EngineResult<()> {
    for validation in validations {
        let summary = match repair_attempt {
            Some(0) => format!("baseline {}: {}", validation.name, validation.status),
            Some(attempt) => format!(
                "repair attempt {attempt} {}: {}",
                validation.name, validation.status
            ),
            None => format!("{}: {}", validation.name, validation.status),
        };
        quest_store.append_timeline_event(
            id,
            "validation",
            &summary,
            serde_json::json!({
                "repair_attempt": repair_attempt.filter(|attempt| *attempt > 0),
                "attempt_id": if repair_attempt == Some(0) { Some("baseline") } else { None },
                "name": validation.name,
                "status": validation.status,
                "summary": validation.summary,
                "policy_registry": AgentCommandPolicy::validation_registry_summary(),
            }),
        )?;
    }
    Ok(())
}

fn merge_agent_outcome(outcome: &mut AgentOutcome, repair_outcome: AgentOutcome) {
    outcome.operations_performed += repair_outcome.operations_performed;
    outcome
        .console_entries
        .extend(repair_outcome.console_entries);
    outcome.trace_entries = repair_outcome.trace_entries;
    outcome.completed = repair_outcome.completed;
    if repair_outcome.summary.is_some() {
        outcome.summary = repair_outcome.summary;
    }
}

fn selected_review_paths_from_params(
    review: &QuestReview,
    params: &Value,
    selection_label: &str,
) -> EngineResult<Vec<String>> {
    let selected_groups = params
        .get("transaction_group_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    let selected_files = params.get("files").and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    let changed_files = match (selected_groups, selected_files) {
        (Some(groups), _) if groups.is_empty() => {
            return Err(EngineError::config(format!(
                "at least one Quest transaction group must be selected to {selection_label}"
            )));
        }
        (Some(groups), _) => {
            let groups_by_id = review
                .transaction_groups
                .iter()
                .map(|group| (group.id.as_str(), group))
                .collect::<HashMap<_, _>>();
            let mut files = Vec::new();
            for group_id in &groups {
                let group = groups_by_id.get(group_id.as_str()).ok_or_else(|| {
                    EngineError::config(format!(
                        "selected Quest transaction group is not present in the review bundle: {group_id}"
                    ))
                })?;
                files.extend(group.files.iter().cloned());
            }
            files.sort();
            files.dedup();
            files
        }
        (_, Some(files)) if files.is_empty() => {
            return Err(EngineError::config(format!(
                "at least one Quest file must be selected to {selection_label}"
            )));
        }
        (_, Some(files)) => files,
        (None, None) => review
            .changed_files
            .iter()
            .map(|file| file.path.clone())
            .collect(),
    };
    let review_paths = review
        .changed_files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<std::collections::HashSet<_>>();
    if changed_files
        .iter()
        .any(|path| !review_paths.contains(path.as_str()))
    {
        return Err(EngineError::config(
            "selected Quest file is not present in the review bundle",
        ));
    }
    Ok(changed_files)
}

fn project_fingerprint(root: &Path) -> EngineResult<String> {
    let snapshot = collect_workspace_snapshot(root)?;
    Ok(snapshot_fingerprint(&snapshot))
}

fn snapshot_fingerprint(snapshot: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hasher = DefaultHasher::new();
    for (path, bytes) in snapshot {
        path.hash(&mut hasher);
        bytes.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn ensure_review_project_is_current(review: &QuestReview, project_root: &Path) -> EngineResult<()> {
    let Some(expected) = review.project_fingerprint.as_deref() else {
        return Ok(());
    };
    let current = project_fingerprint(project_root)?;
    if current != expected {
        return Err(EngineError::config(
            "Quest review is stale because the active project changed after the isolated workspace snapshot. Re-run or revise the Quest before applying or discarding this review.",
        ));
    }
    Ok(())
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
    /// Active browser authorization request.
    pending_codex_oauth: Option<PendingCodexOAuth>,
    /// Active copilot conversation history for multi-turn dialogue.
    copilot_conversation: Vec<engine_ai::ChatMessage>,
    /// Native game window handle (direct GPU surface rendering).
    game_window: Option<game_window::GameWindowHandle>,
    /// Native editor scene view handle (direct GPU surface rendering).
    scene_window: Option<scene_window::SceneWindowHandle>,
    /// Full-window editor compositor seam for the future no-CPU-readback viewport.
    editor_compositor: editor_compositor::EditorCompositor,
    /// Host-owned native editor layout state used by native Scene View presentation.
    native_host_layout: native_host_window::NativeHostLayoutState,
    /// Experimental split WebView panels hosted by the native editor window.
    native_panel_host: native_panel_host::NativePanelHost,
    /// Wayland production no-CPU-readback presentation seam backed by an embedded compositor.
    wayland_embedded_compositor: wayland_embedded_compositor::WaylandEmbeddedCompositor,
    /// Cross-project Quest registry and append-only history store.
    quest_store: QuestStore,
}

/// Maximum number of messages to keep in the copilot conversation.
/// Older messages are evicted in pairs (user+assistant) to maintain context coherence.
const MAX_COPILOT_CONVERSATION_MESSAGES: usize = 40;

impl EditorHost {
    pub fn new(store: FileEditorStore) -> EngineResult<Self> {
        let quest_root = store
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("quests");
        Self::new_with_quest_root(store, quest_root)
    }

    pub fn new_with_quest_root(
        store: FileEditorStore,
        quest_root: impl Into<PathBuf>,
    ) -> EngineResult<Self> {
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
            scene_window: None,
            editor_compositor: editor_compositor::EditorCompositor::default(),
            native_host_layout: native_host_window::NativeHostLayoutState::default(),
            native_panel_host: native_panel_host::NativePanelHost::default(),
            wayland_embedded_compositor:
                wayland_embedded_compositor::WaylandEmbeddedCompositor::default(),
            quest_store: QuestStore::new(quest_root),
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
            "project/create_material" => self.project_create_material(params),
            "project/create_prefab" => self.project_create_prefab(params),
            "project/create_scene" => self.project_create_scene(params),
            "project/list_asset_references" => self.project_list_asset_references(params),
            "project/rename_asset" => self.project_rename_asset(params),
            "project/delete_asset" => self.project_delete_asset(params),
            "project/reimport_asset" => self.project_reimport_asset(params),
            "project/read_file" => self.project_read_file(params),
            "project/check_amdl" => self.project_check_amdl(params),
            "project/check_script" => self.project_check_script(params),
            "project/write_file" => self.project_write_file(params),
            "project/package" => self.project_package(params),

            // ── Quests ──
            "quest/list" => self.quest_list(params),
            "quest/get" => self.quest_get(params),
            "quest/read_artifact" => self.quest_read_artifact(params),
            "quest/create" => Err(EngineError::config(
                "Quest creation must use the background Quest AI command",
            )),
            "quest/create_openai_realtime_transcription_session" => {
                self.quest_create_openai_realtime_transcription_session(params)
            }
            "quest/rewrite_prompt" => Err(EngineError::config(
                "Quest prompt rewriting must use the background Quest AI command",
            )),
            "quest/promote" => self.quest_promote(params),
            "quest/update_intent" => self.quest_update_intent(params),
            "quest/update_spec" => self.quest_update_spec(params),
            "quest/update_tasks" => self.quest_update_tasks(params),
            "quest/update_execution_config" => self.quest_update_execution_config(params),
            "quest/update_knowledge_context" => self.quest_update_knowledge_context(params),
            "quest/add_note" => self.quest_add_note(params),
            "quest/request_quick_fix" => self.quest_request_quick_fix(params),
            "quest/rename" => self.quest_rename(params),
            "quest/branch" => self.quest_branch(params),
            "quest/transition" => self.quest_transition(params),
            "quest/delete" => self.quest_delete(params),
            "quest/execute" => self.quest_execute(params),
            "quest/apply" => self.quest_apply(params),
            "quest/discard" => self.quest_discard(params),
            "quest/rollback" => self.quest_rollback(params),
            "quest/export" => self.quest_export(params),
            "quest/cancel" => self.quest_cancel(params),
            "quest/reopen" => self.quest_reopen(params),
            "quest/continue" => self.quest_continue(params),
            "quest/reject" => self.quest_reject(params),
            "quest/request_revision" => self.quest_request_revision(params),
            "quest/mock_execute" => self.quest_mock_execute(params),
            "knowledge/list" => self.knowledge_list(params),
            "knowledge/propose" => self.knowledge_propose(params),
            "knowledge/approve" => self.knowledge_approve(params),
            "knowledge/reject" => self.knowledge_reject(params),
            "knowledge/revalidate" => self.knowledge_revalidate(params),
            "knowledge/remove" => self.knowledge_remove(params),

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
            "copilot/undo_last" => self.copilot_undo_last(params),
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
}

pub(crate) fn record_quest_execution_failure(
    quest_store: &QuestStore,
    id: &str,
    started_at: Instant,
    error: EngineError,
) -> EngineResult<Value> {
    let reason = error.to_string();
    let findings = match quest_store.write_review_finding(
        id,
        "execution-failed",
        "Quest execution failed",
        &reason,
        "high",
        Some("execution"),
    ) {
        Ok(finding) => vec![finding],
        Err(_) => vec![QuestReviewFinding {
            id: "execution-failed".to_owned(),
            title: "Quest execution failed".to_owned(),
            summary: reason.clone(),
            severity: "high".to_owned(),
            artifact_path: None,
            source: Some("execution".to_owned()),
        }],
    };
    let review = QuestReview {
        summary: "Quest execution stopped before a reviewable bundle was produced.".to_owned(),
        changed_files: Vec::new(),
        transaction_groups: Vec::new(),
        exploration_attempts: Vec::new(),
        findings,
        validations: vec![ValidationResult::new(
            "Quest execution",
            "failed",
            reason.clone(),
        )],
        unresolved_issues: vec![reason.clone()],
        next_actions: vec![
            QuestReviewAction::with_target(
                "inspect-error",
                "Inspect failure details",
                "open_review_finding",
                reason.clone(),
            ),
            QuestReviewAction::with_target(
                "revise-spec",
                "Revise Quest spec",
                "revise",
                reason.clone(),
            ),
            QuestReviewAction::new("retry-execution", "Retry execution", "retry"),
        ],
        project_fingerprint: None,
        metrics: QuestReviewMetrics {
            intent_to_first_action_ms: Some(elapsed_millis(started_at)),
            tool_call_latency_ms: None,
            validator_turnaround_ms: None,
            context_relevance_score: None,
            failed_action_recovery_rate: Some(0.0),
            review_evidence_quality_score: Some(0.2),
            isolated_attempt_count: 0,
            validation_count: 1,
            validation_failure_count: 1,
            baseline_changed_file_count: 0,
            notes: vec![
                "Execution failed before the isolated attempt produced review evidence.".to_owned(),
            ],
        },
        risk: "medium".to_owned(),
    };
    let _ = quest_store.append_timeline_event(
        id,
        "blocked",
        "Quest execution failed",
        serde_json::json!({ "error": reason }),
    );
    let detail = quest_store.set_review(id, QuestStatus::Blocked, review)?;
    serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
}

#[derive(Default)]
struct QuestModelTraceCapture {
    thinking: String,
    text: String,
    tool_calls: Vec<String>,
}

impl QuestModelTraceCapture {
    fn push(&mut self, delta: engine_ai::AiStreamDelta) {
        match delta {
            engine_ai::AiStreamDelta::Thinking(value) => self.thinking.push_str(&value),
            engine_ai::AiStreamDelta::Text(value) => self.text.push_str(&value),
            engine_ai::AiStreamDelta::ToolCallDelta(tool_call) => {
                self.tool_calls
                    .push(serde_json::to_string(&tool_call).unwrap_or_default());
            }
        }
    }

    fn to_markdown(&self) -> String {
        if self.thinking.trim().is_empty()
            && self.text.trim().is_empty()
            && self.tool_calls.is_empty()
        {
            return String::new();
        }

        let mut markdown = String::new();
        if !self.thinking.trim().is_empty() {
            markdown.push_str("## Thinking\n\n");
            markdown.push_str(self.thinking.trim());
            markdown.push_str("\n\n");
        }
        if !self.text.trim().is_empty() {
            markdown.push_str("## Model response\n\n");
            markdown.push_str(self.text.trim());
            markdown.push_str("\n\n");
        }
        if !self.tool_calls.is_empty() {
            markdown.push_str("## Tool call stream\n\n");
            for tool_call in &self.tool_calls {
                markdown.push_str("- `");
                markdown.push_str(tool_call);
                markdown.push_str("`\n");
            }
        }
        markdown
    }
}

fn prepare_quest_workspace(
    quest_store: &QuestStore,
    detail: &quest::QuestDetail,
) -> EngineResult<PathBuf> {
    prepare_quest_attempt_workspace(
        quest_store,
        detail,
        &format!("workspace-{}", unix_time_ms()),
    )
}

fn prepare_quest_attempt_workspace(
    quest_store: &QuestStore,
    detail: &quest::QuestDetail,
    attempt_id: &str,
) -> EngineResult<PathBuf> {
    let workspace_id = format!("{attempt_id}-{}", unix_time_ms());
    let workspace_root = quest_store
        .quest_path(&detail.record.id)?
        .join("workspaces")
        .join(workspace_id);
    prepare_isolated_workspace(&detail.record.project.path, &workspace_root, || {
        unix_time_ms().to_string()
    })?;
    Ok(workspace_root)
}

pub(crate) fn run_quest_execution(prepared: PreparedQuestExecution) -> EngineResult<Value> {
    let id = prepared.quest_id.as_str();
    let quest_store = &prepared.quest_store;
    let mut detail = quest_store.get(id)?;
    if detail.record.status == QuestStatus::Draft {
        detail = quest_store.transition(id, QuestStatus::Specified)?;
    }
    if !matches!(
        detail.record.status,
        QuestStatus::Specified | QuestStatus::WaitingForUser | QuestStatus::Blocked
    ) {
        return Err(EngineError::config(
            "Quest must be specified, waiting, or blocked before execution",
        ));
    }

    let quest_started_at = Instant::now();
    let project_root = detail.record.project.path.clone();
    let workspace_root = prepare_quest_workspace(quest_store, &detail)?;
    let workspace_id = workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_owned();
    quest_store.set_workspace_id(id, workspace_id.clone())?;
    quest_store.transition(id, QuestStatus::Prepared)?;
    quest_store.transition(id, QuestStatus::Running)?;
    quest_store.append_timeline_event(
        id,
        "snapshot",
        "Created isolated Quest workspace",
        serde_json::json!({
            "source_project": project_root,
            "workspace": workspace_root,
        }),
    )?;
    quest_store.append_timeline_event(
        id,
        "command_policy",
        "Registered Solo sandbox command policy",
        serde_json::json!({
            "sandbox_commands": AgentCommandPolicy::validation_registry_summary(),
            "outside_sandbox_commands": "approval_required",
            "destructive_commands": "denied_by_default",
        }),
    )?;
    detail = quest_store.record_checkpoint(
        id,
        "isolated-workspace",
        "Isolated workspace checkpoint",
        "Captured the isolated Quest workspace before model execution and validation.",
        Some(workspace_id.clone()),
        Some(project_fingerprint(&project_root)?),
    )?;
    let baseline_workspace_root =
        prepare_quest_attempt_workspace(quest_store, &detail, "baseline")?;
    quest_store.append_timeline_event(
        id,
        "alternative",
        "Created isolated baseline comparison workspace",
        serde_json::json!({
            "attempt_id": "baseline",
            "workspace": baseline_workspace_root,
            "selected": false,
        }),
    )?;

    let before = collect_workspace_snapshot(&workspace_root)?;
    let baseline_before = collect_workspace_snapshot(&baseline_workspace_root)?;
    let context = engine_editor::ProjectContext::open(&workspace_root)?;
    let knowledge_context = if detail.attached_knowledge.is_empty() {
        String::new()
    } else {
        let entries = detail
            .attached_knowledge
            .iter()
            .map(|entry| format!("- [{}] {}", entry.category, entry.content))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\n\nApproved project Knowledge attached to this Quest:\n{entries}")
    };
    let prompt = SoloQuestRunner::initial_prompt(&detail.spec, &knowledge_context);
    let mut session = AgentSession::new(context)?;
    let model = create_quest_model_from_prepared(prepared.model_provider)?;
    let first_action_started_at = Instant::now();
    let mut planning_trace = QuestModelTraceCapture::default();
    let plan = session.plan_with_history_streaming(
        model.as_ref(),
        &prompt,
        &[],
        PermissionPolicy::worktree_write(),
        parse_thinking_effort(&detail.record.model_config.thinking_effort),
        &mut |delta| planning_trace.push(delta),
    )?;
    quest_store.write_thinking_trace(
        id,
        "initial-plan",
        "Initial planning model trace",
        &planning_trace.to_markdown(),
    )?;
    let plan_latency_ms = elapsed_millis(first_action_started_at);
    let planned: Vec<String> = plan
        .operations
        .iter()
        .map(|operation| operation.preview.clone())
        .collect();
    quest_store.append_timeline_event(
        id,
        "plan",
        "Model produced an executable Quest plan",
        serde_json::json!({
            "operations": planned,
            "requires_write": plan.requires_write,
        }),
    )?;

    let apply_started_at = Instant::now();
    let mut outcome = session.apply_plan(&plan)?;
    let mut tool_call_latency_ms = plan_latency_ms + elapsed_millis(apply_started_at);
    save_project_context_scene(&session.context)?;
    quest_store.append_timeline_event(
        id,
        "execution",
        "Applied model operations in isolated workspace",
        serde_json::json!({
            "operations_performed": outcome.operations_performed,
            "completed": outcome.completed,
            "summary": outcome.summary,
            "trace": outcome.trace_entries.iter().map(|entry| serde_json::json!({
                "tool": entry.tool,
                "result": entry.result,
                "recovery_hint": entry.recovery_hint,
            })).collect::<Vec<_>>(),
        }),
    )?;

    quest_store.transition(id, QuestStatus::Validating)?;
    let validation_started_at = Instant::now();
    let mut validations = validate_quest_workspace(&workspace_root);
    append_validation_events(quest_store, id, &validations, None)?;
    let mut repair_attempts = 0usize;
    while validations_failed(&validations) && repair_attempts < SoloQuestRunner::REPAIR_LIMIT {
        repair_attempts += 1;
        quest_store.transition(id, QuestStatus::Repairing)?;
        let repair_prompt =
            SoloQuestRunner::repair_prompt(&detail.spec, &validations, repair_attempts);
        quest_store.append_timeline_event(
            id,
            "repair",
            &format!("Solo repair attempt {repair_attempts} started"),
            serde_json::json!({
                "attempt": repair_attempts,
                "failed_validations": validation_failure_summaries(&validations),
            }),
        )?;
        let repair_started_at = Instant::now();
        let repair_trace_id = format!("repair-plan-{repair_attempts}");
        let mut repair_trace = QuestModelTraceCapture::default();
        let repair_plan = session.plan_with_history_streaming(
            model.as_ref(),
            &repair_prompt,
            &[],
            PermissionPolicy::worktree_write(),
            parse_thinking_effort(&detail.record.model_config.thinking_effort),
            &mut |delta| repair_trace.push(delta),
        )?;
        quest_store.write_thinking_trace(
            id,
            &repair_trace_id,
            &format!("Repair attempt {repair_attempts} model trace"),
            &repair_trace.to_markdown(),
        )?;
        let repair_plan_latency_ms = elapsed_millis(repair_started_at);
        let planned_repair: Vec<String> = repair_plan
            .operations
            .iter()
            .map(|operation| operation.preview.clone())
            .collect();
        quest_store.append_timeline_event(
            id,
            "repair_plan",
            &format!("Model produced Solo repair plan {repair_attempts}"),
            serde_json::json!({
                "attempt": repair_attempts,
                "operations": planned_repair,
                "requires_write": repair_plan.requires_write,
            }),
        )?;
        let repair_apply_started_at = Instant::now();
        let repair_outcome = session.apply_plan(&repair_plan)?;
        tool_call_latency_ms += repair_plan_latency_ms + elapsed_millis(repair_apply_started_at);
        merge_agent_outcome(&mut outcome, repair_outcome);
        save_project_context_scene(&session.context)?;
        quest_store.append_timeline_event(
            id,
            "repair",
            &format!("Solo repair attempt {repair_attempts} applied"),
            serde_json::json!({
                "attempt": repair_attempts,
                "operations_performed": outcome.operations_performed,
                "trace": outcome.trace_entries.iter().map(|entry| serde_json::json!({
                    "tool": entry.tool,
                    "result": entry.result,
                    "recovery_hint": entry.recovery_hint,
                })).collect::<Vec<_>>(),
            }),
        )?;
        quest_store.transition(id, QuestStatus::Validating)?;
        validations = validate_quest_workspace(&workspace_root);
        append_validation_events(quest_store, id, &validations, Some(repair_attempts))?;
    }
    let baseline_validations = validate_quest_workspace(&baseline_workspace_root);
    append_validation_events(quest_store, id, &baseline_validations, Some(0))?;
    let validator_turnaround_ms = elapsed_millis(validation_started_at);

    let after = collect_workspace_snapshot(&workspace_root)?;
    let changed_files = diff_workspace_snapshots(&before, &after);
    let baseline_after = collect_workspace_snapshot(&baseline_workspace_root)?;
    let baseline_changed_files = diff_workspace_snapshots(&baseline_before, &baseline_after);
    for file in &changed_files {
        quest_store.append_timeline_event(
            id,
            "file_edit",
            &format!("{} {}", file.status, file.path),
            serde_json::json!({
                "path": file.path,
                "status": file.status,
                "additions": file.additions,
                "deletions": file.deletions,
            }),
        )?;
    }
    let has_failed_validation = validations
        .iter()
        .any(|validation| validation.status != "passed" && validation.status != "skipped");
    let baseline_failed_validation = baseline_validations
        .iter()
        .any(|validation| validation.status != "passed" && validation.status != "skipped");
    let exploration_summary = if has_failed_validation {
        "Selected implementation attempt preserved with validation failures for repair or revision."
    } else {
        "Selected implementation attempt produced the current review bundle."
    };
    quest_store.write_exploration_attempt(
        id,
        "selected-implementation",
        "Selected implementation attempt",
        exploration_summary,
        true,
    )?;
    let baseline_summary = format!(
        "Baseline comparison attempt preserved before applying model edits: {} changed file(s), {} validation issue(s).",
        baseline_changed_files.len(),
        baseline_validations
            .iter()
            .filter(|validation| validation.status == "failed")
            .count()
    );
    quest_store.write_exploration_attempt(
        id,
        "baseline",
        "Baseline comparison attempt",
        &baseline_summary,
        false,
    )?;
    let unresolved_issues = if has_failed_validation {
        validations
            .iter()
            .filter(|validation| validation.status == "failed")
            .map(|validation| format!("Validation failed: {}", validation.summary))
            .collect()
    } else if changed_files.is_empty() {
        vec!["Quest execution completed without producing file changes.".to_owned()]
    } else {
        Vec::new()
    };
    let mut findings = Vec::new();
    if has_failed_validation {
        for (index, validation) in validations
            .iter()
            .filter(|validation| validation.status == "failed")
            .enumerate()
        {
            findings.push(quest_store.write_review_finding(
                id,
                &format!("validation-failed-{index}"),
                &format!("Validation failed: {}", validation.name),
                &validation.summary,
                "high",
                Some("validation"),
            )?);
        }
    } else if changed_files.is_empty() {
        findings.push(quest_store.write_review_finding(
            id,
            "no-changes",
            "Quest produced no file changes",
            "Quest execution completed in the isolated workspace without producing changed files.",
            "medium",
            Some("review"),
        )?);
    }
    let next_actions = quest_review_actions_for_result(
        &unresolved_issues,
        has_failed_validation,
        changed_files.is_empty(),
    );
    let status = if has_failed_validation {
        QuestStatus::Blocked
    } else {
        QuestStatus::ReadyForReview
    };
    let has_changes = !changed_files.is_empty();
    let validation_count = validations.len() as u32;
    let validation_failure_count = validations
        .iter()
        .filter(|validation| validation.status == "failed")
        .count() as u32;
    let context_relevance = context_relevance_score(&detail);
    let recovery_rate = failed_action_recovery_rate(&outcome.trace_entries);
    let evidence_quality =
        review_evidence_quality_score(has_changes, &validations, has_failed_validation);
    let review = QuestReview {
        summary: if changed_files.is_empty() {
            "Quest executed in an isolated workspace but produced no file changes.".to_owned()
        } else {
            format!(
                "Quest executed in isolated workspace `{workspace_id}` and produced {} changed file(s).",
                changed_files.len()
            )
        },
        transaction_groups: transaction_groups_from_changed_files(&changed_files),
        exploration_attempts: vec![
            QuestExplorationAttempt {
                id: "selected-implementation".to_owned(),
                label: "Selected implementation attempt".to_owned(),
                summary: exploration_summary.to_owned(),
                outcome: if has_failed_validation {
                    "needs_repair"
                } else {
                    "selected"
                }
                .to_owned(),
                artifact_path: "explorations/selected-implementation.md".to_owned(),
                selected: true,
            },
            QuestExplorationAttempt {
                id: "baseline".to_owned(),
                label: "Baseline comparison attempt".to_owned(),
                summary: baseline_summary,
                outcome: if baseline_failed_validation {
                    "baseline_failed_validation"
                } else {
                    "baseline_reference"
                }
                .to_owned(),
                artifact_path: "explorations/baseline.md".to_owned(),
                selected: false,
            },
        ],
        findings,
        changed_files,
        validations,
        unresolved_issues,
        next_actions,
        project_fingerprint: Some(project_fingerprint(&project_root)?),
        metrics: QuestReviewMetrics {
            intent_to_first_action_ms: Some(elapsed_millis(quest_started_at)),
            tool_call_latency_ms: Some(tool_call_latency_ms),
            validator_turnaround_ms: Some(validator_turnaround_ms),
            context_relevance_score: Some(context_relevance),
            failed_action_recovery_rate: Some(recovery_rate),
            review_evidence_quality_score: Some(evidence_quality),
            isolated_attempt_count: 2,
            validation_count,
            validation_failure_count,
            baseline_changed_file_count: baseline_changed_files.len() as u32,
            notes: vec![
                "Metrics are captured from the isolated Quest execution path.".to_owned(),
                "Baseline attempt is preserved for comparison against the selected implementation."
                    .to_owned(),
                format!(
                    "Solo repair attempts used: {repair_attempts}/{}.",
                    SoloQuestRunner::REPAIR_LIMIT
                ),
            ],
        },
        risk: if has_failed_validation {
            "medium"
        } else {
            "low"
        }
        .to_owned(),
    };
    let apply_decision = QuestApplyPolicy::classify(&review, &detail.record.autonomy);
    let detail = quest_store.set_review(id, status, review)?;
    quest_store.append_timeline_event(
            id,
            "apply_policy",
            "Quest apply policy classified Solo result",
            serde_json::json!({
                "decision": QuestApplyPolicy::as_str(apply_decision),
                "risk": detail.record.review.as_ref().map(|review| review.risk.as_str()),
                "active_project_apply_requires_approval": detail.record.autonomy.active_project_apply_requires_approval,
                "changed_files": detail.record.review.as_ref().map(|review| review.changed_files.len()).unwrap_or_default(),
            }),
        )?;
    quest_store.append_timeline_event(
        id,
        if status == QuestStatus::ReadyForReview {
            "review_ready"
        } else {
            "blocked"
        },
        if status == QuestStatus::ReadyForReview {
            "Quest is ready for review"
        } else {
            "Quest is blocked by validation failures"
        },
        serde_json::json!({ "workspace": workspace_root }),
    )?;
    if apply_decision == QuestApplyDecision::AutoApply && status == QuestStatus::ReadyForReview {
        quest_store.append_timeline_event(
            id,
            "apply_policy",
            "Auto-apply deferred to the desktop thread",
            serde_json::json!({
                "reason": "background_execution_cannot_touch_active_project",
            }),
        )?;
    }
    serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
}

fn create_quest_model_from_prepared(
    prepared: PreparedQuestModelRequest,
) -> EngineResult<Box<dyn engine_ai::AiModel>> {
    engine_ai::providers::create_provider(
        &prepared.provider,
        &prepared.model,
        prepared.api_key.as_deref(),
        prepared.endpoint.as_deref(),
        prepared.max_tokens,
        prepared.codex_oauth,
        prepared.mimo_config.as_ref(),
        prepared.glm_config.as_ref(),
    )
}

fn save_project_context_scene(context: &engine_editor::ProjectContext) -> EngineResult<()> {
    let scene_name = context
        .scene_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Scene");
    let json = context.scene.to_json(scene_name)?;
    if let Some(parent) = context.scene_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&context.scene_path, json).map_err(|source| EngineError::Filesystem {
        path: context.scene_path.clone(),
        source,
    })
}

fn asset_reference_row(kind: &str, label: &str, detail: String) -> Value {
    serde_json::json!({
        "kind": kind,
        "label": label,
        "detail": detail,
    })
}

fn resolve_asset_reference_label(
    project: &engine_editor::ProjectContext,
    guid: engine_assets::AssetGuid,
) -> String {
    project
        .database
        .resolve_guid(guid)
        .ok()
        .and_then(|path| path.to_utf8().ok().map(str::to_owned))
        .unwrap_or_else(|| guid.to_string())
}

fn asset_id_matches_guid(
    asset: Option<engine_core::AssetId>,
    guid: engine_assets::AssetGuid,
) -> bool {
    asset.is_some_and(|asset| asset.as_u128() == guid.as_u128())
}

fn collect_component_asset_references(
    rows: &mut Vec<Value>,
    object_name: &str,
    component: &engine_ecs::ComponentData,
    guid: engine_assets::AssetGuid,
) {
    use engine_ecs::ComponentData;
    match component {
        ComponentData::MeshRenderer(mesh) => {
            if asset_id_matches_guid(mesh.mesh, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "MeshRenderer mesh",
                    format!("{object_name} -> MeshRenderer.mesh"),
                ));
            }
            if asset_id_matches_guid(mesh.material.asset, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "MeshRenderer material",
                    format!("{object_name} -> MeshRenderer.material"),
                ));
            }
        }
        ComponentData::SkinnedMeshRenderer(mesh) => {
            if asset_id_matches_guid(mesh.mesh, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "SkinnedMeshRenderer mesh",
                    format!("{object_name} -> SkinnedMeshRenderer.mesh"),
                ));
            }
            if asset_id_matches_guid(mesh.material.asset, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "SkinnedMeshRenderer material",
                    format!("{object_name} -> SkinnedMeshRenderer.material"),
                ));
            }
        }
        ComponentData::AudioSource(audio) => {
            if asset_id_matches_guid(audio.clip, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "AudioSource clip",
                    format!("{object_name} -> AudioSource.clip"),
                ));
            }
        }
        ComponentData::AudioStreamPlayer2D(audio) => {
            if asset_id_matches_guid(audio.clip, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "AudioStreamPlayer2D clip",
                    format!("{object_name} -> AudioStreamPlayer2D.clip"),
                ));
            }
        }
        ComponentData::AudioStreamPlayer3D(audio) => {
            if asset_id_matches_guid(audio.clip, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "AudioStreamPlayer3D clip",
                    format!("{object_name} -> AudioStreamPlayer3D.clip"),
                ));
            }
        }
        ComponentData::Skybox(skybox) => {
            if asset_id_matches_guid(skybox.cubemap, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "Skybox cubemap",
                    format!("{object_name} -> Skybox.cubemap"),
                ));
            }
        }
        ComponentData::Sprite2D(sprite) => {
            if asset_id_matches_guid(sprite.texture, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "Sprite2D texture",
                    format!("{object_name} -> Sprite2D.texture"),
                ));
            }
        }
        ComponentData::TileMap(tilemap) => {
            if asset_id_matches_guid(tilemap.tileset, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "TileMap tileset",
                    format!("{object_name} -> TileMap.tileset"),
                ));
            }
        }
        ComponentData::AnimationPlayer(animation) => {
            if asset_id_matches_guid(animation.clip, guid) {
                rows.push(asset_reference_row(
                    "scene",
                    "AnimationPlayer clip",
                    format!("{object_name} -> AnimationPlayer.clip"),
                ));
            }
        }
        _ => {}
    }
}

fn write_project_asset(
    project: &engine_editor::ProjectContext,
    asset_path: &str,
    content: &str,
) -> EngineResult<(String, PathBuf)> {
    let asset_root = project.root.join(&project.manifest.asset_root);
    let full_path = resolve_writable_relative_path(&asset_root, asset_path)?;
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

    Ok((asset_path.to_owned(), full_path))
}

fn varg_scene_template(name: &str) -> String {
    format!(
        r##"scene {name} {{
    camera "MainCamera" {{
        transform {{
            position: Vec3(0, 3, 8)
            rotation: Euler(-20, 0, 0)
        }}

        perspective {{
            fov: 60
            near: 0.1
            far: 1000
        }}

        primary: true
    }}

    light "Sun" {{
        kind: directional
        intensity: 2.0
        rotation: Euler(-45, 35, 0)
    }}

    entity "Ground" {{
        mesh: Box(size: Vec3(12, 1, 12))

        material {{
            baseColor: Color("#4f7f4a")
            roughness: 0.8
        }}

        collider {{
            shape: box
        }}
    }}
}}
"##
    )
}

fn varg_prefab_template(name: &str) -> String {
    format!(
        r##"prefab {name} {{
    entity "{name}" {{
        mesh: Box(size: Vec3(1, 1, 1))

        material {{
            baseColor: Color("#7aa2ff")
            roughness: 0.65
        }}

        collider {{
            shape: box
            size: Vec3(1, 1, 1)
        }}
    }}
}}
"##
    )
}

fn varg_material_template(name: &str) -> String {
    format!(
        r##"material {name} {{
    shader: "pbr"

    baseColor: Color("#7aa2ff")
    roughness: 0.7
    metallic: 0.0
}}
"##
    )
}

fn push_created_asset_console(console: &mut ConsoleService, kind: &str, full_path: &Path) {
    console.push(engine_editor::ConsoleEntry {
        timestamp: timestamp_now(),
        level: engine_editor::ConsoleLevel::Info,
        source: engine_editor::ConsoleSource {
            subsystem: "editor".into(),
            file: Some(full_path.to_path_buf()),
            line: None,
        },
        message: format!("Created {kind}: {}", full_path.display()),
    });
}

fn aster_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn append_generated_question_cards(
    quest_store: &QuestStore,
    quest_id: &str,
    question_cards: Vec<GeneratedQuestionCard>,
) -> EngineResult<bool> {
    let has_question_cards = !question_cards.is_empty();
    for card in question_cards {
        quest_store.append_timeline_event(
            quest_id,
            "question_card",
            &card.title,
            serde_json::json!({
                "title": card.title,
                "questions": card.questions.into_iter().map(|question| serde_json::json!({
                    "id": question.id,
                    "prompt": question.prompt,
                    "allow_multiple": question.allow_multiple,
                    "allow_custom": question.allow_custom,
                    "options": question.options.into_iter().map(|option| serde_json::json!({
                        "id": option.id,
                        "label": option.label,
                        "description": option.description,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            }),
        )?;
    }
    Ok(has_question_cards)
}

fn format_editor_knowledge_context(entries: &[quest::KnowledgeEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let lines = entries
        .iter()
        .map(|entry| {
            format!(
                "- [{}] {} (source: {})",
                entry.category, entry.content, entry.source
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n\n[Approved Project Knowledge]\n{lines}")
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

// ─── Tauri commands ─────────────────────────────────────────────────────────

/// Binary viewport readback — returns raw RGBA pixels as ArrayBuffer.
/// Response layout: [width: u32 LE][height: u32 LE][RGBA pixels...]
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

// ─── App entry point ─────────────────────────────────────────────────────────

pub fn run() {
    app::run();
}

#[cfg(test)]
mod tests {
    use super::{
        CODEX_OAUTH_CLIENT_ID, CODEX_OAUTH_REDIRECT_URI, CODEX_OAUTH_SCOPE, DesktopEnvironment,
        EditorHost, QuestApplyDecision, QuestApplyPolicy, QuestProject, QuestReview,
        QuestReviewMetrics, QuestStatus, SoloQuestRunner, ValidationResult,
        asset_meta_path_for_source, codex_authorize_url, copilot_execution_summary,
        extract_codex_account_id, model_detection_config, normalize_relative_path,
        parse_generated_quest_response, project_fingerprint, quest,
        quest_review_actions_for_result, resolve_existing_relative_path,
        resolve_writable_relative_path, should_continue_copilot,
        transaction_groups_from_changed_files, validate_quest_workspace, validations_failed,
    };
    use base64::Engine as _;
    use engine_editor::{CopilotProvider, FileEditorStore};
    use engine_quest::ChangedFile;
    use std::path::Path;
    use std::{collections::HashMap, fs};

    #[test]
    fn kde_uses_native_chrome_when_native_wayland_is_preferred() {
        assert!(DesktopEnvironment::Kde.prefers_native_chrome_for_backend(true));
    }

    #[test]
    fn kde_keeps_native_chrome_when_using_x11_backend() {
        assert!(DesktopEnvironment::Kde.prefers_native_chrome_for_backend(false));
    }

    #[test]
    fn viewport_compositor_is_requested_by_default_when_a_native_adapter_is_available() {
        let native_host = super::editor_compositor::EditorCompositorSupport {
            backend: super::editor_compositor::EditorCompositorBackend::LinuxGtk,
            available: true,
            reason: "available",
        };
        let unavailable_host = super::editor_compositor::EditorCompositorSupport {
            backend: super::editor_compositor::EditorCompositorBackend::LinuxGtk,
            available: false,
            reason: "unavailable",
        };
        let wayland = super::wayland_embedded_compositor::WaylandEmbeddedCompositorSupport {
            status: super::wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::Available,
            available: true,
            reason: "available",
        };
        let unavailable_wayland =
            super::wayland_embedded_compositor::WaylandEmbeddedCompositorSupport {
                status: super::wayland_embedded_compositor::WaylandEmbeddedCompositorStatus::FeatureDisabled,
                available: false,
                reason: "feature disabled",
            };

        assert!(super::default_editor_compositor_requested_for(
            native_host,
            unavailable_wayland
        ));
        assert!(!super::default_editor_compositor_requested_for(
            unavailable_host,
            wayland
        ));
        assert!(!super::default_editor_compositor_requested_for(
            unavailable_host,
            unavailable_wayland
        ));
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
        let script_path = asset_root.join("scripts/player.varg");
        fs::create_dir_all(script_path.parent().unwrap()).unwrap();
        fs::write(&script_path, "script Player { func start() {} }").unwrap();

        let resolved = resolve_existing_relative_path(&asset_root, "scripts/player.varg").unwrap();

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
            resolve_writable_relative_path(&asset_root, "scripts/new_script.varg").unwrap();

        assert_eq!(
            resolved,
            asset_root
                .canonicalize()
                .unwrap()
                .join("scripts/new_script.varg")
        );
    }

    #[test]
    fn quest_creation_accepts_text_only_model_responses() {
        let generated = parse_generated_quest_response(
            &[],
            "# Build Patrol Scene\n\nCreate a focused patrol prototype.",
            "Build a patrol scene",
        )
        .unwrap();

        assert_eq!(generated.title, "Build Patrol Scene");
        assert!(generated.spec.contains("focused patrol"));
        assert!(generated.tasks.is_empty());
    }

    #[test]
    fn quest_ai_network_calls_are_rejected_by_synchronous_rpc_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let mut host = host_with_quest_root(temp.path());

        let create_error = host
            .handle(
                "quest/create",
                &serde_json::json!({ "title": "", "goal": "Build a scene" }),
            )
            .unwrap_err();
        let rewrite_error = host
            .handle(
                "quest/rewrite_prompt",
                &serde_json::json!({ "prompt": "make game" }),
            )
            .unwrap_err();

        assert!(create_error.to_string().contains("background Quest AI"));
        assert!(rewrite_error.to_string().contains("background Quest AI"));
    }

    #[test]
    fn streamed_quest_rewrite_is_cleaned_before_returning_to_the_ui() {
        let temp = tempfile::tempdir().unwrap();
        let mut host = host_with_quest_root(temp.path());

        let value = host
            .finish_quest_rewrite("  \"Build a deterministic patrol scene\"  ".to_owned())
            .unwrap();

        assert_eq!(
            value["prompt"].as_str(),
            Some("Build a deterministic patrol scene")
        );
    }

    #[test]
    fn blocked_quest_reviews_include_clear_next_actions() {
        let issues = vec!["Validation failed: cargo test failed".to_owned()];
        let actions = quest_review_actions_for_result(&issues, true, false);

        assert!(actions.iter().any(|action| action.kind == "quick_fix"
            && action.target.as_deref() == Some("Validation failed: cargo test failed")));
        assert!(actions.iter().any(|action| action.kind == "revise"));
        assert!(actions.iter().any(|action| action.kind == "retry"));
    }

    #[test]
    fn no_change_reviews_offer_inspect_revise_or_archive_actions() {
        let issues = vec!["Quest execution completed without producing file changes.".to_owned()];
        let actions = quest_review_actions_for_result(&issues, false, true);

        assert!(
            actions
                .iter()
                .any(|action| action.kind == "open_review_finding")
        );
        assert!(actions.iter().any(|action| action.kind == "revise"));
        assert!(actions.iter().any(|action| action.kind == "archive"));
    }

    #[test]
    fn quest_creation_accepts_native_spec_and_task_tool_calls() {
        let generated = parse_generated_quest_response(
            &[
                engine_ai::ToolCall {
                    id: "call-spec".to_owned(),
                    name: "create_or_update_spec".to_owned(),
                    arguments: serde_json::json!({
                        "title": "Build Patrol Scene",
                        "markdown": "# Build Patrol Scene\n\nThe model chose this spec shape."
                    }),
                },
                engine_ai::ToolCall {
                    id: "call-task".to_owned(),
                    name: "create_task".to_owned(),
                    arguments: serde_json::json!({
                        "title": "Create patrol behavior",
                        "summary": "Author the behavior through available editor tools.",
                        "acceptance": ["Behavior file exists", "Scene references it"]
                    }),
                },
            ],
            "",
            "Build a patrol scene",
        )
        .unwrap();

        assert_eq!(generated.title, "Build Patrol Scene");
        assert!(generated.spec.contains("model chose"));
        assert_eq!(generated.tasks.len(), 1);
        assert_eq!(generated.tasks[0].acceptance.len(), 2);
    }

    #[test]
    fn quest_creation_accepts_question_card_tool_calls() {
        let generated = parse_generated_quest_response(
            &[engine_ai::ToolCall {
                id: "call-question".to_owned(),
                name: "ask_questions".to_owned(),
                arguments: serde_json::json!({
                    "title": "Questions",
                    "questions": [{
                        "id": "scope",
                        "prompt": "Which scope should Varg optimize first?",
                        "allow_multiple": false,
                        "allow_custom": true,
                        "options": [
                            { "id": "A", "label": "Rendering", "description": "Focus on frame time." },
                            { "id": "B", "label": "Editor UX" }
                        ]
                    }]
                }),
            }],
            "",
            "Optimize Varg",
        )
        .unwrap();

        assert_eq!(generated.question_cards.len(), 1);
        assert_eq!(generated.question_cards[0].questions.len(), 1);
        assert_eq!(generated.question_cards[0].questions[0].options.len(), 2);
        assert!(generated.question_cards[0].questions[0].allow_custom);
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
        let store = FileEditorStore::new(temp.path().join("varg-editor-state.toml"));
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
        let store = FileEditorStore::new(temp.path().join("varg-editor-state.toml"));
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
    fn oauth_provider_clears_api_key_when_settings_are_updated() {
        let temp = tempfile::tempdir().unwrap();
        let state_path = temp.path().join("varg-editor-state.toml");
        let store = FileEditorStore::new(&state_path);
        let mut host = EditorHost::new(store).unwrap();

        host.update_copilot_settings(&serde_json::json!({
            "provider": "custom",
            "model": "varg-test-model",
            "api_endpoint": "https://provider.example/v1",
            "api_key": "secret-test-key",
            "max_tokens": 4096
        }))
        .unwrap();

        host.update_copilot_settings(&serde_json::json!({
            "provider": "codex_oauth",
            "model": "gpt-5.4",
            "api_key": "should-not-stick",
            "max_tokens": 4096
        }))
        .unwrap();

        assert_eq!(host.copilot_settings.provider, CopilotProvider::CodexOAuth);
        assert_eq!(host.copilot_settings.api_key, None);

        let credentials_path = state_path.parent().unwrap().join("credentials.toml");
        let credentials = fs::read_to_string(credentials_path).unwrap();
        assert!(!credentials.contains("secret-test-key"));
        assert!(!credentials.contains("should-not-stick"));
    }

    #[test]
    fn codex_authorize_url_uses_browser_pkce_flow() {
        let url = codex_authorize_url("challenge-test", "state-test");
        let (base, query) = url.split_once('?').unwrap();
        let params = query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(key, value)| {
                (
                    key.to_owned(),
                    urlencoding::decode(value).unwrap().into_owned(),
                )
            })
            .collect::<HashMap<_, _>>();

        assert_eq!(base, "https://auth.openai.com/oauth/authorize");
        assert_eq!(params["response_type"], "code");
        assert_eq!(params["client_id"], CODEX_OAUTH_CLIENT_ID);
        assert_eq!(params["redirect_uri"], CODEX_OAUTH_REDIRECT_URI);
        assert_eq!(params["scope"], CODEX_OAUTH_SCOPE);
        assert_eq!(params["code_challenge"], "challenge-test");
        assert_eq!(params["code_challenge_method"], "S256");
        assert_eq!(params["state"], "state-test");
        assert_eq!(params["id_token_add_organizations"], "true");
        assert_eq!(params["codex_cli_simplified_flow"], "true");
        assert_eq!(params["originator"], "codex_cli_rs");
    }

    #[test]
    fn copilot_settings_survive_host_restart() {
        let temp = tempfile::tempdir().unwrap();
        let state_path = temp.path().join("varg-editor-state.toml");

        {
            let store = FileEditorStore::new(&state_path);
            let mut host = EditorHost::new(store).unwrap();
            host.update_copilot_settings(&serde_json::json!({
                "provider": "custom",
                "model": "varg-test-model",
                "api_endpoint": "https://provider.example/v1",
                "api_key": "secret-test-key",
                "max_tokens": 8192
            }))
            .unwrap();
        }

        let state_text = fs::read_to_string(&state_path).unwrap();
        assert!(state_text.contains("varg-test-model"));
        assert!(!state_text.contains("secret-test-key"));

        let store = FileEditorStore::new(&state_path);
        let host = EditorHost::new(store).unwrap();
        assert_eq!(host.copilot_settings.provider, CopilotProvider::Custom);
        assert_eq!(host.copilot_settings.model, "varg-test-model");
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
        let state_path = temp.path().join("varg-editor-state.toml");

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
    fn editor_ai_requests_can_attach_only_approved_knowledge() {
        let temp = tempfile::tempdir().unwrap();
        let mut host = host_with_quest_root(temp.path());
        host.handle(
            "hub/create_project",
            &serde_json::json!({
                "name": "KnowledgeContextProject",
                "location": temp.path(),
                "template_id": "three_d",
                "toolchain_version": "0.1.0",
            }),
        )
        .unwrap();
        let project_path = temp.path().join("KnowledgeContextProject");
        host.handle(
            "hub/open_project",
            &serde_json::json!({ "path": project_path }),
        )
        .unwrap();
        host.copilot_settings.provider = CopilotProvider::Custom;
        host.copilot_settings.model = "test-model".to_owned();
        host.copilot_settings.api_endpoint = Some("https://provider.example/v1".to_owned());
        let pending = host
            .quest_store
            .propose_knowledge(
                "architecture",
                "Use the render graph for frame orchestration.",
                "manual",
            )
            .unwrap();
        let error = match host.prepare_copilot_request(&serde_json::json!({
            "prompt": "How should I wire rendering?",
            "knowledge_ids": [pending[0].id],
        })) {
            Ok(_) => panic!("pending Knowledge must not be accepted as Editor AI context"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("only attach approved Knowledge"));

        let approved = host.quest_store.approve_knowledge(&pending[0].id).unwrap();
        let prepared = host
            .prepare_copilot_request(&serde_json::json!({
                "prompt": "How should I wire rendering?",
                "knowledge_ids": [approved[0].id, approved[0].id],
            }))
            .unwrap();

        assert_eq!(prepared.knowledge_entries_used, 1);
        let user_message = prepared.request.messages.last().unwrap();
        assert!(
            user_message
                .content
                .contains("[Approved Project Knowledge]")
        );
        assert!(
            user_message
                .content
                .contains("Use the render graph for frame orchestration.")
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

    fn quest_project(root: &Path) -> QuestProject {
        QuestProject {
            name: "QuestApplyProject".to_owned(),
            path: root.to_path_buf(),
        }
    }

    fn host_with_quest_root(root: &Path) -> EditorHost {
        let store = FileEditorStore::new(root.join("varg-editor-state.toml"));
        EditorHost::new_with_quest_root(store, root.join("quests")).unwrap()
    }

    fn create_reviewable_quest(
        host: &mut EditorHost,
        project_root: &Path,
        changed_files: Vec<ChangedFile>,
    ) -> (String, std::path::PathBuf) {
        let created = host
            .quest_store
            .create(
                "Review apply boundary".to_owned(),
                "Apply reviewed files only.".to_owned(),
                "# Review apply boundary\n\n## Goal\n\nApply reviewed files only.".to_owned(),
                quest_project(project_root),
            )
            .unwrap();
        let id = created.record.id.clone();
        let workspace_id = "workspace-test".to_owned();
        let workspace_root = host
            .quest_store
            .quest_path(&id)
            .unwrap()
            .join("workspaces")
            .join(&workspace_id);
        fs::create_dir_all(&workspace_root).unwrap();
        host.quest_store
            .set_workspace_id(&id, workspace_id)
            .unwrap();
        host.quest_store
            .set_review(
                &id,
                QuestStatus::ReadyForReview,
                QuestReview {
                    summary: "Reviewable files are staged in an isolated workspace.".to_owned(),
                    transaction_groups: transaction_groups_from_changed_files(&changed_files),
                    changed_files,
                    exploration_attempts: Vec::new(),
                    findings: Vec::new(),
                    validations: vec![ValidationResult::new(
                        "Focused review test",
                        "passed",
                        "Workspace artifact was prepared by the test.",
                    )],
                    unresolved_issues: Vec::new(),
                    next_actions: Vec::new(),
                    project_fingerprint: Some(project_fingerprint(project_root).unwrap()),
                    metrics: QuestReviewMetrics::default(),
                    risk: "low".to_owned(),
                },
            )
            .unwrap();
        (id, workspace_root)
    }

    #[test]
    fn quest_validations_include_policy_registry_command_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let project_location = temp.path().join("projects");
        let mut host = host_with_quest_root(temp.path());
        let created = host
            .handle(
                "hub/create_project",
                &serde_json::json!({
                    "name": "Validation Evidence",
                    "location": project_location,
                    "template_id": "two_d",
                    "toolchain_version": "0.1.0",
                }),
            )
            .unwrap();
        let project_root = Path::new(created["path"].as_str().unwrap());
        fs::write(
            project_root.join("Cargo.toml"),
            "[package]\nname = \"validation-evidence\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(project_root.join("src")).unwrap();
        fs::write(project_root.join("src/lib.rs"), "pub fn ok() {}\n").unwrap();

        let validations = validate_quest_workspace(project_root);
        let cargo_check = validations
            .iter()
            .find(|validation| validation.name == "cargo check")
            .unwrap();

        assert_eq!(cargo_check.status, "passed");
        assert_eq!(cargo_check.command_id.as_deref(), Some("cargo_check_quiet"));
        assert_eq!(cargo_check.command.as_deref(), Some("cargo check --quiet"));
        assert!(cargo_check.policy_approved);
        assert!(!cargo_check.log.trim().is_empty());
    }

    #[test]
    fn quest_apply_policy_classifies_solo_results() {
        let changed_files = vec![ChangedFile {
            path: "src/main.rs".to_owned(),
            additions: 1,
            deletions: 0,
            status: "modified".to_owned(),
            diff: "diff".to_owned(),
        }];
        let mut review = QuestReview {
            summary: "Solo result".to_owned(),
            transaction_groups: transaction_groups_from_changed_files(&changed_files),
            changed_files,
            exploration_attempts: Vec::new(),
            findings: Vec::new(),
            validations: vec![ValidationResult::new("Project load", "passed", "ok")],
            unresolved_issues: Vec::new(),
            next_actions: Vec::new(),
            project_fingerprint: None,
            metrics: QuestReviewMetrics::default(),
            risk: "low".to_owned(),
        };
        let mut autonomy = quest::QuestAutonomyPolicy {
            active_project_apply_requires_approval: false,
            ..Default::default()
        };

        assert_eq!(
            QuestApplyPolicy::classify(&review, &autonomy),
            QuestApplyDecision::AutoApply
        );

        autonomy.active_project_apply_requires_approval = true;
        assert_eq!(
            QuestApplyPolicy::classify(&review, &autonomy),
            QuestApplyDecision::NeedsReview
        );

        review.validations.push(ValidationResult::new(
            "cargo check",
            "failed",
            "compile error",
        ));
        assert_eq!(
            QuestApplyPolicy::classify(&review, &autonomy),
            QuestApplyDecision::Blocked
        );
    }

    #[test]
    fn solo_repair_prompt_carries_validation_failures_and_limit() {
        let validations = vec![
            ValidationResult::new("Project load", "passed", "ok"),
            ValidationResult::new("cargo check", "failed", "missing semicolon"),
        ];

        let prompt = SoloQuestRunner::repair_prompt("# Spec", &validations, 1);

        assert!(validations_failed(&validations));
        assert!(prompt.contains("Repair attempt: 1/1"));
        assert!(prompt.contains("cargo check: missing semicolon"));
        assert!(prompt.contains("isolated workspace only"));
    }

    #[test]
    fn quest_apply_copies_reviewed_workspace_files_and_rollback_restores_active_project() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("src")).unwrap();
        fs::write(
            project_root.join("src/main.rs"),
            "fn main() {\n    old();\n}\n",
        )
        .unwrap();
        let mut host = host_with_quest_root(temp.path());
        let changed_files = vec![ChangedFile {
            path: "src/main.rs".to_owned(),
            additions: 1,
            deletions: 1,
            status: "modified".to_owned(),
            diff: "--- a/src/main.rs\n+++ b/src/main.rs\n".to_owned(),
        }];
        let (id, workspace_root) = create_reviewable_quest(&mut host, &project_root, changed_files);
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::write(
            workspace_root.join("src/main.rs"),
            "fn main() {\n    new();\n}\n",
        )
        .unwrap();

        let applied = host
            .handle("quest/apply", &serde_json::json!({ "id": id }))
            .unwrap();

        assert_eq!(applied["status"], "completed");
        assert_eq!(
            fs::read_to_string(project_root.join("src/main.rs")).unwrap(),
            "fn main() {\n    new();\n}\n"
        );
        let rollback_id = applied["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|decision| decision["kind"] == "apply")
            .and_then(|decision| decision["rollback_id"].as_str())
            .unwrap()
            .to_owned();

        let rolled_back = host
            .handle(
                "quest/rollback",
                &serde_json::json!({ "id": id, "rollback_id": rollback_id }),
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(project_root.join("src/main.rs")).unwrap(),
            "fn main() {\n    old();\n}\n"
        );
        assert!(
            rolled_back["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|decision| decision["kind"] == "rollback")
        );
    }

    #[test]
    fn quest_apply_rejects_stale_review_when_active_project_changed_after_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("src")).unwrap();
        fs::write(project_root.join("src/main.rs"), "old\n").unwrap();
        let mut host = host_with_quest_root(temp.path());
        let changed_files = vec![ChangedFile {
            path: "src/main.rs".to_owned(),
            additions: 1,
            deletions: 1,
            status: "modified".to_owned(),
            diff: "main diff".to_owned(),
        }];
        let (id, workspace_root) = create_reviewable_quest(&mut host, &project_root, changed_files);
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::write(workspace_root.join("src/main.rs"), "quest output\n").unwrap();
        fs::write(project_root.join("src/main.rs"), "user edit\n").unwrap();

        let error = host
            .handle("quest/apply", &serde_json::json!({ "id": id }))
            .unwrap_err();

        assert!(error.to_string().contains("review is stale"));
        assert_eq!(
            fs::read_to_string(project_root.join("src/main.rs")).unwrap(),
            "user edit\n"
        );
    }

    #[test]
    fn quest_partial_apply_respects_selected_transaction_groups() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("src")).unwrap();
        fs::write(project_root.join("src/a.rs"), "old a\n").unwrap();
        fs::write(project_root.join("src/b.rs"), "old b\n").unwrap();
        let mut host = host_with_quest_root(temp.path());
        let changed_files = vec![
            ChangedFile {
                path: "src/a.rs".to_owned(),
                additions: 1,
                deletions: 1,
                status: "modified".to_owned(),
                diff: "a diff".to_owned(),
            },
            ChangedFile {
                path: "src/b.rs".to_owned(),
                additions: 1,
                deletions: 1,
                status: "modified".to_owned(),
                diff: "b diff".to_owned(),
            },
        ];
        let selected_group = transaction_groups_from_changed_files(&changed_files)[0]
            .id
            .clone();
        let (id, workspace_root) = create_reviewable_quest(&mut host, &project_root, changed_files);
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::write(workspace_root.join("src/a.rs"), "new a\n").unwrap();
        fs::write(workspace_root.join("src/b.rs"), "new b\n").unwrap();

        let applied = host
            .handle(
                "quest/apply",
                &serde_json::json!({
                    "id": id,
                    "transaction_group_ids": [selected_group],
                }),
            )
            .unwrap();

        assert_eq!(applied["status"], "ready_for_review");
        assert_eq!(
            fs::read_to_string(project_root.join("src/a.rs")).unwrap(),
            "new a\n"
        );
        assert_eq!(
            fs::read_to_string(project_root.join("src/b.rs")).unwrap(),
            "old b\n"
        );
        assert!(
            applied["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|decision| decision["kind"] == "partial_apply"
                    && decision["files"].as_array().unwrap().len() == 1)
        );
        let remaining_files = applied["review"]["changed_files"].as_array().unwrap();
        assert_eq!(remaining_files.len(), 1);
        assert_eq!(remaining_files[0]["path"], "src/b.rs");
        let remaining_groups = applied["review"]["transaction_groups"].as_array().unwrap();
        assert_eq!(remaining_groups.len(), 1);
        assert_eq!(
            remaining_groups[0]["files"].as_array().unwrap()[0],
            "src/b.rs"
        );
    }

    #[test]
    fn quest_discard_prunes_selected_transaction_groups_without_mutating_project() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("src")).unwrap();
        fs::write(project_root.join("src/a.rs"), "old a\n").unwrap();
        fs::write(project_root.join("src/b.rs"), "old b\n").unwrap();
        let mut host = host_with_quest_root(temp.path());
        let changed_files = vec![
            ChangedFile {
                path: "src/a.rs".to_owned(),
                additions: 1,
                deletions: 1,
                status: "modified".to_owned(),
                diff: "a diff".to_owned(),
            },
            ChangedFile {
                path: "src/b.rs".to_owned(),
                additions: 1,
                deletions: 1,
                status: "modified".to_owned(),
                diff: "b diff".to_owned(),
            },
        ];
        let selected_group = transaction_groups_from_changed_files(&changed_files)[0]
            .id
            .clone();
        let (id, workspace_root) = create_reviewable_quest(&mut host, &project_root, changed_files);
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::write(workspace_root.join("src/a.rs"), "new a\n").unwrap();
        fs::write(workspace_root.join("src/b.rs"), "new b\n").unwrap();

        let discarded = host
            .handle(
                "quest/discard",
                &serde_json::json!({
                    "id": id,
                    "transaction_group_ids": [selected_group],
                }),
            )
            .unwrap();

        assert_eq!(discarded["status"], "ready_for_review");
        assert_eq!(
            fs::read_to_string(project_root.join("src/a.rs")).unwrap(),
            "old a\n"
        );
        assert_eq!(
            fs::read_to_string(project_root.join("src/b.rs")).unwrap(),
            "old b\n"
        );
        assert!(
            discarded["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|decision| decision["kind"] == "discard"
                    && decision["files"].as_array().unwrap().len() == 1)
        );
        let remaining_files = discarded["review"]["changed_files"].as_array().unwrap();
        assert_eq!(remaining_files.len(), 1);
        assert_eq!(remaining_files[0]["path"], "src/b.rs");
    }

    #[test]
    fn quest_discard_rejects_stale_review_when_active_project_changed_after_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("src")).unwrap();
        fs::write(project_root.join("src/main.rs"), "old\n").unwrap();
        let mut host = host_with_quest_root(temp.path());
        let changed_files = vec![ChangedFile {
            path: "src/main.rs".to_owned(),
            additions: 1,
            deletions: 1,
            status: "modified".to_owned(),
            diff: "main diff".to_owned(),
        }];
        let (id, workspace_root) = create_reviewable_quest(&mut host, &project_root, changed_files);
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::write(workspace_root.join("src/main.rs"), "quest output\n").unwrap();
        fs::write(project_root.join("src/main.rs"), "user edit\n").unwrap();

        let error = host
            .handle("quest/discard", &serde_json::json!({ "id": id }))
            .unwrap_err();

        assert!(error.to_string().contains("review is stale"));
        let detail = host.quest_store.get(&id).unwrap();
        assert!(detail.record.decisions.is_empty());
        assert_eq!(
            fs::read_to_string(project_root.join("src/main.rs")).unwrap(),
            "user edit\n"
        );
    }

    #[test]
    fn quest_discard_all_marks_reviewed_result_completed() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("src")).unwrap();
        fs::write(project_root.join("src/a.rs"), "old a\n").unwrap();
        let mut host = host_with_quest_root(temp.path());
        let changed_files = vec![ChangedFile {
            path: "src/a.rs".to_owned(),
            additions: 1,
            deletions: 1,
            status: "modified".to_owned(),
            diff: "a diff".to_owned(),
        }];
        let (id, workspace_root) = create_reviewable_quest(&mut host, &project_root, changed_files);
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::write(workspace_root.join("src/a.rs"), "new a\n").unwrap();

        let discarded = host
            .handle("quest/discard", &serde_json::json!({ "id": id }))
            .unwrap();

        assert_eq!(discarded["status"], "completed");
        assert_eq!(
            fs::read_to_string(project_root.join("src/a.rs")).unwrap(),
            "old a\n"
        );
        assert!(
            discarded["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|decision| decision["kind"] == "discard"
                    && decision["files"].as_array().unwrap().len() == 1)
        );
        let knowledge = host.quest_store.list_knowledge().unwrap();
        assert!(knowledge.iter().any(|entry| {
            entry.status == "pending"
                && entry.category == "quest-completion"
                && entry.source == id
                && entry.content.contains("intentionally discarding")
        }));
    }

    #[test]
    fn quest_export_publishes_selected_artifacts_under_project_local_aster_directory() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).unwrap();
        let mut host = host_with_quest_root(temp.path());
        let created = host
            .quest_store
            .create(
                "Export Quest".to_owned(),
                "Publish task artifacts for review.".to_owned(),
                "# Export Quest\n\n## Goal\n\nPublish task artifacts for review.".to_owned(),
                quest_project(&project_root),
            )
            .unwrap();

        let exported = host
            .handle(
                "quest/export",
                &serde_json::json!({ "id": created.record.id }),
            )
            .unwrap();
        let export_root = project_root
            .join(".aster")
            .join("quests")
            .join(created.record.id);

        assert!(export_root.join("quest.json").is_file());
        assert!(export_root.join("intent.md").is_file());
        assert!(export_root.join("spec.md").is_file());
        assert!(export_root.join("events.jsonl").is_file());
        assert!(
            exported["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|decision| decision["kind"] == "export")
        );
    }

    #[test]
    fn quest_read_artifact_serves_quest_files_and_rejects_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).unwrap();
        let mut host = host_with_quest_root(temp.path());
        let created = host
            .quest_store
            .create(
                "Read Artifact Quest".to_owned(),
                "Read a durable artifact.".to_owned(),
                "# Read Artifact Quest\n\n## Goal\n\nRead a durable artifact.".to_owned(),
                quest_project(&project_root),
            )
            .unwrap();
        host.quest_store
            .write_thinking_trace(
                &created.record.id,
                "initial-plan",
                "Initial planning model trace",
                "Visible provider thinking.",
            )
            .unwrap();

        let artifact = host
            .handle(
                "quest/read_artifact",
                &serde_json::json!({
                    "id": created.record.id,
                    "path": "thinking/initial-plan.md",
                }),
            )
            .unwrap();
        assert!(
            artifact["content"]
                .as_str()
                .unwrap()
                .contains("Visible provider thinking")
        );

        let error = host
            .handle(
                "quest/read_artifact",
                &serde_json::json!({
                    "id": created.record.id,
                    "path": "../knowledge.json",
                }),
            )
            .unwrap_err();
        assert!(error.to_string().contains("inside the project"));
    }
}
