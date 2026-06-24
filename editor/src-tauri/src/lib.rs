//! Tauri backend for the Varg Editor.
//!
//! Single `rpc` command that dispatches to EditorHost methods,
//! mirroring the original stdin/stdout JSON-RPC protocol.

use std::{
    cell::UnsafeCell,
    collections::{BTreeMap, HashMap},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Mutex,
    thread::ThreadId,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::Engine as _;
use engine_ai::{AgentOutcome, AgentPlan, AgentSession};
use engine_core::{EngineConfig, EngineError, EngineResult, RuntimeProfile};
use engine_editor::agent::{PermissionPolicy, SandboxPolicy, TraceEntry};
use engine_editor::{
    ConsoleEntry, ConsoleLevel, ConsoleService, DurableEditorState, EditorPreferences,
    FileEditorStore, ProjectMetadata, ThemePreference, UndoCommand,
};
use engine_editor::{EditorShell, HubState, ProjectDeletionDecision, ProjectDeletionMode};
use engine_i18n::{Locale, Translations};
use engine_packager::{
    PackageChannel, PackageFormat, PackageRequest, PackageTarget, package_project,
};
use engine_render::ImageFormat;
use engine_render_wgpu::{WgpuOffscreenConfig, WgpuRenderDevice};
use runtime_min::{RuntimeServices, headless_services_from_scene};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Manager, State, WebviewWindowBuilder, image::Image, utils::config::Color};

mod editor_compositor;
mod game_window;
mod native_host_window;
mod native_panel_host;
mod quest;
mod scene_window;
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
    ChangedFile, QuestExplorationAttempt, QuestMode, QuestModelConfig, QuestProject, QuestReview,
    QuestReviewAction, QuestReviewFinding, QuestReviewMetrics, QuestStatus, QuestStore, QuestTask,
    ValidationResult, transaction_groups_from_changed_files,
};

const APP_WINDOW_ICON: Image<'static> = tauri::include_image!("./icons/128x128.png");

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
        quest_validation_registry()
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
    device_auth_id: String,
    user_code: String,
    interval_seconds: u64,
}

#[derive(Debug)]
struct GeneratedQuestSpec {
    title: String,
    spec: String,
    tasks: Vec<GeneratedQuestTask>,
    question_cards: Vec<GeneratedQuestionCard>,
}

#[derive(Debug)]
struct GeneratedQuestTask {
    title: String,
    summary: Option<String>,
    acceptance: Vec<String>,
}

#[derive(Debug)]
struct GeneratedQuestionCard {
    title: String,
    questions: Vec<GeneratedQuestion>,
}

#[derive(Debug)]
struct GeneratedQuestion {
    id: String,
    prompt: String,
    options: Vec<GeneratedQuestionOption>,
    allow_multiple: bool,
    allow_custom: bool,
}

#[derive(Debug)]
struct GeneratedQuestionOption {
    id: String,
    label: String,
    description: Option<String>,
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
    knowledge_entries_used: usize,
}

struct CompletedCopilotRequest {
    original_prompt: String,
    response: Result<String, String>,
    tool_calls: Vec<engine_ai::ToolCall>,
    cached_context: engine_editor::ProjectContext,
    knowledge_entries_used: usize,
}

struct PreparedQuestModelRequest {
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

struct PreparedQuestCreateRequest {
    model_request: PreparedQuestModelRequest,
    title: String,
    goal: String,
    project: QuestProject,
    mode: QuestMode,
    model_config: QuestModelConfig,
}

enum PreparedQuestAiRequest {
    Create(PreparedQuestCreateRequest),
    Rewrite(PreparedQuestModelRequest),
}

enum CompletedQuestAiRequest {
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

struct PreparedQuestExecution {
    quest_store: QuestStore,
    quest_id: String,
    model_provider: PreparedQuestModelRequest,
}

#[derive(Default)]
struct QuestExecutionRequests {
    completed: HashMap<String, Result<Value, String>>,
    cancelled: std::collections::HashSet<String>,
}

#[derive(Clone, Default)]
struct QuestExecutionRequestState {
    requests: std::sync::Arc<Mutex<QuestExecutionRequests>>,
}

#[derive(Default)]
struct QuestAiRequests {
    completed: HashMap<String, CompletedQuestAiRequest>,
    cancelled: std::collections::HashSet<String>,
}

#[derive(Clone, Default)]
struct QuestAiRequestState {
    requests: std::sync::Arc<Mutex<QuestAiRequests>>,
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

fn elapsed_millis(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis() as u64
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
    /// Active device authorization request.
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

    fn quest_list(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({ "quests": self.quest_store.list()? }))
    }

    fn quest_get(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        serde_json::to_value(self.quest_store.get(id)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_read_artifact(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let path = required_string(params, "path")?;
        let relative = normalize_relative_path(path)?;
        let root = self.quest_store.quest_path(id)?;
        let full_path = root.join(relative);
        let content =
            std::fs::read_to_string(&full_path).map_err(|source| EngineError::Filesystem {
                path: full_path.clone(),
                source,
            })?;
        Ok(serde_json::json!({ "content": content }))
    }

    fn prepare_quest_create_request(
        &mut self,
        params: &Value,
    ) -> EngineResult<PreparedQuestCreateRequest> {
        let title = params
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let goal = required_string(params, "goal")?.trim().to_owned();
        if goal.is_empty() {
            return Err(EngineError::config("Quest goal must not be empty"));
        }
        let mode = parse_quest_mode(params.get("mode"))?;
        let model_config = self.quest_model_config_from_params(params)?;
        let project = {
            let project = self
                .shell
                .project()
                .ok_or_else(|| EngineError::config("no project open"))?;
            QuestProject {
                name: project.name().to_owned(),
                path: project.root.clone(),
            }
        };
        let mut request = engine_ai::AiRequest::single_turn(
            "You are Varg Quest Mode. Create only the initial editable Markdown spec for an AI-led game-editor Quest. Prefer calling `create_or_update_spec` once. If the user's goal is underspecified and the missing choice materially changes the plan, call `ask_questions` to create an interactive question card instead of writing questions in prose. If tool calling is unavailable or awkward, return the editable Markdown spec directly as normal text. Do not create execution tasks yet; tasks are planned later after the user reviews and updates the spec. Do not force a generic workflow; choose the spec shape that best fits the user's goal.".to_owned(),
            serde_json::json!({}),
            format!("Quest goal:\n{goal}"),
        );
        request.tools = quest_creation_tool_definitions();
        let model_request = self.prepare_quest_model_request(&model_config, request)?;
        Ok(PreparedQuestCreateRequest {
            model_request,
            title,
            goal,
            project,
            mode,
            model_config,
        })
    }

    fn finish_quest_create(
        &mut self,
        generated: GeneratedQuestSpec,
        title: String,
        goal: String,
        project: QuestProject,
        mode: QuestMode,
        model_config: QuestModelConfig,
    ) -> EngineResult<Value> {
        let title = if title.is_empty() {
            generated.title
        } else {
            title
        };
        let detail = self.quest_store.create_with_config(
            title,
            goal,
            generated.spec,
            project,
            mode,
            model_config,
        )?;
        let has_question_cards = append_generated_question_cards(
            &self.quest_store,
            &detail.record.id,
            generated.question_cards,
        )?;
        let detail = if has_question_cards {
            self.quest_store
                .transition(&detail.record.id, QuestStatus::Clarifying)?
        } else {
            self.quest_store.get(&detail.record.id)?
        };
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_create_openai_realtime_transcription_session(
        &self,
        _params: &Value,
    ) -> EngineResult<Value> {
        if !matches!(
            self.copilot_settings.provider,
            engine_editor::CopilotProvider::OpenAI
        ) {
            return Err(EngineError::config(
                "Quest voice input requires the OpenAI API provider.",
            ));
        }
        let api_key = self.copilot_settings.api_key.as_deref().ok_or_else(|| {
            EngineError::config("OpenAI API key is required for Quest voice input")
        })?;
        let endpoint = self
            .copilot_settings
            .api_endpoint
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/');
        let url = format!("{endpoint}/realtime/client_secrets");
        let body = serde_json::json!({
            "session": {
                "type": "transcription",
                "audio": {
                    "input": {
                        "transcription": {
                            "model": "gpt-realtime-whisper",
                            "delay": "low"
                        }
                    }
                }
            }
        });
        let mut response = ureq::post(&url)
            .header("Authorization", &format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|error| {
                EngineError::other(format!(
                    "OpenAI Realtime transcription session failed: {error}"
                ))
            })?;
        let json: Value = response.body_mut().read_json().map_err(|error| {
            EngineError::other(format!(
                "OpenAI Realtime transcription session response parse failed: {error}"
            ))
        })?;
        Ok(serde_json::json!({
            "session": json,
            "model": "gpt-realtime-whisper",
            "endpoint": endpoint,
            "realtime_url": format!("{endpoint}/realtime/calls"),
        }))
    }

    fn openai_realtime_transcription_config(&self) -> EngineResult<(String, String)> {
        if !matches!(
            self.copilot_settings.provider,
            engine_editor::CopilotProvider::OpenAI
        ) {
            return Err(EngineError::config(
                "Quest voice input requires the OpenAI API provider.",
            ));
        }
        let api_key = self.copilot_settings.api_key.clone().ok_or_else(|| {
            EngineError::config("OpenAI API key is required for Quest voice input")
        })?;
        let endpoint = self
            .copilot_settings
            .api_endpoint
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_owned();
        Ok((api_key, endpoint))
    }

    fn prepare_quest_rewrite_request(
        &mut self,
        params: &Value,
    ) -> EngineResult<PreparedQuestModelRequest> {
        let prompt = required_string(params, "prompt")?.trim();
        if prompt.is_empty() {
            return Err(EngineError::config("Prompt must not be empty"));
        }
        let model_config = self.quest_model_config_from_params(params)?;
        let request = engine_ai::AiRequest {
            system: "You rewrite rough Quest prompts into clear, actionable game-engine development tasks. Return only the rewritten prompt. Do not add markdown fences, titles, commentary, or multiple options.".to_owned(),
            context: serde_json::json!({}),
            messages: vec![engine_ai::ChatMessage::user(format!(
                "Rewrite this Quest prompt so an autonomous coding agent can execute it. Preserve the user's intent, concrete nouns, language, and constraints. Make it concise but specific.\n\nPrompt:\n{prompt}"
            ))],
            thinking_effort: parse_thinking_effort(&model_config.thinking_effort),
            tools: Vec::new(),
        };
        self.prepare_quest_model_request(&model_config, request)
    }

    fn finish_quest_rewrite(&mut self, response: String) -> EngineResult<Value> {
        let rewritten = response.trim().trim_matches('"').trim().to_owned();
        if rewritten.is_empty() {
            return Err(EngineError::other(
                "Prompt rewrite returned an empty result",
            ));
        }
        Ok(serde_json::json!({ "prompt": rewritten }))
    }

    fn quest_promote(&mut self, params: &Value) -> EngineResult<Value> {
        let prompt = required_string(params, "prompt")?.trim();
        if prompt.is_empty() {
            return Err(EngineError::config(
                "Promoted Quest prompt must not be empty",
            ));
        }
        let context = params
            .get("context")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let goal = if context.is_empty() {
            prompt.to_owned()
        } else {
            format!("{prompt}\n\nPromoted Editor context:\n{context}")
        };
        let generated = self.generate_quest_spec(&goal)?;
        let model_config = self.default_quest_model_config();
        let project = self
            .shell
            .project()
            .ok_or_else(|| EngineError::config("no project open"))?;
        let detail = self.quest_store.create_with_config(
            generated.title,
            goal.clone(),
            generated.spec,
            QuestProject {
                name: project.name().to_owned(),
                path: project.root.clone(),
            },
            QuestMode::Solo,
            model_config,
        )?;
        if !context.is_empty() {
            let promoted_intent = format!(
                "# {}\n\n## Goal\n\n{}\n\n## Promoted Editor Context\n\n{}\n",
                detail.record.title, prompt, context
            );
            self.quest_store
                .update_intent(&detail.record.id, &promoted_intent)?;
            self.quest_store.append_timeline_event(
                &detail.record.id,
                "context_attached",
                "Promoted Editor context into Quest intent",
                serde_json::json!({ "context_bytes": context.len() }),
            )?;
        }
        for task in generated.tasks {
            self.quest_store.append_timeline_event(
                &detail.record.id,
                "task_created",
                &task.title,
                serde_json::json!({
                    "summary": task.summary,
                    "acceptance": task.acceptance,
                    "source": "promoted_editor_context",
                }),
            )?;
        }
        let has_question_cards = append_generated_question_cards(
            &self.quest_store,
            &detail.record.id,
            generated.question_cards,
        )?;
        let detail = if has_question_cards {
            self.quest_store
                .transition(&detail.record.id, QuestStatus::Clarifying)?
        } else {
            self.quest_store.get(&detail.record.id)?
        };
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    fn generate_quest_spec(&mut self, goal: &str) -> EngineResult<GeneratedQuestSpec> {
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
                    "Quest creation requires a configured AI provider because the Quest spec and execution plan are AI-generated. Go to Settings → Copilot to configure an API key, OAuth provider, Ollama, or a custom endpoint.",
                ));
            }
        };
        let codex_oauth = if provider_str == "codex_oauth" {
            Some(self.ensure_codex_oauth()?)
        } else {
            None
        };
        let model = engine_ai::providers::create_provider(
            provider_str,
            &self.copilot_settings.model,
            self.copilot_settings.api_key.as_deref(),
            if self.copilot_settings.provider.endpoint_configurable() {
                self.copilot_settings.api_endpoint.as_deref()
            } else {
                None
            },
            self.copilot_settings.max_tokens,
            codex_oauth,
            if provider_str == "mimo" {
                Some(&self.copilot_settings.mimo_config)
            } else {
                None
            },
            if provider_str == "glm" {
                Some(&self.copilot_settings.glm_config)
            } else {
                None
            },
        )?;
        let mut request = engine_ai::AiRequest::single_turn(
            "You are Varg Quest Mode. Create only the initial editable Markdown spec for an AI-led game-editor Quest. Prefer calling `create_or_update_spec` once. If the user's goal is underspecified and the missing choice materially changes the plan, call `ask_questions` to create an interactive question card instead of writing questions in prose. If tool calling is unavailable or awkward, return the editable Markdown spec directly as normal text. Do not create execution tasks yet; tasks are planned later after the user reviews and updates the spec. Do not force a generic workflow; choose the spec shape that best fits the user's goal.".to_owned(),
            serde_json::json!({}),
            format!("Quest goal:\n{goal}"),
        );
        request.tools = quest_creation_tool_definitions();
        let response = model.chat(request)?;
        parse_generated_quest_response(&response.tool_calls, &response.content, goal)
    }

    fn quest_update_spec(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let spec = required_string(params, "spec")?;
        serde_json::to_value(self.quest_store.update_spec(id, spec)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_update_tasks(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let tasks_value = params
            .get("tasks")
            .cloned()
            .ok_or_else(|| EngineError::config("missing 'tasks' parameter"))?;
        let tasks: Vec<QuestTask> = serde_json::from_value(tasks_value)
            .map_err(|error| EngineError::config(format!("invalid Quest tasks: {error}")))?;
        serde_json::to_value(self.quest_store.replace_tasks(id, tasks)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_update_execution_config(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let mode = parse_quest_mode(params.get("mode"))?;
        let model_config = self.quest_model_config_from_params(params)?;
        let autonomy = params
            .get("autonomy")
            .map(|value| {
                serde_json::from_value(value.clone()).map_err(|error| {
                    EngineError::config(format!("invalid Quest autonomy config: {error}"))
                })
            })
            .transpose()?;
        serde_json::to_value(self.quest_store.update_execution_config(
            id,
            mode,
            model_config,
            autonomy,
        )?)
        .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_update_knowledge_context(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let knowledge_ids = params
            .get("knowledge_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| EngineError::config("missing 'knowledge_ids'"))?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        serde_json::to_value(
            self.quest_store
                .update_knowledge_context(id, knowledge_ids)?,
        )
        .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_update_intent(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let intent = required_string(params, "intent")?;
        serde_json::to_value(self.quest_store.update_intent(id, intent)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_add_note(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let kind = params
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("message");
        let message = required_string(params, "message")?;
        serde_json::to_value(self.quest_store.add_user_note(id, kind, message)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_request_quick_fix(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let issue = required_string(params, "issue")?;
        serde_json::to_value(self.quest_store.request_quick_fix(id, issue)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_rename(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let title = required_string(params, "title")?;
        serde_json::to_value(self.quest_store.rename(id, title)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_branch(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let title = params.get("title").and_then(Value::as_str);
        serde_json::to_value(self.quest_store.branch(id, title)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_transition(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let status: QuestStatus = serde_json::from_value(
            params
                .get("status")
                .cloned()
                .ok_or_else(|| EngineError::config("missing 'status'"))?,
        )
        .map_err(|error| EngineError::config(error.to_string()))?;
        serde_json::to_value(self.quest_store.transition(id, status)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_delete(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        self.quest_store.delete(id)?;
        Ok(serde_json::json!({ "deleted": true }))
    }

    fn knowledge_list(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({ "entries": self.quest_store.list_knowledge()? }))
    }

    fn knowledge_propose(&mut self, params: &Value) -> EngineResult<Value> {
        let category = required_string(params, "category")?;
        let content = required_string(params, "content")?;
        let source = params
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("manual");
        Ok(serde_json::json!({
            "entries": self.quest_store.propose_knowledge(category, content, source)?
        }))
    }

    fn knowledge_approve(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        Ok(serde_json::json!({ "entries": self.quest_store.approve_knowledge(id)? }))
    }

    fn knowledge_reject(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        Ok(serde_json::json!({ "entries": self.quest_store.reject_knowledge(id)? }))
    }

    fn knowledge_revalidate(&mut self, _params: &Value) -> EngineResult<Value> {
        Ok(serde_json::json!({ "entries": self.quest_store.revalidate_knowledge()? }))
    }

    fn knowledge_remove(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        Ok(serde_json::json!({ "entries": self.quest_store.remove_knowledge(id)? }))
    }

    fn quest_execute(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?.to_owned();
        let started_at = Instant::now();
        let prepared = self.prepare_quest_execution(&id)?;
        match run_quest_execution(prepared) {
            Ok(value) => Ok(value),
            Err(error) => record_quest_execution_failure(&self.quest_store, &id, started_at, error),
        }
    }

    fn prepare_quest_execution(&mut self, id: &str) -> EngineResult<PreparedQuestExecution> {
        let detail = self.quest_store.get(id)?;
        let model_provider = self.prepare_quest_model_request(
            &detail.record.model_config,
            engine_ai::AiRequest::single_turn(String::new(), serde_json::json!({}), String::new()),
        )?;
        Ok(PreparedQuestExecution {
            quest_store: self.quest_store.clone(),
            quest_id: id.to_owned(),
            model_provider,
        })
    }
}

fn record_quest_execution_failure(
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
    if let Some(parent) = workspace_root.parent() {
        std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    if try_create_git_worktree(&detail.record.project.path, &workspace_root)? {
        return Ok(workspace_root);
    }
    copy_project_tree(&detail.record.project.path, &workspace_root)?;
    Ok(workspace_root)
}

fn run_quest_execution(prepared: PreparedQuestExecution) -> EngineResult<Value> {
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

impl EditorHost {
    fn quest_mock_execute(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        serde_json::to_value(self.quest_store.mock_execute(id)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_cancel(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Canceled Quest");
        serde_json::to_value(self.quest_store.cancel(id, reason)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_reopen(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Reopened Quest");
        serde_json::to_value(self.quest_store.reopen(id, reason)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_continue(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Continue Quest from current evidence");
        serde_json::to_value(self.quest_store.continue_quest(id, reason)?)
            .map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_apply(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let detail = self.quest_store.get(id)?;
        if detail.record.status != QuestStatus::ReadyForReview {
            return Err(EngineError::config("Quest must be in review before apply"));
        }
        let review = detail
            .record
            .review
            .as_ref()
            .ok_or_else(|| EngineError::config("Quest has no review bundle"))?;
        ensure_review_project_is_current(review, &detail.record.project.path)?;
        let workspace_id = detail
            .record
            .workspace_id
            .as_deref()
            .ok_or_else(|| EngineError::config("Quest has no workspace"))?;
        let workspace_root = self
            .quest_store
            .quest_path(id)?
            .join("workspaces")
            .join(workspace_id);
        if !workspace_root.is_dir() {
            return Err(EngineError::config("Quest workspace is missing"));
        }

        let changed_files = selected_review_paths_from_params(review, params, "apply")?;
        let selected_paths: std::collections::HashSet<&str> =
            changed_files.iter().map(String::as_str).collect();
        let selected_review_files = review
            .changed_files
            .iter()
            .filter(|file| selected_paths.contains(file.path.as_str()))
            .collect::<Vec<_>>();
        if selected_review_files.len() != changed_files.len() {
            return Err(EngineError::config(
                "selected Quest file is not present in the review bundle",
            ));
        }

        let project_root = detail.record.project.path.clone();
        let mut applied = Vec::new();
        let rollback_id = format!("rollback-{}", unix_time_ms());
        let rollback_root = self
            .quest_store
            .quest_path(id)?
            .join("rollbacks")
            .join(&rollback_id);
        for file in selected_review_files {
            let relative = normalize_relative_path(&file.path)?;
            let source = workspace_root.join(&relative);
            let destination = project_root.join(&relative);
            snapshot_rollback_file(&rollback_root, &relative, &destination)?;
            if file.status == "deleted" {
                if destination.exists() {
                    std::fs::remove_file(&destination).map_err(|source| {
                        EngineError::Filesystem {
                            path: destination.clone(),
                            source,
                        }
                    })?;
                }
            } else {
                if !source.is_file() {
                    return Err(EngineError::config(format!(
                        "changed file is missing from Quest workspace: {}",
                        file.path
                    )));
                }
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                std::fs::copy(&source, &destination).map_err(|source| EngineError::Filesystem {
                    path: destination.clone(),
                    source,
                })?;
            }
            applied.push(file.path.clone());
        }

        let total_changed = review.changed_files.len();
        let partial = applied.len() < total_changed;
        let summary = if partial {
            format!(
                "Partially applied {} of {} reviewed Quest file(s)",
                applied.len(),
                total_changed
            )
        } else {
            "Applied reviewed Quest bundle to active project".to_owned()
        };
        let applied_paths = applied
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let _ = self.quest_store.record_decision_with_rollback(
            id,
            if partial { "partial_apply" } else { "apply" },
            &summary,
            applied.clone(),
            Some(rollback_id.clone()),
        )?;
        let detail = if partial {
            let mut remaining_review = review.clone();
            remaining_review
                .changed_files
                .retain(|file| !applied_paths.contains(&file.path));
            for group in &mut remaining_review.transaction_groups {
                group.files.retain(|path| !applied_paths.contains(path));
            }
            remaining_review
                .transaction_groups
                .retain(|group| !group.files.is_empty());
            remaining_review.summary = format!(
                "{} {} reviewed file(s) remain pending.",
                summary,
                remaining_review.changed_files.len()
            );
            remaining_review.project_fingerprint = Some(project_fingerprint(&project_root)?);
            self.quest_store
                .set_review(id, QuestStatus::ReadyForReview, remaining_review)?
        } else {
            self.quest_store.transition(id, QuestStatus::Applying)?;
            let detail = self.quest_store.transition(id, QuestStatus::Completed)?;
            let _ = self.quest_store.propose_knowledge(
                "quest-completion",
                &format!(
                    "{} completed with {} applied file(s). Review validations before reusing this as project knowledge.",
                    detail.record.title,
                    detail
                        .record
                        .decisions
                        .last()
                        .map(|decision| decision.files.len())
                        .unwrap_or_default()
                ),
                id,
            );
            detail
        };
        self.quest_store.append_timeline_event(
            id,
            "apply_result",
            &summary,
            serde_json::json!({ "partial": partial }),
        )?;
        if detail.record.project.path == project_root {
            let _ = self.hub_open_project(&serde_json::json!({ "path": project_root }));
        }
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_discard(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let detail = self.quest_store.get(id)?;
        if detail.record.status != QuestStatus::ReadyForReview {
            return Err(EngineError::config(
                "Quest must be in review before discarding pending items",
            ));
        }
        let review = detail
            .record
            .review
            .as_ref()
            .ok_or_else(|| EngineError::config("Quest has no review bundle"))?;
        ensure_review_project_is_current(review, &detail.record.project.path)?;
        let discarded = selected_review_paths_from_params(review, params, "discard")?;
        let discarded_paths = discarded
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();

        let total_changed = review.changed_files.len();
        let partial = discarded.len() < total_changed;
        let summary = if partial {
            format!(
                "Discarded {} of {} pending Quest file(s)",
                discarded.len(),
                total_changed
            )
        } else {
            "Discarded remaining Quest review bundle".to_owned()
        };
        let _ = self
            .quest_store
            .record_decision(id, "discard", &summary, discarded.clone())?;
        let detail = if partial {
            let mut remaining_review = review.clone();
            remaining_review
                .changed_files
                .retain(|file| !discarded_paths.contains(&file.path));
            for group in &mut remaining_review.transaction_groups {
                group.files.retain(|path| !discarded_paths.contains(path));
            }
            remaining_review
                .transaction_groups
                .retain(|group| !group.files.is_empty());
            remaining_review.summary = format!(
                "{} {} reviewed file(s) remain pending.",
                summary,
                remaining_review.changed_files.len()
            );
            self.quest_store
                .set_review(id, QuestStatus::ReadyForReview, remaining_review)?
        } else {
            let detail = self.quest_store.transition(id, QuestStatus::Completed)?;
            let _ = self.quest_store.propose_knowledge(
                "quest-completion",
                &format!(
                    "{} completed by intentionally discarding {} reviewed file(s). Preserve this as a review decision before reusing the Quest result.",
                    detail.record.title,
                    discarded.len()
                ),
                id,
            );
            detail
        };
        self.quest_store.append_timeline_event(
            id,
            "discard_result",
            &summary,
            serde_json::json!({ "partial": partial, "files": discarded }),
        )?;
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_rollback(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let rollback_id = required_string(params, "rollback_id")?;
        let detail = self.quest_store.get(id)?;
        let decision = detail
            .record
            .decisions
            .iter()
            .find(|decision| decision.rollback_id.as_deref() == Some(rollback_id))
            .ok_or_else(|| EngineError::config("rollback snapshot is not linked to this Quest"))?;
        let rollback_root = self
            .quest_store
            .quest_path(id)?
            .join("rollbacks")
            .join(rollback_id);
        if !rollback_root.is_dir() {
            return Err(EngineError::config("rollback snapshot is missing"));
        }
        restore_rollback_files(&rollback_root, &detail.record.project.path, &decision.files)?;
        let files = decision.files.clone();
        let detail = self.quest_store.record_decision(
            id,
            "rollback",
            "Rolled back applied Quest files",
            files.clone(),
        )?;
        self.quest_store.append_timeline_event(
            id,
            "rollback",
            "Rolled back applied Quest files",
            serde_json::json!({ "rollback_id": rollback_id, "files": files }),
        )?;
        let _ = self
            .hub_open_project(&serde_json::json!({ "path": detail.record.project.path.clone() }));
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_export(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let detail = self.quest_store.get(id)?;
        let quest_dir = self.quest_store.quest_path(id)?;
        let export_root = detail
            .record
            .project
            .path
            .join(".aster")
            .join("quests")
            .join(id);
        std::fs::create_dir_all(&export_root).map_err(|source| EngineError::Filesystem {
            path: export_root.clone(),
            source,
        })?;
        for file_name in ["quest.json", "intent.md", "spec.md", "events.jsonl"] {
            let source = quest_dir.join(file_name);
            if source.is_file() {
                std::fs::copy(&source, export_root.join(file_name)).map_err(|source| {
                    EngineError::Filesystem {
                        path: export_root.join(file_name),
                        source,
                    }
                })?;
            }
        }
        let relative_export = format!(".aster/quests/{id}");
        let detail = self.quest_store.record_decision(
            id,
            "export",
            &format!("Exported Quest artifacts to {relative_export}"),
            vec![relative_export.clone()],
        )?;
        self.quest_store.append_timeline_event(
            id,
            "exported",
            "Exported Quest artifacts to project",
            serde_json::json!({ "path": relative_export }),
        )?;
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_reject(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Rejected reviewed Quest result");
        let _ = self
            .quest_store
            .record_decision(id, "reject", reason, Vec::new())?;
        let detail = self.quest_store.transition(id, QuestStatus::Archived)?;
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    fn quest_request_revision(&mut self, params: &Value) -> EngineResult<Value> {
        let id = required_string(params, "id")?;
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("Requested Quest revision");
        let _ = self
            .quest_store
            .record_decision(id, "revise", reason, Vec::new())?;
        let detail = self.quest_store.transition(id, QuestStatus::Specified)?;
        serde_json::to_value(detail).map_err(|error| EngineError::other(error.to_string()))
    }

    fn prepare_quest_model_request(
        &mut self,
        config: &QuestModelConfig,
        request: engine_ai::AiRequest,
    ) -> EngineResult<PreparedQuestModelRequest> {
        let provider = if config.provider == "inherit" {
            copilot_provider_str(&self.copilot_settings.provider)?.to_owned()
        } else if config.provider == "stub" {
            return Err(EngineError::config(
                "Quest execution requires a configured AI provider.",
            ));
        } else {
            config.provider.clone()
        };
        let provider_str = provider.as_str();
        let model = if config.model.trim().is_empty() {
            self.copilot_settings.model.clone()
        } else {
            config.model.clone()
        };
        let max_tokens = config.max_tokens.max(1);
        let endpoint = config.api_endpoint.clone().or_else(|| {
            if config.provider == "inherit"
                && self.copilot_settings.provider.endpoint_configurable()
            {
                self.copilot_settings.api_endpoint.clone()
            } else {
                None
            }
        });
        let codex_oauth = if provider_str == "codex_oauth" {
            Some(self.ensure_codex_oauth()?)
        } else {
            None
        };
        let mimo_config =
            (provider_str == "mimo").then(|| self.copilot_settings.mimo_config.clone());
        let glm_config = (provider_str == "glm").then(|| self.copilot_settings.glm_config.clone());
        Ok(PreparedQuestModelRequest {
            request,
            provider,
            model,
            api_key: self.copilot_settings.api_key.clone(),
            endpoint,
            max_tokens,
            codex_oauth,
            mimo_config,
            glm_config,
        })
    }

    fn default_quest_model_config(&self) -> QuestModelConfig {
        QuestModelConfig {
            provider: copilot_provider_str(&self.copilot_settings.provider)
                .unwrap_or("inherit")
                .to_owned(),
            model: self.copilot_settings.model.clone(),
            api_endpoint: if self.copilot_settings.provider.endpoint_configurable() {
                self.copilot_settings.api_endpoint.clone()
            } else {
                None
            },
            max_tokens: self.copilot_settings.max_tokens,
            thinking_effort: "medium".to_owned(),
        }
    }

    fn quest_model_config_from_params(&self, params: &Value) -> EngineResult<QuestModelConfig> {
        let mut config = self.default_quest_model_config();
        if let Some(value) = params.get("model_config") {
            config = serde_json::from_value(value.clone()).map_err(|error| {
                EngineError::config(format!("invalid Quest model config: {error}"))
            })?;
        }
        Ok(config)
    }

    fn app_open_folder(&mut self, params: &Value) -> EngineResult<Value> {
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
            "locale": locale_code(self.hub.preferences().locale),
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

    fn hub_get_translations(&mut self, params: &Value) -> EngineResult<Value> {
        let requested_locale = params.get("locale").and_then(Value::as_str);
        let translations;
        let active_translations = if requested_locale.is_some() {
            translations = Translations::load(parse_locale(requested_locale));
            &translations
        } else {
            &self.translations
        };
        let entries: Vec<serde_json::Value> = active_translations
            .entries()
            .into_iter()
            .map(|(k, v)| serde_json::json!({ "key": k, "value": v }))
            .collect();
        Ok(serde_json::json!({
            "locale": locale_code(active_translations.locale()),
            "entries": entries,
        }))
    }

    fn hub_set_locale(&mut self, params: &Value) -> EngineResult<Value> {
        let locale_str = params
            .get("locale")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'locale' parameter"))?;
        let locale = parse_locale(Some(locale_str));
        self.hub.set_locale(locale);
        // Reload translations for the new locale
        self.translations = Translations::load(locale);
        self.sync_durable_state();
        Ok(serde_json::json!({ "locale": locale_code(locale) }))
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
        validate_file_name(name)?;
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };

        // Use the asset root relative to project root
        let asset_root = project.root.join(&project.manifest.asset_root);
        std::fs::create_dir_all(&asset_root).map_err(|source| EngineError::Filesystem {
            path: asset_root.clone(),
            source,
        })?;

        let script_path = format!("scripts/{name}.varg");
        let full_path = asset_root.join(&script_path);

        let template = format!(
            r#"script {name} {{
    @export var speed: Float = 6.0

    func start() {{
        log("{name} ready")
    }}

    func update(_ dt: Float) {{
    }}
}}
"#
        );

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

    fn project_create_material(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let content = varg_material_template(name);
        let (asset_path, full_path) =
            write_project_asset(project, &format!("materials/{name}.vasset"), &content)?;
        project.rescan_assets()?;
        push_created_asset_console(&mut self.console, "material", &full_path);

        Ok(serde_json::json!({
            "path": asset_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    fn project_create_prefab(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let content = varg_prefab_template(name);
        let (asset_path, full_path) =
            write_project_asset(project, &format!("prefabs/{name}.vscene"), &content)?;
        project.rescan_assets()?;
        push_created_asset_console(&mut self.console, "prefab", &full_path);

        Ok(serde_json::json!({
            "path": asset_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    fn project_create_scene(&mut self, params: &Value) -> EngineResult<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'name'"))?;
        validate_file_name(name)?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };

        let content = varg_scene_template(name);
        let (asset_path, full_path) =
            write_project_asset(project, &format!("scenes/{name}.vscene"), &content)?;
        project.rescan_assets()?;
        push_created_asset_console(&mut self.console, "scene", &full_path);

        Ok(serde_json::json!({
            "path": asset_path,
            "full_path": full_path.to_string_lossy(),
        }))
    }

    fn project_list_asset_references(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EngineError::config("missing 'path'"))?;

        let Some(project) = self.shell.project_mut() else {
            return Err(EngineError::config("no project open"));
        };
        project.rescan_assets()?;

        let asset_path = normalize_relative_path(path_str)?;
        let guid = project
            .database
            .guid_for_path(&asset_path)
            .map_err(EngineError::from)?;
        let mut rows = Vec::new();

        for dependency in project.database.dependencies().dependencies(guid) {
            rows.push(asset_reference_row(
                "dependency",
                "Asset dependency",
                resolve_asset_reference_label(project, dependency),
            ));
        }
        for dependent in project.database.dependencies().dependents(guid) {
            rows.push(asset_reference_row(
                "dependent",
                "Used by asset",
                resolve_asset_reference_label(project, dependent),
            ));
        }

        for (_entity, object) in project.scene.objects() {
            for component in &object.components {
                collect_component_asset_references(&mut rows, &object.name, component, guid);
                if let engine_ecs::ComponentData::Script(script) = component {
                    if script.source == path_str {
                        rows.push(asset_reference_row(
                            "scene",
                            "Script component",
                            format!("{} -> {}", object.name, script.source),
                        ));
                    }
                }
            }
            for script in &object.scripts {
                if script.source == path_str {
                    rows.push(asset_reference_row(
                        "scene",
                        "Legacy script",
                        format!("{} -> {}", object.name, script.source),
                    ));
                }
            }
        }

        rows.sort_by(|left, right| {
            left["kind"]
                .as_str()
                .cmp(&right["kind"].as_str())
                .then_with(|| left["label"].as_str().cmp(&right["label"].as_str()))
                .then_with(|| left["detail"].as_str().cmp(&right["detail"].as_str()))
        });
        rows.dedup();

        Ok(serde_json::json!({
            "guid": guid.to_string(),
            "path": asset_path.to_string_lossy(),
            "references": rows,
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

        let extension = full_path
            .extension()
            .and_then(|extension| extension.to_str());
        if matches!(extension, Some("varg" | "vscene" | "vasset")) {
            let diagnostics = engine_script_varg::diagnose_source(&full_path, content);
            if !diagnostics.is_empty() {
                return Err(EngineError::config(format_varg_diagnostics(
                    path_str,
                    &diagnostics,
                )));
            }
        }

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

    fn project_check_script(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let source = params
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::config("missing 'source'"))?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let asset_root = project.root.join(&project.manifest.asset_root);
        let full_path = resolve_writable_relative_path(&asset_root, path_str)?;
        let extension = full_path
            .extension()
            .and_then(|extension| extension.to_str());
        let (diagnostics, ast) = if matches!(extension, Some("varg" | "vscene" | "vasset")) {
            let (ast, diagnostics) = engine_script_varg::parse_source(&full_path, source);
            let diagnostics = diagnostics
                .into_iter()
                .map(|diagnostic| {
                    serde_json::json!({
                        "code": diagnostic.code,
                        "severity": match diagnostic.severity {
                            engine_script_varg::VargDiagnosticSeverity::Error => "error",
                            engine_script_varg::VargDiagnosticSeverity::Warning => "warning",
                        },
                        "line": diagnostic.line,
                        "column": diagnostic.column,
                        "message": diagnostic.message,
                        "suggestion": diagnostic.suggestion,
                        "source_line": diagnostic.source_line,
                    })
                })
                .collect::<Vec<_>>();
            let ast = ast
                .map(|ast| serde_json::to_value(ast).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            (diagnostics, ast)
        } else {
            (
                vec![serde_json::json!({
                    "code": "VARG0000",
                    "severity": "error",
                    "line": null,
                    "column": null,
                    "message": "unsupported script file extension",
                    "suggestion": "Use .varg for runtime scripts, .vscene for scenes, or .vasset for assets.",
                    "source_line": null,
                })],
                serde_json::Value::Null,
            )
        };
        Ok(serde_json::json!({
            "valid": diagnostics.is_empty(),
            "diagnostics": diagnostics,
            "ast": ast,
        }))
    }

    fn project_check_amdl(&mut self, params: &Value) -> EngineResult<Value> {
        let path_str = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::config("missing 'path'"))?;
        let source = params
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::config("missing 'source'"))?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let asset_root = project.root.join(&project.manifest.asset_root);
        let _full_path = resolve_writable_relative_path(&asset_root, path_str)?;

        let diagnostics = engine_assets::diagnose_amdl(source)
            .into_iter()
            .map(|diagnostic| serde_json::to_value(diagnostic).unwrap_or(Value::Null))
            .collect::<Vec<_>>();

        Ok(serde_json::json!({
            "valid": diagnostics.is_empty(),
            "diagnostics": diagnostics,
        }))
    }

    fn project_package(&mut self, params: &Value) -> EngineResult<Value> {
        let target = params
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("native");
        let format = params
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("folder");
        let channel = params
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or("release");
        let optimize_assets = params
            .get("optimize_assets")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let include_debug_symbols = params
            .get("include_debug_symbols")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let project_root = {
            let Some(project) = self.shell.project() else {
                return Err(EngineError::config("no project open"));
            };
            project.root.clone()
        };

        if self
            .shell
            .project()
            .is_some_and(|project| project.scene_dirty)
        {
            self.shell_save_scene(&serde_json::json!({}))?;
        }

        let output = package_project(&PackageRequest {
            project: project_root,
            repo_root: aster_repo_root(),
            target: PackageTarget::parse(target)?,
            format: PackageFormat::parse(format)?,
            channel: PackageChannel::parse(channel)?,
            optimize_assets,
            include_debug_symbols,
            output_dir: None,
        })?;

        self.console.push(ConsoleEntry {
            timestamp: timestamp_now(),
            level: ConsoleLevel::Info,
            source: engine_editor::ConsoleSource {
                subsystem: "build".to_owned(),
                file: None,
                line: None,
            },
            message: format!(
                "Packaged {} for {}/{} at {}",
                output.project,
                output.target,
                output.channel,
                output.path.display()
            ),
        });

        Ok(serde_json::json!({
            "project": output.project,
            "target": output.target,
            "format": output.format,
            "channel": output.channel,
            "path": output.path.to_string_lossy(),
            "binary": output.binary.map(|path| path.to_string_lossy().to_string()),
            "launcher": output.launcher.map(|path| path.to_string_lossy().to_string()),
            "assets_manifest": output.assets_manifest.to_string_lossy(),
            "asset_count": output.asset_count,
        }))
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
            use image::ImageEncoder;
            use image::codecs::png::PngEncoder;
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
            prepared.knowledge_entries_used,
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

        let selected_knowledge_ids = params
            .get("knowledge_ids")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let attached_knowledge = self.selected_approved_knowledge(&selected_knowledge_ids)?;
        let knowledge_context = format_editor_knowledge_context(&attached_knowledge);
        let editor_context = params
            .get("editor_context")
            .map(|context| {
                format!(
                    "\n\n[Editor Context]\n{}",
                    serde_json::to_string_pretty(context).unwrap_or_default()
                )
            })
            .unwrap_or_default();

        // Build enriched prompt with explicit editor, entity, and Knowledge context.
        let enriched_prompt = if let Some(entity) = params.get("selected_entity") {
            format!(
                "{}{}{}\n\n[Selected Entity Context]\n{}",
                prompt,
                editor_context,
                knowledge_context,
                serde_json::to_string_pretty(entity).unwrap_or_default()
            )
        } else {
            format!("{prompt}{editor_context}{knowledge_context}")
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
            knowledge_entries_used: attached_knowledge.len(),
        })
    }

    fn selected_approved_knowledge(
        &self,
        selected_ids: &[String],
    ) -> EngineResult<Vec<quest::KnowledgeEntry>> {
        if selected_ids.is_empty() {
            return Ok(Vec::new());
        }
        let entries = self.quest_store.list_knowledge()?;
        let approved_by_id = entries
            .iter()
            .filter(|entry| entry.status == "approved")
            .map(|entry| (entry.id.as_str(), entry))
            .collect::<std::collections::HashMap<_, _>>();
        let mut selected = Vec::new();
        for id in selected_ids {
            if selected
                .iter()
                .any(|entry: &quest::KnowledgeEntry| entry.id == *id)
            {
                continue;
            }
            let entry = approved_by_id.get(id.as_str()).ok_or_else(|| {
                EngineError::config(
                    "Editor AI can only attach approved Knowledge entries to requests",
                )
            })?;
            selected.push((*entry).clone());
        }
        Ok(selected)
    }

    fn finish_copilot_response_with_tools(
        &mut self,
        original_prompt: &str,
        response: &str,
        tool_calls: &[engine_ai::ToolCall],
        cached_context: engine_editor::ProjectContext,
        knowledge_entries_used: usize,
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
            "knowledge_entries_used": knowledge_entries_used,
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

        let before_snapshot = self.scene_snapshot().ok();
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

        let after_snapshot = self.scene_snapshot().ok();
        let undo_available = if !applied_read_only {
            if let (Some(before), Some(after)) = (before_snapshot, after_snapshot) {
                if before != after {
                    self.shell.push_undo(UndoCommand::new(
                        "AI scoped edit",
                        "copilot",
                        before,
                        after,
                    ));
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

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
            "undo_available": undo_available,
            "undo_label": if undo_available { Some("AI scoped edit") } else { None::<&str> },
            "needs_continuation": should_continue_copilot(applied_read_only, outcome.completed),
        }))
    }

    fn copilot_undo_last(&mut self, _params: &Value) -> EngineResult<Value> {
        let applied = self.shell.undo_scene_command()?;
        if applied {
            self.drain_shell_console();
            self.bump_scene_version();
            self.copilot_conversation
                .push(engine_ai::ChatMessage::assistant(
                    "Undid the last AI scoped edit through the editor undo stack.".to_owned(),
                ));
        }
        Ok(serde_json::json!({
            "applied": applied,
            "summary": if applied {
                "Undid the last AI scoped edit."
            } else {
                "No undoable AI scoped edit was available."
            },
            "trace_entries": [{
                "tool": "editor.undo",
                "result": if applied { "applied" } else { "skipped" },
                "recovery_hint": null
            }]
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
        let selected_entity = self
            .shell
            .selected_entity_id()
            .map(|id| format!("{:032x}", id.as_u128()));
        Ok(serde_json::json!({
            "has_project": self.shell.project().is_some(),
            "project_name": self.shell.project().map(|p| p.name()),
            "scene_dirty": self.shell.is_scene_dirty(),
            "can_undo": self.shell.undo_stack().can_undo(),
            "can_redo": self.shell.undo_stack().can_redo(),
            "scene_version": self.scene_version,
            "selected_entity": selected_entity,
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
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("New Object");

        let object_id = self.shell.create_scene_object(name)?;

        if let Some(parent_id_str) = params.get("parent_id").and_then(|v| v.as_str()) {
            let parent_id = engine_core::EntityId::from_u128(
                u128::from_str_radix(parent_id_str, 16)
                    .map_err(|_| EngineError::config("invalid parent id"))?,
            );
            let before = self.scene_snapshot()?;
            let Some(project) = self.shell.project_mut() else {
                return Err(EngineError::config("no project open"));
            };
            let entity = project
                .scene
                .find_by_id(object_id)
                .ok_or_else(|| EngineError::config("created entity not found"))?;
            let parent_entity = project
                .scene
                .find_by_id(parent_id)
                .ok_or_else(|| EngineError::config("parent entity not found"))?;
            project.scene.set_parent(entity, Some(parent_entity))?;
            project.scene_dirty = true;
            let after = self.scene_snapshot()?;
            self.shell
                .push_undo(UndoCommand::new("Reparent Object", name, before, after));
        }

        self.bump_scene_version();

        let project = self
            .shell
            .project()
            .ok_or_else(|| EngineError::config("no project open"))?;
        let entity = project
            .scene
            .find_by_id(object_id)
            .ok_or_else(|| EngineError::config("created entity not found"))?;
        let obj = project
            .scene
            .object(entity)
            .ok_or_else(|| EngineError::config("created entity metadata not found"))?;
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

        self.shell.select_entity_id(entity_id);
        self.shell.rename_selected_scene_object(new_name)?;

        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        if project.scene.find_by_id(entity_id).is_none() {
            return Err(EngineError::config("entity not found"));
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

        self.shell.select_entity_id(entity_id);
        self.shell.delete_selected_scene_object()?;
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

        let delta = t.translation - current.translation;
        self.shell
            .nudge_selected_scene_object(delta, "Update Transform")?;

        if t.rotation != current.rotation || t.scale != current.scale {
            let before = self.scene_snapshot()?;
            let Some(project) = self.shell.project_mut() else {
                return Err(EngineError::config("no project open"));
            };
            let entity = project
                .scene
                .find_by_id(entity_id)
                .ok_or_else(|| EngineError::config("entity not found"))?;
            let mut transform = project.scene.transforms().local(entity).unwrap_or_default();
            transform.rotation = t.rotation;
            transform.scale = t.scale;
            project.scene.transforms_mut().set_local(entity, transform);
            project.scene_dirty = true;
            let after = self.scene_snapshot()?;
            if before != after {
                self.shell
                    .push_undo(UndoCommand::new("Update Transform", id_str, before, after));
            }
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

        let component = match comp_type {
            "Camera" => ComponentData::Camera(Default::default()),
            "Light" => ComponentData::Light(Default::default()),
            "MeshRenderer" => ComponentData::MeshRenderer(Default::default()),
            "Rigidbody" => ComponentData::Rigidbody(Default::default()),
            "Collider" => ComponentData::Collider(Default::default()),
            "FluidVolume" => ComponentData::FluidVolume(Default::default()),
            "WindZone" => ComponentData::WindZone(Default::default()),
            "AudioSource" => ComponentData::AudioSource(Default::default()),
            "AudioListener" => ComponentData::AudioListener(Default::default()),
            "AcousticMaterial" => ComponentData::AcousticMaterial(Default::default()),
            "AcousticGeometry" => ComponentData::AcousticGeometry(Default::default()),
            "AcousticRoom" => ComponentData::AcousticRoom(Default::default()),
            "AcousticPortal" => ComponentData::AcousticPortal(Default::default()),
            "AudioZone" => ComponentData::AudioZone(Default::default()),
            "Script" => ComponentData::Script(engine_ecs::ScriptComponent::new(String::new())),
            _ => {
                return Err(EngineError::config(format!(
                    "unknown component type: {comp_type}"
                )));
            }
        };

        self.shell.select_entity_id(entity_id);
        self.shell
            .add_component_to_selected_scene_object(component)?;
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

        self.shell.select_entity_id(entity_id);
        self.shell
            .remove_component_from_selected_scene_object(comp_type)?;
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

    fn create_scene_runtime_snapshot(&self) -> EngineResult<scene_window::SceneRuntimeSnapshot> {
        let Some(project) = self.shell.project() else {
            return Err(EngineError::config("no project open"));
        };
        let config = EngineConfig::new(
            project.name().to_owned(),
            project.root.clone(),
            RuntimeProfile::RuntimeGame,
        );
        Ok(scene_window::SceneRuntimeSnapshot::new(
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

        let mut closed = false;
        for event in gw.poll_events() {
            match event {
                game_window::GameEvent::Closed => {
                    tracing::debug!(target: "editor", "game window closed");
                    closed = true;
                }
                game_window::GameEvent::Error(msg) => {
                    tracing::error!(target: "editor", "game window error: {msg}");
                }
            }
        }
        if closed {
            self.game_window = None;
        }
    }

    /// Polls events from the native scene window and handles close/error.
    fn poll_scene_window(&mut self) {
        let Some(scene_window) = self.scene_window.as_ref() else {
            return;
        };

        let mut closed = false;
        for event in scene_window.poll_events() {
            match event {
                scene_window::SceneEvent::Closed => {
                    tracing::debug!(target: "editor", "scene window closed");
                }
                scene_window::SceneEvent::Error(msg) => {
                    tracing::error!(target: "editor", "scene window error: {msg}");
                    closed = true;
                }
            }
        }
        if closed {
            self.scene_window = None;
        }
    }

    /// Forward console entries from the shell's console service to our shared one.
    fn drain_shell_console(&mut self) {
        for entry in self.shell.console().entries().iter() {
            self.console.push(entry.clone());
        }
    }
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

fn try_create_git_worktree(project_root: &Path, workspace_root: &Path) -> EngineResult<bool> {
    let inside_git = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output();
    let Ok(inside_git) = inside_git else {
        return Ok(false);
    };
    if !inside_git.status.success() {
        return Ok(false);
    }

    let branch = workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(|name| format!("quest/{name}"))
        .unwrap_or_else(|| format!("quest/workspace-{}", unix_time_ms()));
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(&branch)
        .arg(workspace_root)
        .arg("HEAD")
        .output()
        .map_err(|source| EngineError::Filesystem {
            path: project_root.to_path_buf(),
            source,
        })?;
    Ok(output.status.success())
}

fn copy_project_tree(source: &Path, destination: &Path) -> EngineResult<()> {
    const SKIPPED_DIRS: &[&str] = &[
        ".git",
        "target",
        "dist",
        "node_modules",
        ".ralph-tui",
        ".reasonix",
    ];
    std::fs::create_dir_all(destination).map_err(|source| EngineError::Filesystem {
        path: destination.to_path_buf(),
        source,
    })?;
    for entry in std::fs::read_dir(source).map_err(|source_error| EngineError::Filesystem {
        path: source.to_path_buf(),
        source: source_error,
    })? {
        let entry = entry.map_err(|source_error| EngineError::Filesystem {
            path: source.to_path_buf(),
            source: source_error,
        })?;
        let path = entry.path();
        let name = entry.file_name();
        let name_string = name.to_string_lossy();
        if SKIPPED_DIRS.iter().any(|skipped| *skipped == name_string) {
            continue;
        }
        let target = destination.join(&name);
        let file_type = entry
            .file_type()
            .map_err(|source_error| EngineError::Filesystem {
                path: path.clone(),
                source: source_error,
            })?;
        if file_type.is_dir() {
            copy_project_tree(&path, &target)?;
        } else if file_type.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            std::fs::copy(&path, &target).map_err(|source| EngineError::Filesystem {
                path: target,
                source,
            })?;
        }
    }
    Ok(())
}

fn snapshot_rollback_file(
    rollback_root: &Path,
    relative: &Path,
    active_file: &Path,
) -> EngineResult<()> {
    let target = rollback_root.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    if active_file.is_file() {
        std::fs::copy(active_file, &target).map_err(|source| EngineError::Filesystem {
            path: target,
            source,
        })?;
    } else {
        let tombstone = target.with_extension(format!(
            "{}missing",
            target
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        std::fs::write(&tombstone, b"").map_err(|source| EngineError::Filesystem {
            path: tombstone,
            source,
        })?;
    }
    Ok(())
}

fn restore_rollback_files(
    rollback_root: &Path,
    project_root: &Path,
    files: &[String],
) -> EngineResult<()> {
    for file in files {
        let relative = normalize_relative_path(file)?;
        let snapshot = rollback_root.join(&relative);
        let destination = project_root.join(&relative);
        let tombstone = snapshot.with_extension(format!(
            "{}missing",
            snapshot
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        if tombstone.is_file() {
            if destination.exists() {
                std::fs::remove_file(&destination).map_err(|source| EngineError::Filesystem {
                    path: destination.clone(),
                    source,
                })?;
            }
            continue;
        }
        if !snapshot.is_file() {
            return Err(EngineError::config(format!(
                "rollback file is missing: {file}"
            )));
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::copy(&snapshot, &destination).map_err(|source| EngineError::Filesystem {
            path: destination,
            source,
        })?;
    }
    Ok(())
}

fn collect_workspace_snapshot(root: &Path) -> EngineResult<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    collect_workspace_snapshot_inner(root, root, &mut files)?;
    Ok(files)
}

fn collect_workspace_snapshot_inner(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> EngineResult<()> {
    const SKIPPED_DIRS: &[&str] = &[".git", "target", "dist", "node_modules"];
    if !current.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(current).map_err(|source| EngineError::Filesystem {
        path: current.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| EngineError::Filesystem {
            path: current.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| EngineError::Filesystem {
                path: path.clone(),
                source,
            })?;
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIPPED_DIRS.iter().any(|skipped| *skipped == name) {
                continue;
            }
            collect_workspace_snapshot_inner(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path).map_err(|source| EngineError::Filesystem {
                path: path.clone(),
                source,
            })?;
            files.insert(relative, bytes);
        }
    }
    Ok(())
}

fn diff_workspace_snapshots(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Vec<ChangedFile> {
    let mut changed = Vec::new();
    for (path, after_bytes) in after {
        match before.get(path) {
            Some(before_bytes) if before_bytes == after_bytes => {}
            Some(before_bytes) => changed.push(ChangedFile {
                path: path.clone(),
                additions: count_added_lines(before_bytes, after_bytes),
                deletions: count_added_lines(after_bytes, before_bytes),
                status: "modified".to_owned(),
                diff: format_line_diff(path, Some(before_bytes), Some(after_bytes)),
            }),
            None => changed.push(ChangedFile {
                path: path.clone(),
                additions: String::from_utf8_lossy(after_bytes).lines().count() as u32,
                deletions: 0,
                status: "added".to_owned(),
                diff: format_line_diff(path, None, Some(after_bytes)),
            }),
        }
    }
    for (path, before_bytes) in before {
        if !after.contains_key(path) {
            changed.push(ChangedFile {
                path: path.clone(),
                additions: 0,
                deletions: String::from_utf8_lossy(before_bytes).lines().count() as u32,
                status: "deleted".to_owned(),
                diff: format_line_diff(path, Some(before_bytes), None),
            });
        }
    }
    changed
}

fn format_line_diff(path: &str, before: Option<&[u8]>, after: Option<&[u8]>) -> String {
    let mut output = String::new();
    output.push_str(&format!("--- a/{path}\n+++ b/{path}\n"));
    match (before, after) {
        (Some(before), Some(after)) => {
            let before_text = String::from_utf8_lossy(before);
            let after_text = String::from_utf8_lossy(after);
            let before_lines: std::collections::HashSet<&str> = before_text.lines().collect();
            let after_lines: std::collections::HashSet<&str> = after_text.lines().collect();
            for line in before_text
                .lines()
                .filter(|line| !after_lines.contains(line))
                .take(80)
            {
                output.push_str(&format!("-{line}\n"));
            }
            for line in after_text
                .lines()
                .filter(|line| !before_lines.contains(line))
                .take(80)
            {
                output.push_str(&format!("+{line}\n"));
            }
        }
        (None, Some(after)) => {
            for line in String::from_utf8_lossy(after).lines().take(120) {
                output.push_str(&format!("+{line}\n"));
            }
        }
        (Some(before), None) => {
            for line in String::from_utf8_lossy(before).lines().take(120) {
                output.push_str(&format!("-{line}\n"));
            }
        }
        (None, None) => {}
    }
    output
}

fn count_added_lines(before: &[u8], after: &[u8]) -> u32 {
    let before_text = String::from_utf8_lossy(before);
    let after_text = String::from_utf8_lossy(after);
    let before_lines: std::collections::HashSet<&str> = before_text.lines().collect();
    after_text
        .lines()
        .filter(|line| !before_lines.contains(line))
        .count() as u32
}

fn validate_quest_workspace(workspace_root: &Path) -> Vec<ValidationResult> {
    let mut results = Vec::new();
    let project = match engine_editor::ProjectContext::open(workspace_root) {
        Ok(project) => {
            results.push(ValidationResult::new(
                "Project load",
                "passed",
                "Project manifest, default scene, and asset database loaded.",
            ));
            project
        }
        Err(error) => {
            results.push(ValidationResult::new(
                "Project load",
                "failed",
                error.to_string(),
            ));
            return results;
        }
    };

    results.push(validate_quest_scene_round_trip(&project));
    results.push(validate_quest_asset_scan(&project));
    results.push(validate_quest_script_references(&project));
    results.push(validate_quest_play_preview(workspace_root, &project));

    let cargo_toml = workspace_root.join("Cargo.toml");
    if cargo_toml.is_file() {
        let command = quest_validation_registry()
            .into_iter()
            .find(|command| command.id == "cargo_check_quiet")
            .expect("quest validation registry must include cargo_check_quiet");
        match command.run(workspace_root) {
            Ok(output) if output.status.success() => results.push(
                ValidationResult::new(
                    command.label,
                    "passed",
                    format!(
                        "{} completed successfully through policy-approved registry entry `{}`.",
                        command.display(),
                        command.id
                    ),
                )
                .with_policy_command(
                    command.id,
                    command.display(),
                    command_output_log(&output),
                ),
            ),
            Ok(output) => {
                let log = command_output_log(&output);
                let summary = log.lines().take(12).collect::<Vec<_>>().join("\n");
                results.push(
                    ValidationResult::new(command.label, "failed", summary).with_policy_command(
                        command.id,
                        command.display(),
                        log,
                    ),
                );
            }
            Err(error) => results.push(
                ValidationResult::new(
                    command.label,
                    "failed",
                    format!("failed to run {}: {error}", command.display()),
                )
                .with_policy_command(
                    command.id,
                    command.display(),
                    error.to_string(),
                ),
            ),
        }
    } else {
        results.push(ValidationResult::new(
            "cargo check",
            "skipped",
            "No Cargo.toml found in Quest workspace.",
        ));
    }
    results
}

fn validate_quest_scene_round_trip(project: &engine_editor::ProjectContext) -> ValidationResult {
    match project
        .scene
        .to_json("quest-validation")
        .and_then(|scene_json| engine_ecs::Scene::from_json(&scene_json).map(|_| scene_json))
    {
        Ok(scene_json) => ValidationResult::new(
            "Scene schema",
            "passed",
            format!(
                "Default scene round-tripped through the ECS JSON schema ({} bytes).",
                scene_json.len()
            ),
        ),
        Err(error) => ValidationResult::new("Scene schema", "failed", error.to_string()),
    }
}

fn validate_quest_asset_scan(project: &engine_editor::ProjectContext) -> ValidationResult {
    let assets = project.sorted_assets();
    let missing = assets
        .iter()
        .filter(|asset| {
            !project
                .root
                .join(&project.manifest.asset_root)
                .join(&asset.source_path)
                .is_file()
        })
        .map(|asset| asset.source_path.display().to_string())
        .take(8)
        .collect::<Vec<_>>();
    if missing.is_empty() {
        ValidationResult::new(
            "Asset scan",
            "passed",
            format!(
                "Asset database scanned {} supported asset(s).",
                assets.len()
            ),
        )
    } else {
        ValidationResult::new(
            "Asset scan",
            "failed",
            format!(
                "Asset metadata points at missing source files: {}",
                missing.join(", ")
            ),
        )
    }
}

fn validate_quest_script_references(project: &engine_editor::ProjectContext) -> ValidationResult {
    let mut script_refs = Vec::new();
    for (_, object) in project.scene.objects() {
        for script in &object.scripts {
            if !script.source.trim().is_empty() {
                script_refs.push(script.source.clone());
            }
        }
        for component in &object.components {
            if let engine_ecs::ComponentData::Script(script) = component {
                if !script.source.trim().is_empty() {
                    script_refs.push(script.source.clone());
                }
            }
        }
    }
    script_refs.sort();
    script_refs.dedup();
    let missing = script_refs
        .iter()
        .filter(|script| !resolve_project_script_reference(project, script).is_file())
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        ValidationResult::new(
            "Script references",
            "passed",
            format!("Validated {} scene script reference(s).", script_refs.len()),
        )
    } else {
        ValidationResult::new(
            "Script references",
            "failed",
            format!("Missing script reference(s): {}", missing.join(", ")),
        )
    }
}

fn resolve_project_script_reference(
    project: &engine_editor::ProjectContext,
    script: &str,
) -> PathBuf {
    let script_path = Path::new(script);
    if let Ok(stripped) = script_path.strip_prefix("project:/") {
        project.root.join(stripped)
    } else if script_path.is_absolute() {
        script_path.to_path_buf()
    } else {
        project.root.join(script_path)
    }
}

fn validate_quest_play_preview(
    workspace_root: &Path,
    project: &engine_editor::ProjectContext,
) -> ValidationResult {
    match headless_services_from_scene(
        EngineConfig::default(),
        workspace_root.to_path_buf(),
        &project.scene,
    )
    .and_then(|mut services| {
        services.load_project_assets(project.root.join(&project.manifest.asset_root))?;
        services.tick_game_frame(Duration::from_millis(16), true)?;
        Ok(services.diagnostics)
    }) {
        Ok(diagnostics) => {
            let errors = diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.level == "error")
                .take(5)
                .map(|diagnostic| diagnostic.message.clone())
                .collect::<Vec<_>>();
            if errors.is_empty() {
                ValidationResult::new(
                    "Play preview smoke",
                    "passed",
                    format!(
                        "Headless runtime advanced one frame with {} diagnostic(s).",
                        diagnostics.len()
                    ),
                )
            } else {
                ValidationResult::new("Play preview smoke", "failed", errors.join("\n"))
            }
        }
        Err(error) => ValidationResult::new("Play preview smoke", "failed", error.to_string()),
    }
}

fn command_output_log(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut lines = Vec::new();
    if !stdout.trim().is_empty() {
        lines.push("stdout:".to_owned());
        lines.extend(stdout.lines().map(str::to_owned));
    }
    if !stderr.trim().is_empty() {
        lines.push("stderr:".to_owned());
        lines.extend(stderr.lines().map(str::to_owned));
    }
    if lines.is_empty() {
        "Command completed without stdout or stderr.".to_owned()
    } else {
        lines.join("\n")
    }
}

#[derive(Clone, Copy)]
struct QuestValidationCommand {
    id: &'static str,
    label: &'static str,
    program: &'static str,
    args: &'static [&'static str],
}

impl QuestValidationCommand {
    fn display(&self) -> String {
        std::iter::once(self.program)
            .chain(self.args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn run(&self, workspace_root: &Path) -> EngineResult<std::process::Output> {
        let mut sandbox = SandboxPolicy::new([workspace_root.to_path_buf()]);
        sandbox.allow_command(std::iter::once(self.program).chain(self.args.iter().copied()));
        let command: Vec<String> = std::iter::once(self.program)
            .chain(self.args.iter().copied())
            .map(str::to_owned)
            .collect();
        if !sandbox.allows_path(workspace_root) || !sandbox.allows_command(&command) {
            return Err(EngineError::config(format!(
                "Quest validation command `{}` is not policy-approved",
                self.display()
            )));
        }
        Command::new(self.program)
            .args(self.args)
            .current_dir(workspace_root)
            .output()
            .map_err(|source| EngineError::Filesystem {
                path: workspace_root.to_path_buf(),
                source,
            })
    }
}

fn quest_validation_registry() -> Vec<QuestValidationCommand> {
    vec![QuestValidationCommand {
        id: "cargo_check_quiet",
        label: "cargo check",
        program: "cargo",
        args: &["check", "--quiet"],
    }]
}

fn parse_generated_quest_response(
    tool_calls: &[engine_ai::ToolCall],
    text_content: &str,
    goal: &str,
) -> EngineResult<GeneratedQuestSpec> {
    let mut spec_artifact: Option<(String, String)> = None;
    let mut tasks = Vec::new();
    let mut question_cards = Vec::new();
    for call in tool_calls {
        match call.name.as_str() {
            "create_or_update_spec" => {
                let title = call
                    .arguments
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .trim_matches(['"', '\'', '`', '.', ':', '#', '*'])
                    .trim()
                    .chars()
                    .take(96)
                    .collect::<String>();
                let spec = call
                    .arguments
                    .get("markdown")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                if title.is_empty() {
                    return Err(EngineError::config(
                        "Quest spec tool call must include a non-empty title",
                    ));
                }
                if spec.is_empty() {
                    return Err(EngineError::config(
                        "Quest spec tool call must include non-empty markdown",
                    ));
                }
                spec_artifact = Some((title, spec));
            }
            "create_task" => {
                let title = call
                    .arguments
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                if title.is_empty() {
                    return Err(EngineError::config(
                        "Quest task tool call must include a non-empty title",
                    ));
                }
                let summary = call
                    .arguments
                    .get("summary")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                let acceptance = call
                    .arguments
                    .get("acceptance")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                tasks.push(GeneratedQuestTask {
                    title,
                    summary,
                    acceptance,
                });
            }
            "ask_questions" => {
                let title = call
                    .arguments
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("Questions")
                    .chars()
                    .take(96)
                    .collect::<String>();
                let questions_value = call
                    .arguments
                    .get("questions")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        EngineError::config("Quest question tool call must include questions")
                    })?;
                let mut questions = Vec::new();
                for (question_index, question_value) in questions_value.iter().enumerate() {
                    let prompt = question_value
                        .get("prompt")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                    if prompt.is_empty() {
                        return Err(EngineError::config(
                            "Quest question tool call includes an empty prompt",
                        ));
                    }
                    let id = question_value
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("q{}", question_index + 1));
                    let allow_multiple = question_value
                        .get("allow_multiple")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let allow_custom = question_value
                        .get("allow_custom")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    let options = question_value
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .enumerate()
                                .filter_map(|(option_index, option_value)| {
                                    let label = option_value
                                        .get("label")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .trim();
                                    if label.is_empty() {
                                        return None;
                                    }
                                    let id = option_value
                                        .get("id")
                                        .and_then(Value::as_str)
                                        .map(str::trim)
                                        .filter(|value| !value.is_empty())
                                        .map(str::to_owned)
                                        .unwrap_or_else(|| {
                                            char::from(b'A' + option_index.min(25) as u8)
                                                .to_string()
                                        });
                                    let description = option_value
                                        .get("description")
                                        .and_then(Value::as_str)
                                        .map(str::trim)
                                        .filter(|value| !value.is_empty())
                                        .map(str::to_owned);
                                    Some(GeneratedQuestionOption {
                                        id,
                                        label: label.to_owned(),
                                        description,
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if options.is_empty() && !allow_custom {
                        return Err(EngineError::config(
                            "Quest question without options must allow a custom answer",
                        ));
                    }
                    questions.push(GeneratedQuestion {
                        id,
                        prompt,
                        options,
                        allow_multiple,
                        allow_custom,
                    });
                }
                if questions.is_empty() {
                    return Err(EngineError::config(
                        "Quest question tool call must include at least one question",
                    ));
                }
                question_cards.push(GeneratedQuestionCard { title, questions });
            }
            other => {
                return Err(EngineError::config(format!(
                    "unsupported Quest creation tool call: {other}"
                )));
            }
        }
    }
    let (title, spec) = if let Some(spec_artifact) = spec_artifact {
        spec_artifact
    } else {
        let spec = text_content.trim();
        if spec.is_empty() {
            (
                quest_title_from_goal(goal),
                format!(
                    "# {}\n\n## Goal\n\n{}\n",
                    quest_title_from_goal(goal),
                    goal.trim()
                ),
            )
        } else {
            (
                quest_title_from_markdown_or_goal(spec, goal),
                spec.to_owned(),
            )
        }
    };
    Ok(GeneratedQuestSpec {
        title,
        spec,
        tasks,
        question_cards,
    })
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

fn quest_title_from_goal(goal: &str) -> String {
    let title = goal
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Untitled Quest")
        .trim()
        .trim_matches(['"', '\'', '`', '.', ':', '#', '*'])
        .trim()
        .chars()
        .take(96)
        .collect::<String>();
    if title.is_empty() {
        "Untitled Quest".to_owned()
    } else {
        title
    }
}

fn quest_title_from_markdown_or_goal(markdown: &str, goal: &str) -> String {
    markdown
        .lines()
        .find_map(|line| line.trim().strip_prefix("# "))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(|title| title.chars().take(96).collect::<String>())
        .unwrap_or_else(|| quest_title_from_goal(goal))
}

fn quest_creation_tool_definitions() -> Vec<engine_ai::ToolDefinition> {
    vec![
        engine_ai::ToolDefinition {
            name: "create_or_update_spec".to_owned(),
            description: "Create the editable Markdown specification for the Quest.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["title", "markdown"],
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short Quest title."
                    },
                    "markdown": {
                        "type": "string",
                        "description": "Editable Markdown spec. Choose sections that fit the goal; do not force a fixed template."
                    }
                }
            }),
        },
        engine_ai::ToolDefinition {
            name: "create_task".to_owned(),
            description:
                "Create one flexible execution or investigation task for the Quest timeline."
                    .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["title"],
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Task title."
                    },
                    "summary": {
                        "type": "string",
                        "description": "Optional context or intent for this task."
                    },
                    "acceptance": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional task-specific acceptance checks."
                    }
                }
            }),
        },
        engine_ai::ToolDefinition {
            name: "ask_questions".to_owned(),
            description:
                "Create an interactive question card when user clarification is needed before a useful Quest spec can be finalized."
                    .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["questions"],
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short card title, for example Questions."
                    },
                    "questions": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["prompt"],
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "Stable question id."
                                },
                                "prompt": {
                                    "type": "string",
                                    "description": "Question shown to the user."
                                },
                                "allow_multiple": {
                                    "type": "boolean",
                                    "description": "Whether multiple options can be selected."
                                },
                                "allow_custom": {
                                    "type": "boolean",
                                    "description": "Whether the user may type a custom answer."
                                },
                                "options": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["label"],
                                        "properties": {
                                            "id": {
                                                "type": "string",
                                                "description": "Stable option id such as A, B, C."
                                            },
                                            "label": {
                                                "type": "string",
                                                "description": "Option label."
                                            },
                                            "description": {
                                                "type": "string",
                                                "description": "Optional explanation for this option."
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }),
        },
    ]
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

// ─── Thread-safe wrapper ─────────────────────────────────────────────────────

/// Thread-safe wrapper for `EditorHost`.
///
/// `EditorHost` is accessed from synchronous Tauri commands and may hold
/// platform-bound runtime state. This wrapper uses
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
        let knowledge_entries_used = prepared.knowledge_entries_used;
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
                knowledge_entries_used,
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
                completed.knowledge_entries_used,
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

fn run_prepared_quest_model(
    prepared: PreparedQuestModelRequest,
    on_delta: &mut dyn FnMut(engine_ai::AiStreamDelta),
) -> EngineResult<engine_ai::AiResponse> {
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
    model.chat_stream(prepared.request, on_delta)
}

#[tauri::command]
fn start_quest_ai_request(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    requests: State<'_, QuestAiRequestState>,
    request_id: String,
    kind: String,
    params: Value,
) -> Result<(), String> {
    let prepared = state
        .with_host(|host| match kind.as_str() {
            "create" => host
                .prepare_quest_create_request(&params)
                .map(PreparedQuestAiRequest::Create),
            "rewrite" => host
                .prepare_quest_rewrite_request(&params)
                .map(PreparedQuestAiRequest::Rewrite),
            _ => Err(EngineError::config(format!(
                "unknown Quest AI request kind: {kind}"
            ))),
        })
        .map_err(|error| error.to_string())?;
    let requests = requests.requests.clone();
    std::thread::spawn(move || {
        let emit_delta = &mut |delta: engine_ai::AiStreamDelta| {
            if requests
                .lock()
                .expect("poisoned lock")
                .cancelled
                .contains(&request_id)
            {
                return;
            }
            let delta_payload = match &delta {
                engine_ai::AiStreamDelta::ToolCallDelta(tool_call) => {
                    serde_json::to_string(tool_call).unwrap_or_default()
                }
                _ => delta.text().to_owned(),
            };
            let _ = app.emit(
                "quest-ai-stream",
                serde_json::json!({
                    "request_id": request_id,
                    "kind": delta.kind(),
                    "delta": delta_payload,
                }),
            );
        };
        let completed = match prepared {
            PreparedQuestAiRequest::Create(prepared) => {
                let PreparedQuestCreateRequest {
                    model_request,
                    title,
                    goal,
                    project,
                    mode,
                    model_config,
                } = prepared;
                let generated = run_prepared_quest_model(model_request, emit_delta)
                    .and_then(|response| {
                        parse_generated_quest_response(
                            &response.tool_calls,
                            &response.content,
                            &goal,
                        )
                    })
                    .map_err(|error| error.to_string());
                CompletedQuestAiRequest::Create {
                    generated,
                    title,
                    goal,
                    project,
                    mode,
                    model_config,
                }
            }
            PreparedQuestAiRequest::Rewrite(prepared) => {
                let rewritten = run_prepared_quest_model(prepared, emit_delta)
                    .map(|response| response.content)
                    .map_err(|error| error.to_string());
                CompletedQuestAiRequest::Rewrite(rewritten)
            }
        };
        let mut request_state = requests.lock().expect("poisoned lock");
        if request_state.cancelled.remove(&request_id) {
            drop(request_state);
            let _ = app.emit(
                "quest-ai-stream-complete",
                serde_json::json!({ "request_id": request_id }),
            );
            return;
        }
        request_state
            .completed
            .insert(request_id.clone(), completed);
        drop(request_state);
        let _ = app.emit(
            "quest-ai-stream-complete",
            serde_json::json!({ "request_id": request_id }),
        );
    });
    Ok(())
}

#[tauri::command]
fn finish_quest_ai_request(
    state: State<'_, EditorHostState>,
    requests: State<'_, QuestAiRequestState>,
    request_id: String,
) -> Result<Value, String> {
    let completed = requests
        .requests
        .lock()
        .expect("poisoned lock")
        .completed
        .remove(&request_id)
        .ok_or_else(|| "Quest AI request has not completed".to_owned())?;
    state
        .with_host(|host| match completed {
            CompletedQuestAiRequest::Create {
                generated,
                title,
                goal,
                project,
                mode,
                model_config,
            } => host.finish_quest_create(
                generated.map_err(EngineError::other)?,
                title,
                goal,
                project,
                mode,
                model_config,
            ),
            CompletedQuestAiRequest::Rewrite(response) => {
                host.finish_quest_rewrite(response.map_err(EngineError::other)?)
            }
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_quest_ai_request(
    requests: State<'_, QuestAiRequestState>,
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
fn start_quest_execution(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    requests: State<'_, QuestExecutionRequestState>,
    request_id: String,
    id: String,
) -> Result<(), String> {
    let started_at = Instant::now();
    let prepared = state
        .with_host(|host| host.prepare_quest_execution(&id))
        .map_err(|error| error.to_string())?;
    let quest_store = prepared.quest_store.clone();
    let requests = requests.requests.clone();
    std::thread::spawn(move || {
        let result = run_quest_execution(prepared)
            .or_else(|error| record_quest_execution_failure(&quest_store, &id, started_at, error));
        let mut request_state = requests.lock().expect("poisoned lock");
        if request_state.cancelled.remove(&request_id) {
            drop(request_state);
            let _ = app.emit(
                "quest-execution-complete",
                serde_json::json!({ "request_id": request_id }),
            );
            return;
        }
        request_state.completed.insert(
            request_id.clone(),
            result.map_err(|error| error.to_string()),
        );
        drop(request_state);
        let _ = app.emit(
            "quest-execution-complete",
            serde_json::json!({ "request_id": request_id }),
        );
    });
    Ok(())
}

#[tauri::command]
fn finish_quest_execution(
    requests: State<'_, QuestExecutionRequestState>,
    request_id: String,
) -> Result<Value, String> {
    requests
        .requests
        .lock()
        .expect("poisoned lock")
        .completed
        .remove(&request_id)
        .ok_or_else(|| "Quest execution has not completed".to_owned())?
}

#[tauri::command]
fn cancel_quest_execution(
    requests: State<'_, QuestExecutionRequestState>,
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

#[tauri::command]
async fn create_openai_realtime_transcription_session(
    state: State<'_, EditorHostState>,
) -> Result<Value, String> {
    let (api_key, endpoint) = state
        .with_host(|host| host.openai_realtime_transcription_config())
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let url = format!("{endpoint}/realtime/client_secrets");
        let body = serde_json::json!({
            "session": {
                "type": "transcription",
                "audio": {
                    "input": {
                        "transcription": {
                            "model": "gpt-realtime-whisper",
                            "delay": "low"
                        }
                    }
                }
            }
        });
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(12)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .build()
            .into();
        let mut response = agent
            .post(&url)
            .header("Authorization", &format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|error| format!("OpenAI Realtime transcription session failed: {error}"))?;
        let json: Value = response.body_mut().read_json().map_err(|error| {
            format!("OpenAI Realtime transcription session response parse failed: {error}")
        })?;
        Ok(serde_json::json!({
            "session": json,
            "model": "gpt-realtime-whisper",
            "endpoint": endpoint,
            "realtime_url": format!("{endpoint}/realtime/calls"),
        }))
    })
    .await
    .map_err(|error| error.to_string())?
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

#[tauri::command]
fn viewport_presentation_capabilities() -> editor_compositor::ViewportPresentationCapabilities {
    editor_compositor::presentation_capabilities(editor_compositor_requested())
}

#[tauri::command]
fn viewport_presentation_status() -> editor_compositor::ViewportPresentationStatus {
    editor_compositor::presentation_status(editor_compositor_requested())
}

#[tauri::command]
fn viewport_presentation_status_for_main_window(
    app: tauri::AppHandle,
) -> editor_compositor::ViewportPresentationStatus {
    editor_compositor::presentation_status_for(
        editor_compositor_requested(),
        main_window_editor_compositor_support(&app),
        wayland_embedded_compositor::support(),
    )
}

#[tauri::command]
fn sync_editor_compositor_viewport(
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
fn sync_wayland_embedded_compositor_viewport(
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
fn wayland_embedded_compositor_status(
    state: State<'_, EditorHostState>,
) -> Result<wayland_embedded_compositor::WaylandEmbeddedCompositorRuntimeStatus, String> {
    state.with_host(|host| Ok(host.wayland_embedded_compositor.status()))
}

#[derive(Debug, Deserialize)]
struct NativeHostEditorLayout {
    viewport: EmbeddedSceneViewport,
    hierarchy_open: bool,
    inspector_open: bool,
    ai_panel_open: bool,
}

#[tauri::command]
fn sync_native_host_editor_layout(
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
fn native_panel_host_status(
    state: State<'_, EditorHostState>,
) -> native_panel_host::NativePanelHostStatus {
    state.with_host(|host| host.native_panel_host.status())
}

#[tauri::command]
fn ensure_native_panel_host(
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
fn sync_native_panel_layout(
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
fn sync_no_cpu_readback_scene_view(
    app: tauri::AppHandle,
    state: State<'_, EditorHostState>,
    viewport: EmbeddedSceneViewport,
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
fn sync_zero_copy_scene_view(
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
        app, state, viewport, yaw, pitch, distance, target_x, target_y, target_z, None, None, None,
    )
}

#[tauri::command]
fn open_no_cpu_readback_scene_view(
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
        host.scene_window = Some(handle);
        Ok(())
    })
}

#[tauri::command]
fn open_zero_copy_scene_view(
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
        app, state, viewport, yaw, pitch, distance, target_x, target_y, target_z,
    )
}

#[tauri::command]
fn open_editor_compositor_scene_view(
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
        app, state, viewport, yaw, pitch, distance, target_x, target_y, target_z,
    )
}

#[tauri::command]
fn open_wayland_embedded_compositor_scene_view(
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
            .map(|h| PathBuf::from(h).join("varg"))
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
            .map(|h| PathBuf::from(h).join("varg"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Some(PathBuf::from(".varg-data"))
    }
}

// ─── App entry point ─────────────────────────────────────────────────────────

#[tauri::command]
fn open_game_view(_app: tauri::AppHandle, state: State<'_, EditorHostState>) -> Result<(), String> {
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
fn set_game_render_scaling(
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
fn open_native_scene_view(
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

#[derive(Debug, Deserialize)]
struct EmbeddedSceneViewport {
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

#[tauri::command]
fn close_native_scene_view(
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
    // Initialize layered logging: engine / game / editor targets
    // Logs go to: ~/.local/share/varg-editor/logs/ (Linux)
    //             ~/Library/Logs/varg-editor/        (macOS)
    //             %APPDATA%/varg-editor/logs/        (Windows)
    // RUST_LOG=engine=debug,game=info,editor=warn (default: info for all)
    let log_dir = dirs_data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("varg-editor")
        .join("logs");
    let file_appender = tracing_appender::rolling::daily(&log_dir, "varg.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // Keep _guard alive for the entire process lifetime so logs are flushed.
    // We intentionally leak it since run() never returns.
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

    tracing::info!(target: "editor", "logging initialized → {:?}", log_dir);

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
            rpc,
            create_openai_realtime_transcription_session,
            start_copilot_plan,
            finish_copilot_plan,
            cancel_copilot_plan,
            start_quest_ai_request,
            finish_quest_ai_request,
            cancel_quest_ai_request,
            start_quest_execution,
            finish_quest_execution,
            cancel_quest_execution,
            open_game_view,
            set_game_render_scaling,
            open_native_scene_view,
            close_native_scene_view,
            viewport_presentation_capabilities,
            viewport_presentation_status,
            viewport_presentation_status_for_main_window,
            sync_native_host_editor_layout,
            native_panel_host_status,
            ensure_native_panel_host,
            sync_native_panel_layout,
            sync_editor_compositor_viewport,
            sync_wayland_embedded_compositor_viewport,
            wayland_embedded_compositor_status,
            open_no_cpu_readback_scene_view,
            sync_no_cpu_readback_scene_view,
            open_zero_copy_scene_view,
            sync_zero_copy_scene_view,
            open_wayland_embedded_compositor_scene_view,
            open_editor_compositor_scene_view,
            select_project_location,
            viewport_readback_raw,
            open_scene_dialog,
            import_asset_dialog,
            save_scene_as_dialog
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

#[cfg(test)]
mod tests {
    use super::{
        ChangedFile, DesktopEnvironment, EditorHost, QuestApplyDecision, QuestApplyPolicy,
        QuestProject, QuestReview, QuestReviewMetrics, QuestStatus, SoloQuestRunner,
        ValidationResult, asset_meta_path_for_source, copilot_execution_summary,
        extract_codex_account_id, model_detection_config, normalize_relative_path,
        parse_generated_quest_response, project_fingerprint, quest,
        quest_review_actions_for_result, resolve_existing_relative_path,
        resolve_writable_relative_path, should_continue_copilot,
        transaction_groups_from_changed_files, validate_quest_workspace, validations_failed,
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
