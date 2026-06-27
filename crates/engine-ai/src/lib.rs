#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! AI agent service bridging LLMs with the Aster engine.
//!
//! Provides the [`AgentSession`] type that serializes project context,
//! sends it to an AI model, parses the response into [`AgentOperation`]s,
//! and executes them through the editor command system.

mod parser;
pub mod providers;
pub mod registry;
pub mod skills;
mod system_prompt;
pub mod tools;

pub use parser::parse_operations;
pub use registry::ModelRegistry;
pub use skills::{
    SkillReadRequest, SkillReadResult, SkillRegistryConfig, SkillSearchQuery, SkillSearchResult,
    SkillSource, read_skill, search_skills,
};
pub use tools::{
    CapabilityDecision, CapabilityDecisionResult, CapabilityRequest, CapabilityRequestResult,
    EvidenceKind, RiskClass, ToolExposure, ToolSearchQuery, ToolSearchResult, ToolStage, ToolType,
    VargToolMetadata, search_tools,
};

use std::path::{Component, Path, PathBuf};

use engine_core::{EngineError, EngineResult};

fn default_true() -> bool {
    true
}
use engine_editor::{
    CommandContext, CommandRegistry, ConsoleEntry, ConsoleLevel, ConsoleService, ConsoleSource,
    ProjectContext, SelectionService, UndoRedoStack,
    agent::{AgentWriteMode, PermissionPolicy, TraceEntry, TraceRecorder},
};

/// Abstract AI model backend.
///
/// Implementations handle the communication protocol with a specific
/// model provider (OpenAI, Anthropic, Ollama, etc.).
pub trait AiModel {
    /// Sends a chat request and returns the model's response text.
    fn chat(&self, request: AiRequest) -> EngineResult<AiResponse>;

    /// Sends a chat request and reports response text as it arrives.
    ///
    /// Implementations without native streaming support fall back to a single
    /// callback containing the complete response.
    fn chat_stream(
        &self,
        request: AiRequest,
        on_delta: &mut dyn FnMut(AiStreamDelta),
    ) -> EngineResult<AiResponse> {
        let response = self.chat(request)?;
        if !response.thinking.is_empty() {
            on_delta(AiStreamDelta::Thinking(response.thinking.clone()));
        }
        on_delta(AiStreamDelta::Text(response.content.clone()));
        Ok(response)
    }
}

/// A streamed fragment from a model response.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub enum AiStreamDelta {
    /// User-visible answer text.
    Text(String),
    /// Provider-exposed reasoning text or reasoning summary.
    Thinking(String),
    /// A tool call is being constructed (partial arguments).
    ToolCallDelta(ToolCallDelta),
}

impl AiStreamDelta {
    /// Stable event kind used by frontend transports.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Thinking(_) => "thinking",
            Self::ToolCallDelta(_) => "tool_call",
        }
    }

    /// Text carried by this stream fragment.
    pub fn text(&self) -> &str {
        match self {
            Self::Text(text) | Self::Thinking(text) => text,
            Self::ToolCallDelta(d) => &d.name,
        }
    }
}

/// Partial tool call information emitted during streaming.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ToolCallDelta {
    /// Provider-assigned tool call ID.
    pub id: String,
    /// Tool/function name.
    pub name: String,
    /// Partial JSON arguments string (append to accumulate).
    pub arguments_delta: String,
}

/// A complete tool call from the model response.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    /// Provider-assigned tool call ID.
    pub id: String,
    /// Tool/function name.
    pub name: String,
    /// Parsed JSON arguments.
    pub arguments: serde_json::Value,
}

/// Definition of a tool available to the model.
#[derive(Clone, Debug)]
pub struct ToolDefinition {
    /// Tool name (must match an `AgentOperation` action).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the tool's parameters.
    pub parameters: serde_json::Value,
}

/// Role of a message in a multi-turn conversation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    /// System-level instructions (only used internally, not in `messages`).
    System,
    /// User turn.
    User,
    /// Model response.
    Assistant,
}

/// A single message in a conversation history.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    /// Who produced this message.
    pub role: ChatRole,
    /// Text content of the message.
    pub content: String,
}

impl ChatMessage {
    /// Creates a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    /// Creates an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

/// Thinking effort level for models that support extended reasoning.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ThinkingEffort {
    /// Disable thinking/reasoning mode.
    Off,
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort (default when thinking is enabled).
    #[default]
    Medium,
    /// High reasoning effort.
    High,
}

/// Request sent to an AI model.
#[derive(Clone, Debug)]
pub struct AiRequest {
    /// System prompt describing the engine, available tools, and constraints.
    pub system: String,
    /// Serialized project context (scene graph, assets).
    pub context: serde_json::Value,
    /// Complete conversation history (prior turns + current user message).
    ///
    /// The last entry should be a `User` message representing the current prompt.
    /// Empty for single-turn usage (backwards-compatible: providers treat this
    /// as a single user message derived from `user` field).
    pub messages: Vec<ChatMessage>,
    /// Optional thinking effort level for models that support extended reasoning.
    pub thinking_effort: Option<ThinkingEffort>,
    /// Tool definitions for native tool calling. Empty means no tools.
    pub tools: Vec<ToolDefinition>,
}

impl AiRequest {
    /// Creates a single-turn request (convenience for backwards compatibility).
    pub fn single_turn(system: String, context: serde_json::Value, user: String) -> Self {
        Self {
            system,
            context,
            messages: vec![ChatMessage::user(user)],
            thinking_effort: None,
            tools: Vec::new(),
        }
    }
}

/// Response returned by an AI model.
#[derive(Clone, Debug)]
pub struct AiResponse {
    /// Raw text content from the model.
    pub content: String,
    /// Provider-exposed reasoning text or reasoning summary.
    pub thinking: String,
    /// Tool calls requested by the model (native tool calling).
    pub tool_calls: Vec<ToolCall>,
}

/// An operation the AI agent requests the engine to perform.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentOperation {
    /// Execute a registered editor command.
    ExecuteCommand {
        /// Command identifier (e.g. "gameobject.create_empty").
        command: String,
        /// Optional parameters forwarded to the command handler.
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Create or update a Varg script file.
    WriteScript {
        /// Path relative to the asset root (e.g. "scripts/player.varg").
        path: String,
        /// Varg source code.
        source: String,
    },
    /// Run final language-service validation on one or more Varg script files.
    CheckScript {
        /// Paths relative to the asset root.
        paths: Vec<String>,
    },
    /// Create or update a text file relative to the project root.
    WriteFile {
        /// Path relative to the project root.
        path: String,
        /// Complete file content to write.
        content: String,
    },
    /// Create a new GameObject with optional components and position.
    CreateObject {
        /// Display name for the object.
        name: String,
        /// Component specifications.
        #[serde(default)]
        components: Vec<ComponentSpec>,
        /// Optional initial position [x, y, z].
        #[serde(default)]
        position: Option<[f32; 3]>,
    },
    /// Modify a component field on an entity.
    SetProperty {
        /// Entity identifier (e.g. "1:1").
        entity: String,
        /// Component type name (e.g. "Camera").
        component: String,
        /// Field name to modify.
        field: String,
        /// New value for the field.
        value: serde_json::Value,
    },
    /// Remove a component from an entity.
    RemoveComponent {
        /// Entity identifier.
        entity: String,
        /// Component type name to remove.
        component: String,
    },
    /// Delete an entity.
    DestroyObject {
        /// Entity identifier.
        entity: String,
    },
    /// Read a source file from the project.
    ReadFile {
        /// Path relative to the project root.
        path: String,
    },
    /// Create a short-lived Copilot task shown in the editor task card.
    CreateTask {
        /// Stable task id chosen by the model.
        id: String,
        /// Short task title.
        title: String,
    },
    /// Update a short-lived Copilot task shown in the editor task card.
    UpdateTask {
        /// Stable task id previously created by `create_task`.
        id: String,
        /// Optional replacement task title.
        #[serde(default)]
        title: Option<String>,
        /// Whether the task is done.
        #[serde(default)]
        done: Option<bool>,
    },
    /// Report completion with an optional summary message.
    Complete {
        /// Optional summary of what was accomplished.
        #[serde(default)]
        summary: Option<String>,
    },
    /// Update the project memory file (.aster/project.md).
    UpdateProjectMemory {
        /// New content for the project memory, or a section to append.
        content: String,
        /// If true, append as a new section rather than replacing.
        #[serde(default)]
        append: bool,
        /// Section heading when appending.
        #[serde(default)]
        heading: Option<String>,
    },
    /// Update the user memory with an observed pattern or preference.
    UpdateUserMemory {
        /// Memory key (e.g. "naming", "style", "workflow").
        key: String,
        /// Observed value or preference description.
        value: String,
    },
    /// Query the project dependency graph.
    QueryDependencyGraph {
        /// Query type: "all", "entity", "scripts", "edges_for".
        query: String,
        /// Optional filter target (e.g. entity ID for "edges_for").
        #[serde(default)]
        target: Option<String>,
    },
    /// Request asset generation from an external AI tool.
    GenerateAsset {
        /// Tool name (e.g. "gpt-image", "suno", "meshy").
        tool: String,
        /// Natural language prompt for generation.
        prompt: String,
        /// Target path relative to asset root for the output.
        target_path: String,
        /// Optional style hint or parameters.
        #[serde(default)]
        style: Option<String>,
    },
    /// Highlight or focus an entity in the viewport.
    ShowInViewport {
        /// Entity identifier to highlight.
        entity: String,
        /// Whether to add an outline highlight.
        #[serde(default)]
        highlight: bool,
        /// Whether to frame (focus camera on) the entity.
        #[serde(default)]
        frame: bool,
    },
    /// Execute multiple operations in sequence with optional rollback on failure.
    BatchOperation {
        /// Operations to execute in order.
        operations: Vec<AgentOperation>,
        /// If true, revert all operations if any fails (transaction-like behavior).
        #[serde(default)]
        rollback_on_failure: bool,
    },
    /// Query the scene using natural language.
    ///
    /// Examples: "all enemies", "objects near player", "damaged entities", "cameras in scene".
    QuerySceneSemantic {
        /// Natural language query.
        query: String,
    },
    /// Search the AI tool registry for tools that are not always visible.
    ToolSearch {
        /// Natural-language search text.
        query: String,
        /// Optional tool type filters.
        #[serde(default)]
        types: Vec<String>,
        /// Optional required capability filters.
        #[serde(default)]
        capabilities: Vec<String>,
        /// Optional stage filter.
        #[serde(default)]
        stage: Option<String>,
        /// Optional maximum risk class.
        #[serde(default)]
        risk_max: Option<String>,
        /// Maximum number of results to return.
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Search project and global Varg skills.
    SkillSearch {
        /// Natural-language search text.
        query: String,
        /// Optional source filter: "project" or "global".
        #[serde(default)]
        source: Option<String>,
        /// Maximum number of results to return.
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Read a Varg skill file from a resolved skill ID.
    SkillRead {
        /// Resolved skill ID from `skill_search`.
        id: String,
        /// Optional path inside the skill directory. Defaults to `SKILL.md`.
        #[serde(default)]
        path: Option<String>,
    },
    /// Ask the permission gate whether scoped capabilities are currently allowed.
    RequestCapability {
        /// Capability strings requested directly.
        capabilities: Vec<String>,
        /// Optional tool names whose declared capabilities should be included.
        #[serde(default)]
        tools: Vec<String>,
        /// Optional explanation for why access is needed.
        #[serde(default)]
        reason: Option<String>,
    },
    /// Return structured scene hierarchy and object/component summaries.
    GetSceneInfo {
        /// Whether to include component payload details.
        #[serde(default)]
        include_components: bool,
    },
    /// Return detailed transform, component, mesh, material, and bounds info for one object.
    GetObjectInfo {
        /// Entity identifier or exact object name.
        entity: String,
    },
    /// Return asset metadata and scene references for an asset path or GUID.
    GetAssetInfo {
        /// Asset-root-relative path or 128-bit GUID string.
        asset: String,
    },
    /// Create a primitive mesh object using structured transform and material fields.
    CreatePrimitive {
        /// Object name.
        name: String,
        /// Primitive kind, such as cube, sphere, plane, capsule, cylinder, or quad.
        primitive: String,
        /// Optional transform.
        #[serde(default)]
        transform: Option<TransformSpec>,
        /// Optional material override.
        #[serde(default)]
        material: Option<MaterialSpec>,
    },
    /// Set an object's transform using structured position/rotation/scale values.
    SetTransform {
        /// Entity identifier or exact object name.
        entity: String,
        /// New transform values. Omitted fields keep current values.
        transform: TransformSpec,
    },
    /// Duplicate an object one or more times with an optional transform offset.
    DuplicateObject {
        /// Entity identifier or exact object name.
        entity: String,
        /// Number of copies to create.
        #[serde(default)]
        count: Option<usize>,
        /// Local transform offset applied per copy.
        #[serde(default)]
        offset: Option<TransformSpec>,
    },
    /// Create or assign material parameters for an object's MeshRenderer.
    SetMaterial {
        /// Entity identifier or exact object name.
        entity: String,
        /// Material parameters or built-in material name.
        material: MaterialSpec,
    },
    /// Write a structured model authoring file under the asset root.
    CreateMeshAsset {
        /// Asset-root-relative target path, usually under models/ and ending in .vmodel.
        path: String,
        /// Mesh authoring operations or primitives to record.
        operations: Vec<MeshOperationSpec>,
        /// Optional assign-to entity after asset creation.
        #[serde(default)]
        assign_to: Option<String>,
    },
    /// Apply structured mesh operations by recording a derived model authoring file.
    ModifyMesh {
        /// Source asset path/GUID or entity whose MeshRenderer mesh should be used.
        source: String,
        /// Asset-root-relative target path for the derived model authoring file.
        target_path: String,
        /// Operations such as bevel, inset, extrude, mirror, boolean, or array.
        operations: Vec<MeshOperationSpec>,
    },
    /// Capture a lightweight viewport feedback request for the editor surface.
    CaptureViewport {
        /// Optional entity identifier or name to frame before capture.
        #[serde(default)]
        entity: Option<String>,
        /// Optional output path relative to project root for future screenshot persistence.
        #[serde(default)]
        output_path: Option<String>,
    },
    /// Validate scene references, assets, and basic authoring constraints.
    ValidateScene {
        /// Whether to include advisory warnings in addition to blocking errors.
        #[serde(default)]
        include_warnings: bool,
    },
    /// Attach a declarative behavior tree to an entity.
    AttachBehavior {
        /// Entity identifier or name.
        entity: String,
        /// Behavior tree JSON inline or file path.
        #[serde(flatten)]
        behavior: BehaviorSource,
    },
    /// Move an entity to a target position with optional animation.
    MoveEntityTo {
        /// Entity identifier or name.
        entity: String,
        /// Target position [x, y, z].
        position: [f32; 3],
        /// Whether to animate the movement.
        #[serde(default)]
        animated: bool,
        /// Duration in seconds (only if animated).
        #[serde(default)]
        duration: Option<f32>,
    },
    /// Execute a shell command or external process.
    RunCommand {
        /// Command to execute (e.g. "cargo", "python", "ls").
        command: String,
        /// Arguments to pass to the command.
        #[serde(default)]
        args: Vec<String>,
        /// Working directory (relative to project root). Defaults to project root.
        #[serde(default)]
        working_dir: Option<String>,
        /// Timeout in milliseconds. Defaults to 30000 (30 seconds).
        #[serde(default)]
        timeout_ms: Option<u64>,
        /// Whether to capture and return stdout.
        #[serde(default = "default_true")]
        capture_stdout: bool,
        /// Whether to capture and return stderr.
        #[serde(default = "default_true")]
        capture_stderr: bool,
    },
}

impl AgentOperation {
    /// Returns a human-readable action name for trace recording.
    pub fn action_name(&self) -> &'static str {
        match self {
            Self::ExecuteCommand { .. } => "execute_command",
            Self::WriteScript { .. } => "write_script",
            Self::CheckScript { .. } => "check_script",
            Self::WriteFile { .. } => "write_file",
            Self::CreateObject { .. } => "create_object",
            Self::SetProperty { .. } => "set_property",
            Self::RemoveComponent { .. } => "remove_component",
            Self::DestroyObject { .. } => "destroy_object",
            Self::ReadFile { .. } => "read_file",
            Self::CreateTask { .. } => "create_task",
            Self::UpdateTask { .. } => "update_task",
            Self::Complete { .. } => "complete",
            Self::UpdateProjectMemory { .. } => "update_project_memory",
            Self::UpdateUserMemory { .. } => "update_user_memory",
            Self::QueryDependencyGraph { .. } => "query_dependency_graph",
            Self::GenerateAsset { .. } => "generate_asset",
            Self::ShowInViewport { .. } => "show_in_viewport",
            Self::BatchOperation { .. } => "batch_operation",
            Self::QuerySceneSemantic { .. } => "query_scene_semantic",
            Self::ToolSearch { .. } => "tool_search",
            Self::SkillSearch { .. } => "skill_search",
            Self::SkillRead { .. } => "skill_read",
            Self::RequestCapability { .. } => "request_capability",
            Self::GetSceneInfo { .. } => "get_scene_info",
            Self::GetObjectInfo { .. } => "get_object_info",
            Self::GetAssetInfo { .. } => "get_asset_info",
            Self::CreatePrimitive { .. } => "create_primitive",
            Self::SetTransform { .. } => "set_transform",
            Self::DuplicateObject { .. } => "duplicate_object",
            Self::SetMaterial { .. } => "set_material",
            Self::CreateMeshAsset { .. } => "create_mesh_asset",
            Self::ModifyMesh { .. } => "modify_mesh",
            Self::CaptureViewport { .. } => "capture_viewport",
            Self::ValidateScene { .. } => "validate_scene",
            Self::AttachBehavior { .. } => "attach_behavior",
            Self::MoveEntityTo { .. } => "move_entity_to",
            Self::RunCommand { .. } => "run_command",
        }
    }
}

/// Source of a behavior tree definition.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(untagged)]
pub enum BehaviorSource {
    /// Inline behavior tree JSON.
    Inline {
        /// Behavior tree structure.
        behavior_tree: serde_json::Value,
    },
    /// Path to a behavior file.
    File {
        /// Path relative to asset root.
        behavior_path: String,
    },
}

/// Component specification in an AI `CreateObject` operation.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ComponentSpec {
    /// Component type name (e.g. "Camera", "Rigidbody", "Collider").
    #[serde(rename = "type")]
    pub component_type: String,
    /// Optional initial properties for the component.
    #[serde(default)]
    pub properties: serde_json::Value,
}

/// Structured transform values accepted by modeling tools.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct TransformSpec {
    /// Optional translation [x, y, z].
    #[serde(default)]
    pub position: Option<[f32; 3]>,
    /// Optional quaternion rotation [x, y, z, w].
    #[serde(default)]
    pub rotation: Option<[f32; 4]>,
    /// Optional local scale [x, y, z].
    #[serde(default)]
    pub scale: Option<[f32; 3]>,
}

/// Structured material values accepted by modeling tools.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct MaterialSpec {
    /// Built-in material name, if using a built-in material.
    #[serde(default)]
    pub builtin: Option<String>,
    /// Optional generated material asset path relative to the asset root.
    #[serde(default)]
    pub asset_path: Option<String>,
    /// Optional display name for generated material descriptors.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional base color [r, g, b].
    #[serde(default)]
    pub base_color: Option<[f32; 3]>,
    /// Optional roughness value.
    #[serde(default)]
    pub roughness: Option<f32>,
    /// Optional metallic value.
    #[serde(default)]
    pub metallic: Option<f32>,
}

/// Structured mesh operation descriptor.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct MeshOperationSpec {
    /// Operation kind, such as cube, bevel, inset, extrude, mirror, boolean, or array.
    #[serde(rename = "type")]
    pub operation_type: String,
    /// Operation parameters.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Outcome of an AI agent interaction.
#[derive(Clone, Debug)]
pub struct AgentOutcome {
    /// Number of operations performed.
    pub operations_performed: usize,
    /// Console entries produced during execution.
    pub console_entries: Vec<ConsoleEntry>,
    /// Trace entries produced during execution.
    pub trace_entries: Vec<TraceEntry>,
    /// Whether the agent signalled completion.
    pub completed: bool,
    /// Optional completion summary.
    pub summary: Option<String>,
}

impl Default for AgentOutcome {
    fn default() -> Self {
        Self {
            operations_performed: 0,
            console_entries: Vec::new(),
            trace_entries: Vec::new(),
            completed: false,
            summary: None,
        }
    }
}

/// A model-generated operation that has been validated but not yet applied.
#[derive(Clone, Debug)]
pub struct PlannedOperation {
    /// Operation to apply after user approval.
    pub operation: AgentOperation,
    /// Human-readable preview of the planned change.
    pub preview: String,
    /// Whether this operation requires write permission.
    pub requires_write: bool,
    /// Capabilities declared for this operation.
    pub capabilities: Vec<String>,
    /// Policy decisions for each declared capability.
    pub capability_decisions: Vec<CapabilityDecisionResult>,
}

/// A validated agent plan ready for user preview.
#[derive(Clone, Debug)]
pub struct AgentPlan {
    /// Planned operations in model order.
    pub operations: Vec<PlannedOperation>,
    /// Whether all operations are read-only.
    pub read_only: bool,
    /// Whether any operation requires write permission.
    pub requires_write: bool,
    /// Permission policy used to validate this plan.
    pub policy: PermissionPolicy,
}

/// An AI agent session bound to a project.
///
/// Owns the project context, command registry, and undo stack
/// for the duration of an AI interaction.
pub struct AgentSession {
    /// Project state including scene, assets, and manifest.
    pub context: ProjectContext,
    /// Available editor commands.
    pub commands: CommandRegistry,
    /// Undo/redo stack for reversible operations.
    pub undo_stack: UndoRedoStack,
    /// Console service for logging.
    pub console: ConsoleService,
    /// Current selection state.
    pub selection: SelectionService,
    /// Trace recorder for audited agent operations.
    pub trace: TraceRecorder,
    /// Asset root path for script creation.
    asset_root: PathBuf,
    /// Permission policy currently being applied, used by capability preflight logs.
    active_policy: PermissionPolicy,
}

impl AgentSession {
    /// Creates a new agent session from a project context.
    ///
    /// Initializes the script backend, command registry with AI commands,
    /// and supporting services.
    pub fn new(context: ProjectContext) -> EngineResult<Self> {
        let mut commands = CommandRegistry::default();
        engine_editor::register_core_commands(&mut commands);
        engine_editor::register_ai_commands(&mut commands);

        let asset_root = context.root.join(&context.manifest.asset_root);

        Ok(Self {
            context,
            commands,
            undo_stack: UndoRedoStack::default(),
            console: ConsoleService::default(),
            selection: SelectionService::default(),
            trace: TraceRecorder::default(),
            asset_root,
            active_policy: PermissionPolicy::read_only(),
        })
    }

    /// Builds and validates a plan for one AI interaction cycle.
    ///
    /// 1. Builds project context JSON
    /// 2. Constructs the system prompt with available tools
    /// 3. Sends the request to the model
    /// 4. Parses the response into operations
    /// 5. Validates operations against the permission policy without applying them
    pub fn plan(
        &mut self,
        model: &dyn AiModel,
        user_prompt: &str,
        policy: PermissionPolicy,
    ) -> EngineResult<AgentPlan> {
        self.plan_with_history(model, user_prompt, &[], policy)
    }

    /// Builds and validates a plan using multi-turn conversation history.
    ///
    /// `history` contains prior user/assistant turns. The current `user_prompt`
    /// is appended as the final user message.
    pub fn plan_with_history(
        &mut self,
        model: &dyn AiModel,
        user_prompt: &str,
        history: &[ChatMessage],
        policy: PermissionPolicy,
    ) -> EngineResult<AgentPlan> {
        self.plan_with_history_streaming(model, user_prompt, history, policy, None, &mut |_| {})
    }

    /// Builds and validates a plan while forwarding model response deltas.
    pub fn plan_with_history_streaming(
        &mut self,
        model: &dyn AiModel,
        user_prompt: &str,
        history: &[ChatMessage],
        policy: PermissionPolicy,
        thinking_effort: Option<ThinkingEffort>,
        on_delta: &mut dyn FnMut(AiStreamDelta),
    ) -> EngineResult<AgentPlan> {
        let request = self.prepare_request(user_prompt, history, thinking_effort);
        let response = model.chat_stream(request, on_delta)?;
        if !response.tool_calls.is_empty() {
            self.plan_from_tool_calls(&response.tool_calls, &response.content, policy)
        } else {
            self.plan_from_response(&response.content, policy)
        }
    }

    /// Builds a provider request without performing network I/O.
    pub fn prepare_request(
        &self,
        user_prompt: &str,
        history: &[ChatMessage],
        thinking_effort: Option<ThinkingEffort>,
    ) -> AiRequest {
        let available_commands: Vec<&engine_editor::EditorCommand> =
            self.commands.list_executable().collect();
        let mut messages = history.to_vec();
        messages.push(ChatMessage::user(user_prompt));
        AiRequest {
            system: system_prompt::build(&available_commands),
            context: self.context.to_ai_context(),
            messages,
            thinking_effort,
            tools: agent_tool_definitions(),
        }
    }

    fn skill_registry_config(&self) -> SkillRegistryConfig {
        SkillRegistryConfig::new(&self.context.root, default_global_varg_root())
    }

    fn current_policy_hint(&self) -> PermissionPolicy {
        self.active_policy.clone()
    }

    /// Parses and validates a completed provider response.
    pub fn plan_from_response(
        &mut self,
        response: &str,
        policy: PermissionPolicy,
    ) -> EngineResult<AgentPlan> {
        let operations = match parser::parse_operations(response) {
            Ok(operations) => operations,
            Err(error) => {
                self.push_agent_error(format!("parse_response: {error}"));
                return Err(error);
            }
        };

        self.build_plan(operations, policy)
    }

    /// Converts native tool calls into a validated agent plan.
    ///
    /// Each tool call maps to an `AgentOperation` via the tool name.
    /// The model's text content is preserved as a `Complete` operation summary.
    pub fn plan_from_tool_calls(
        &mut self,
        tool_calls: &[ToolCall],
        text_content: &str,
        policy: PermissionPolicy,
    ) -> EngineResult<AgentPlan> {
        let mut operations: Vec<AgentOperation> = Vec::new();

        for tc in tool_calls {
            let op = tool_call_to_operation(tc)?;
            operations.push(op);
        }

        // If the model also produced text content and no Complete op was emitted,
        // add one to preserve the conversational response.
        let has_complete = operations
            .iter()
            .any(|op| matches!(op, AgentOperation::Complete { .. }));
        if !text_content.is_empty() && !has_complete {
            operations.push(AgentOperation::Complete {
                summary: Some(text_content.to_owned()),
            });
        }

        self.build_plan(operations, policy)
    }

    /// Runs one AI interaction cycle using a transactional policy.
    ///
    /// This preserves the original convenience API while routing through plan
    /// validation before any operation is applied.
    pub fn run(&mut self, model: &dyn AiModel, user_prompt: &str) -> EngineResult<AgentOutcome> {
        let plan = self.plan(model, user_prompt, PermissionPolicy::transactional_write())?;
        self.apply_plan(&plan)
    }

    /// Applies an approved plan and records diagnostics and trace entries.
    pub fn apply_plan(&mut self, plan: &AgentPlan) -> EngineResult<AgentOutcome> {
        let mut outcome = AgentOutcome::default();
        self.active_policy = plan.policy.clone();
        for planned in &plan.operations {
            let op = &planned.operation;
            if matches!(op, AgentOperation::Complete { .. }) {
                outcome.completed = true;
                if let AgentOperation::Complete { summary } = op {
                    outcome.summary = summary.clone();
                }
                self.trace.record(
                    op.action_name(),
                    "completed",
                    "No recovery needed; completion does not mutate the project.",
                );
                break;
            }
            match self.execute_operation(op) {
                Ok(()) => {
                    outcome.operations_performed += 1;
                    self.trace
                        .record(op.action_name(), "applied", recovery_hint_for_success(op));
                }
                Err(error) => {
                    let entry = self.push_agent_error(format!("{}: {error}", op.action_name()));
                    outcome.console_entries.push(entry);
                    self.trace.record(
                        op.action_name(),
                        format!("failed: {error}"),
                        recovery_hint_for_failure(op),
                    );
                }
            }
        }
        outcome.trace_entries = self.trace.entries().to_vec();

        Ok(outcome)
    }

    /// Builds a validated plan from parsed operations.
    fn build_plan(
        &self,
        operations: Vec<AgentOperation>,
        policy: PermissionPolicy,
    ) -> EngineResult<AgentPlan> {
        let mut planned = Vec::with_capacity(operations.len());
        let mut requires_write = false;

        for operation in operations {
            let access = operation_access(&operation);
            if access.requires_write {
                requires_write = true;
            }
            validate_operation_policy(&operation, access, &policy)?;
            let capabilities = operation_capabilities(&operation);
            let capability_decisions = evaluate_capabilities(&capabilities, &policy);
            planned.push(PlannedOperation {
                preview: preview_operation(&operation),
                operation,
                requires_write: access.requires_write,
                capabilities,
                capability_decisions,
            });
        }

        Ok(AgentPlan {
            operations: planned,
            read_only: !requires_write,
            requires_write,
            policy,
        })
    }

    /// Executes a single agent operation against the engine state.
    fn execute_operation(&mut self, op: &AgentOperation) -> EngineResult<()> {
        self.execute_operation_inner(op, 0)
    }

    /// Internal execution with recursion depth tracking.
    fn execute_operation_inner(&mut self, op: &AgentOperation, depth: u32) -> EngineResult<()> {
        match op {
            AgentOperation::ExecuteCommand { command, .. } => {
                let mut cmd_ctx = CommandContext {
                    project: &mut self.context,
                    selection: &self.selection,
                    console: &mut self.console,
                };
                let undo = self.commands.execute(command, &mut cmd_ctx)?;
                self.undo_stack.push(undo);
                Ok(())
            }
            AgentOperation::WriteScript { path, source } => {
                let relative = PathBuf::from(path);
                let full_path = write_varg_script(&self.asset_root, &relative, source)?;
                self.context.rescan_assets()?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: Some(full_path),
                        line: None,
                    },
                    message: format!("Created script: {path}"),
                });
                Ok(())
            }
            AgentOperation::CheckScript { paths } => {
                if paths.is_empty() {
                    return Err(EngineError::config(
                        "check_script requires at least one .varg path",
                    ));
                }
                let mut error_count = 0usize;
                for path in paths {
                    let relative = sanitize_project_relative_path(path)?;
                    if relative.extension().and_then(|ext| ext.to_str()) != Some("varg") {
                        return Err(EngineError::config(format!(
                            "check_script path must use .varg extension: {path}"
                        )));
                    }
                    let full_path = self.asset_root.join(&relative);
                    let source = std::fs::read_to_string(&full_path).map_err(|source| {
                        EngineError::Filesystem {
                            path: full_path.clone(),
                            source,
                        }
                    })?;
                    let diagnostics = engine_script_varg::diagnose_source(&full_path, &source);
                    for diagnostic in diagnostics {
                        error_count += 1;
                        self.console.push(ConsoleEntry {
                            timestamp: "now".into(),
                            level: ConsoleLevel::Error,
                            source: ConsoleSource {
                                subsystem: "varg-language-service".into(),
                                file: Some(full_path.clone()),
                                line: diagnostic.line.map(|line| line as u32),
                            },
                            message: format!(
                                "{}: {} Suggestion: {}{}",
                                diagnostic.code,
                                diagnostic.message,
                                diagnostic.suggestion,
                                diagnostic
                                    .source_line
                                    .as_ref()
                                    .map(|line| format!(" Source: `{}`", line.trim()))
                                    .unwrap_or_default()
                            ),
                        });
                    }
                }
                if error_count > 0 {
                    return Err(EngineError::config(format!(
                        "Varg script acceptance failed with {error_count} diagnostic(s). Fix every diagnostic and run check_script again."
                    )));
                }
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "varg-language-service".into(),
                        file: None,
                        line: None,
                    },
                    message: format!("Varg script acceptance passed for {} file(s).", paths.len()),
                });
                Ok(())
            }
            AgentOperation::WriteFile { path, content } => {
                let relative = sanitize_project_relative_path(path)?;
                let full_path = self.context.root.join(relative);
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
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: Some(full_path),
                        line: None,
                    },
                    message: format!("Wrote file: {path}"),
                });
                Ok(())
            }
            AgentOperation::CreateObject {
                name,
                components,
                position,
            } => {
                let entity = self.context.scene.create_object(name.as_str())?;
                if let Some(pos) = position {
                    use engine_core::math::Vec3;
                    if let Some(mut t) = self.context.scene.transforms().local(entity) {
                        t.translation = Vec3::new(pos[0], pos[1], pos[2]);
                        self.context.scene.transforms_mut().set_local(entity, t);
                    }
                }
                for spec in components {
                    let component = self.build_component(spec)?;
                    self.context.scene.upsert_component(entity, component)?;
                }
                let entity_id = format!(
                    "{}:{}",
                    entity.handle().slot(),
                    entity.handle().generation().get()
                );
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: format!("Created object: {name} ({entity_id})"),
                });
                Ok(())
            }
            AgentOperation::SetProperty {
                entity,
                component,
                field,
                value,
            } => self.apply_property(entity, component, field, value),
            AgentOperation::RemoveComponent { entity, component } => {
                let parsed = self.resolve_entity(entity)?;
                let removed = self.context.scene.remove_component(parsed, component)?;
                if removed {
                    self.console.push(ConsoleEntry {
                        timestamp: "now".into(),
                        level: ConsoleLevel::Info,
                        source: ConsoleSource {
                            subsystem: "ai-agent".into(),
                            file: None,
                            line: None,
                        },
                        message: format!("Removed {component} from {entity}"),
                    });
                }
                Ok(())
            }
            AgentOperation::DestroyObject { entity } => {
                let parsed = self.resolve_entity(entity)?;
                self.context.scene.destroy_deferred(parsed)?;
                self.context.scene.process_deferred_destroy()?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: format!("Destroyed object: {entity}"),
                });
                Ok(())
            }
            AgentOperation::ReadFile { path } => {
                let full_path = self.context.root.join(path);
                let content = std::fs::read_to_string(&full_path).map_err(|source| {
                    EngineError::Filesystem {
                        path: full_path,
                        source,
                    }
                })?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: content,
                });
                Ok(())
            }
            AgentOperation::CreateTask { id, title } => {
                if id.trim().is_empty() || title.trim().is_empty() {
                    return Err(EngineError::config(
                        "create_task requires non-empty id and title",
                    ));
                }
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent-task".into(),
                        file: None,
                        line: None,
                    },
                    message: format!("create_task:{id}:{title}"),
                });
                Ok(())
            }
            AgentOperation::UpdateTask { id, title, done } => {
                if id.trim().is_empty() {
                    return Err(EngineError::config("update_task requires non-empty id"));
                }
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent-task".into(),
                        file: None,
                        line: None,
                    },
                    message: format!(
                        "update_task:{id}:{}:{}",
                        title.as_deref().unwrap_or(""),
                        done.map(|value| value.to_string()).unwrap_or_default()
                    ),
                });
                Ok(())
            }
            AgentOperation::Complete { .. } => Ok(()),
            AgentOperation::UpdateProjectMemory {
                content,
                append,
                heading,
            } => {
                use engine_editor::memory::ProjectMemory;
                let mut mem = ProjectMemory::open(&self.context.root);
                if *append {
                    let h = heading.as_deref().unwrap_or("Update");
                    mem.append_section(h, content);
                } else {
                    mem.set_content(content.clone());
                }
                mem.save()?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: "Updated project memory".into(),
                });
                Ok(())
            }
            AgentOperation::UpdateUserMemory { key, value } => {
                use engine_editor::memory::UserMemory;
                let mut mem = UserMemory::open(&self.context.root);
                mem.upsert(key, value);
                mem.save()?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: format!("Remembered: {key} = {value}"),
                });
                Ok(())
            }
            AgentOperation::QueryDependencyGraph { query, target } => {
                use engine_editor::memory::DependencyGraph;
                let graph = DependencyGraph::from_scene(&self.context.scene);
                let result = match query.as_str() {
                    "all" => {
                        serde_json::to_string_pretty(&graph.to_ai_context()).unwrap_or_default()
                    }
                    "entities" => {
                        let entities: Vec<_> = graph
                            .nodes_of_kind(engine_editor::memory::NodeKind::Entity)
                            .iter()
                            .map(|n| format!("{}: {}", n.id, n.label))
                            .collect();
                        entities.join("\n")
                    }
                    "scripts" => {
                        let scripts: Vec<_> = graph
                            .nodes_of_kind(engine_editor::memory::NodeKind::Script)
                            .iter()
                            .map(|n| format!("{}: {}", n.id, n.label))
                            .collect();
                        scripts.join("\n")
                    }
                    "edges_for" => {
                        let t = target.as_deref().unwrap_or("");
                        let edges: Vec<_> = graph
                            .edges_for(t)
                            .iter()
                            .map(|e| format!("{} --{:?}--> {}", e.from, e.relation, e.to))
                            .collect();
                        edges.join("\n")
                    }
                    _ => format!("Unknown query type: {query}"),
                };
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: result,
                });
                Ok(())
            }
            AgentOperation::GenerateAsset {
                tool,
                prompt,
                target_path,
                ..
            } => {
                // Placeholder: asset generation requires external tool integration.
                // For now, log the request and return a descriptive error.
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Warn,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: format!(
                        "Asset generation requested: tool={tool}, target={target_path}, prompt={prompt}"
                    ),
                });
                Err(EngineError::config(format!(
                    "Asset generation tool '{tool}' is not yet connected. Configure it in editor settings."
                )))
            }
            AgentOperation::ShowInViewport {
                entity,
                highlight,
                frame,
            } => {
                // Record the viewport instruction for the frontend to process.
                let action = match (*highlight, *frame) {
                    (true, true) => "highlight and frame",
                    (true, false) => "highlight",
                    (false, true) => "frame",
                    (false, false) => "show",
                };
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: format!("Viewport: {action} entity {entity}"),
                });
                // Selection integration: select the entity so the frontend highlights it
                self.selection
                    .select(engine_editor::Selection::Entity(entity.clone()));
                Ok(())
            }
            AgentOperation::BatchOperation {
                operations,
                rollback_on_failure,
            } => self.execute_batch_operation_inner(operations, *rollback_on_failure, depth),
            AgentOperation::QuerySceneSemantic { query } => self.execute_semantic_query(query),
            AgentOperation::ToolSearch {
                query,
                types,
                capabilities,
                stage,
                risk_max,
                limit,
            } => {
                let results = tools::search_tools(&tools::ToolSearchQuery {
                    query: query.clone(),
                    types: types.clone(),
                    capabilities: capabilities.clone(),
                    stage: stage.clone(),
                    risk_max: risk_max.clone(),
                    limit: *limit,
                })?;
                let result_text = serde_json::to_string_pretty(&results)
                    .map_err(|error| EngineError::other(error.to_string()))?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent-tools".into(),
                        file: None,
                        line: None,
                    },
                    message: result_text,
                });
                Ok(())
            }
            AgentOperation::SkillSearch {
                query,
                source,
                limit,
            } => {
                let results = skills::search_skills(
                    &self.skill_registry_config(),
                    &skills::SkillSearchQuery {
                        query: query.clone(),
                        source: source.clone(),
                        limit: *limit,
                    },
                )?;
                let result_text = serde_json::to_string_pretty(&results)
                    .map_err(|error| EngineError::other(error.to_string()))?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent-skills".into(),
                        file: None,
                        line: None,
                    },
                    message: result_text,
                });
                Ok(())
            }
            AgentOperation::SkillRead { id, path } => {
                let result = skills::read_skill(
                    &self.skill_registry_config(),
                    &skills::SkillReadRequest {
                        id: id.clone(),
                        path: path.clone(),
                    },
                )?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent-skills".into(),
                        file: None,
                        line: None,
                    },
                    message: result.content,
                });
                Ok(())
            }
            AgentOperation::RequestCapability {
                capabilities,
                tools,
                reason,
            } => {
                let requested = resolve_requested_capabilities(capabilities, tools);
                let result = capability_request_result(&requested, &self.current_policy_hint());
                let message = serde_json::to_string_pretty(&serde_json::json!({
                    "reason": reason,
                    "requested": requested,
                    "result": result,
                    "note": "request_capability is a preflight only; it does not grant permission."
                }))
                .map_err(|error| EngineError::other(error.to_string()))?;
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Info,
                    source: ConsoleSource {
                        subsystem: "ai-agent-permissions".into(),
                        file: None,
                        line: None,
                    },
                    message,
                });
                Ok(())
            }
            AgentOperation::GetSceneInfo { include_components } => {
                let info = self.scene_info_json(*include_components)?;
                self.push_json_console("ai-agent-scene", info)
            }
            AgentOperation::GetObjectInfo { entity } => {
                let parsed = self.resolve_entity(entity)?;
                let info = self.object_info_json(parsed)?;
                self.push_json_console("ai-agent-scene", info)
            }
            AgentOperation::GetAssetInfo { asset } => {
                let info = self.asset_info_json(asset);
                self.push_json_console("ai-agent-assets", info)
            }
            AgentOperation::CreatePrimitive {
                name,
                primitive,
                transform,
                material,
            } => self.execute_create_primitive(
                name,
                primitive,
                transform.as_ref(),
                material.as_ref(),
            ),
            AgentOperation::SetTransform { entity, transform } => {
                self.execute_set_transform(entity, transform)
            }
            AgentOperation::DuplicateObject {
                entity,
                count,
                offset,
            } => self.execute_duplicate_object(entity, count.unwrap_or(1), offset.as_ref()),
            AgentOperation::SetMaterial { entity, material } => {
                self.execute_set_material(entity, material)
            }
            AgentOperation::CreateMeshAsset {
                path,
                operations,
                assign_to,
            } => self.execute_create_mesh_asset(path, operations, assign_to.as_deref()),
            AgentOperation::ModifyMesh {
                source,
                target_path,
                operations,
            } => self.execute_modify_mesh(source, target_path, operations),
            AgentOperation::CaptureViewport {
                entity,
                output_path,
            } => self.execute_capture_viewport(entity.as_deref(), output_path.as_deref()),
            AgentOperation::ValidateScene { include_warnings } => {
                let report = self.validate_scene_json(*include_warnings);
                self.push_json_console("ai-agent-validation", report)
            }
            AgentOperation::AttachBehavior { entity, behavior } => {
                self.execute_attach_behavior(entity, behavior)
            }
            AgentOperation::MoveEntityTo {
                entity,
                position,
                animated,
                duration,
            } => self.execute_move_entity(entity, position, *animated, *duration),
            AgentOperation::RunCommand {
                command,
                args,
                working_dir,
                timeout_ms,
                capture_stdout,
                capture_stderr,
            } => self.execute_run_command(
                command,
                args,
                working_dir.as_deref(),
                *timeout_ms,
                *capture_stdout,
                *capture_stderr,
            ),
        }
    }

    fn push_json_console(
        &mut self,
        subsystem: impl Into<String>,
        value: serde_json::Value,
    ) -> EngineResult<()> {
        let message = serde_json::to_string_pretty(&value)
            .map_err(|error| EngineError::other(error.to_string()))?;
        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: subsystem.into(),
                file: None,
                line: None,
            },
            message,
        });
        Ok(())
    }

    fn scene_info_json(&self, include_components: bool) -> EngineResult<serde_json::Value> {
        let objects = self
            .context
            .scene
            .objects()
            .into_iter()
            .map(|(entity, object)| {
                let transform = self.context.scene.transforms().local(entity);
                let parent = self.context.scene.transforms().parent(entity);
                let components = if include_components {
                    object
                        .components
                        .iter()
                        .map(component_json)
                        .collect::<Vec<_>>()
                } else {
                    object
                        .components
                        .iter()
                        .map(|component| serde_json::json!({ "type": component.type_id() }))
                        .collect::<Vec<_>>()
                };
                serde_json::json!({
                    "id": entity_id_string(entity),
                    "name": object.name,
                    "tag": object.tag,
                    "layer": object.layer,
                    "active": object.active,
                    "parent": parent.map(entity_id_string),
                    "transform": transform.map(transform_json),
                    "components": components,
                })
            })
            .collect::<Vec<_>>();

        Ok(serde_json::json!({
            "scene_path": self.context.scene_path,
            "mode": format!("{:?}", self.context.scene.mode()),
            "structure_version": self.context.scene.structure_version(),
            "object_count": objects.len(),
            "objects": objects,
        }))
    }

    fn object_info_json(&self, entity: engine_ecs::Entity) -> EngineResult<serde_json::Value> {
        let object = self
            .context
            .scene
            .object(entity)
            .ok_or_else(|| EngineError::config("object not found"))?;
        let transform = self.context.scene.transforms().local(entity);
        let mesh_renderer = object.components.iter().find_map(|component| {
            if let engine_ecs::ComponentData::MeshRenderer(mesh) = component {
                Some(mesh)
            } else {
                None
            }
        });

        Ok(serde_json::json!({
            "id": entity_id_string(entity),
            "name": object.name,
            "tag": object.tag,
            "layer": object.layer,
            "active": object.active,
            "parent": self.context.scene.transforms().parent(entity).map(entity_id_string),
            "transform": transform.map(transform_json),
            "components": object.components.iter().map(component_json).collect::<Vec<_>>(),
            "mesh": mesh_renderer.map(|mesh| serde_json::json!({
                "asset": mesh.mesh.map(|id| id.as_u128().to_string()),
                "builtin_mesh": mesh.builtin_mesh,
            })),
            "material": mesh_renderer.map(|mesh| serde_json::json!({
                "asset": mesh.material.asset.map(|id| id.as_u128().to_string()),
                "builtin": mesh.material.builtin,
            })),
            "bounds": transform.map(|t| serde_json::json!({
                "center": [t.translation.x, t.translation.y, t.translation.z],
                "extents": [
                    t.scale.x.abs() * 0.5,
                    t.scale.y.abs() * 0.5,
                    t.scale.z.abs() * 0.5
                ],
            })),
        }))
    }

    fn asset_info_json(&self, asset: &str) -> serde_json::Value {
        let query = asset.trim();
        let by_path = self
            .context
            .assets
            .iter()
            .find(|candidate| candidate.source_path.to_string_lossy() == query);
        let by_guid = self
            .context
            .assets
            .iter()
            .find(|candidate| candidate.guid.to_string() == query);
        let meta = by_path.or(by_guid);
        let references = meta
            .map(|meta| scene_references_to_asset(&self.context.scene, meta.guid.as_asset_id()))
            .unwrap_or_default();

        serde_json::json!({
            "query": query,
            "found": meta.is_some(),
            "asset": meta.map(|meta| serde_json::json!({
                "guid": meta.guid.to_string(),
                "path": meta.source_path,
                "kind": format!("{:?}", meta.kind),
                "importer": meta.importer,
            })),
            "references": references,
        })
    }

    fn execute_create_primitive(
        &mut self,
        name: &str,
        primitive: &str,
        transform: Option<&TransformSpec>,
        material: Option<&MaterialSpec>,
    ) -> EngineResult<()> {
        let builtin_mesh = builtin_mesh_for_primitive(primitive)?;
        let entity = self.context.scene.create_object(name)?;
        let mut renderer = engine_ecs::MeshRendererComponentData {
            builtin_mesh: Some(builtin_mesh.to_owned()),
            ..engine_ecs::MeshRendererComponentData::default()
        };
        if let Some(material) = material {
            renderer.material = self.material_ref_from_spec(material)?;
        }
        self.context
            .scene
            .upsert_component(entity, engine_ecs::ComponentData::MeshRenderer(renderer))?;
        if let Some(transform) = transform {
            let current = self
                .context
                .scene
                .transforms()
                .local(entity)
                .unwrap_or_default();
            self.context
                .scene
                .transforms_mut()
                .set_local(entity, apply_transform_spec(current, transform));
        }
        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent-modeling".into(),
                file: None,
                line: None,
            },
            message: format!(
                "Created primitive: {name} ({}) using {builtin_mesh}",
                entity_id_string(entity)
            ),
        });
        Ok(())
    }

    fn execute_set_transform(
        &mut self,
        entity: &str,
        transform: &TransformSpec,
    ) -> EngineResult<()> {
        let parsed = self.resolve_entity(entity)?;
        let current = self
            .context
            .scene
            .transforms()
            .local(parsed)
            .unwrap_or_default();
        self.context
            .scene
            .transforms_mut()
            .set_local(parsed, apply_transform_spec(current, transform));
        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent-modeling".into(),
                file: None,
                line: None,
            },
            message: format!("Updated transform for {entity}"),
        });
        Ok(())
    }

    fn execute_duplicate_object(
        &mut self,
        entity: &str,
        count: usize,
        offset: Option<&TransformSpec>,
    ) -> EngineResult<()> {
        let count = count.clamp(1, 256);
        let source = self.resolve_entity(entity)?;
        let mut created = Vec::with_capacity(count);
        for index in 0..count {
            let duplicate = self.context.scene.clone_object(source)?;
            if let Some(offset) = offset {
                let current = self
                    .context
                    .scene
                    .transforms()
                    .local(duplicate)
                    .unwrap_or_default();
                let stepped = apply_repeated_transform_offset(current, offset, index + 1);
                self.context
                    .scene
                    .transforms_mut()
                    .set_local(duplicate, stepped);
            }
            created.push(entity_id_string(duplicate));
        }
        self.push_json_console(
            "ai-agent-modeling",
            serde_json::json!({
                "duplicated": entity,
                "count": count,
                "created": created,
            }),
        )
    }

    fn execute_set_material(&mut self, entity: &str, material: &MaterialSpec) -> EngineResult<()> {
        let parsed = self.resolve_entity(entity)?;
        let material_ref = self.material_ref_from_spec(material)?;
        let mut renderer = self
            .context
            .scene
            .components(parsed)
            .and_then(|components| {
                components.iter().find_map(|component| {
                    if let engine_ecs::ComponentData::MeshRenderer(mesh) = component {
                        Some(mesh.clone())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default();
        renderer.material = material_ref;
        self.context
            .scene
            .upsert_component(parsed, engine_ecs::ComponentData::MeshRenderer(renderer))?;
        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent-modeling".into(),
                file: None,
                line: None,
            },
            message: format!("Set material on {entity}"),
        });
        Ok(())
    }

    fn material_ref_from_spec(
        &mut self,
        material: &MaterialSpec,
    ) -> EngineResult<engine_ecs::MaterialRef> {
        if let Some(builtin) = material
            .builtin
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(engine_ecs::MaterialRef {
                asset: None,
                builtin: Some(builtin.trim().to_owned()),
            });
        }
        if let Some(path) = material
            .asset_path
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let relative = sanitize_project_relative_path(path)?;
            let full_path = self.asset_root.join(&relative);
            if !full_path.exists() {
                if let Some(parent) = full_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                let descriptor = material_descriptor_source(material);
                std::fs::write(&full_path, descriptor).map_err(|source| {
                    EngineError::Filesystem {
                        path: full_path.clone(),
                        source,
                    }
                })?;
                self.context.rescan_assets()?;
            }
            let guid = self
                .context
                .database
                .get_guid_for_path(&relative)
                .map(|guid| guid.as_asset_id());
            return Ok(engine_ecs::MaterialRef {
                asset: guid,
                builtin: guid
                    .is_none()
                    .then(|| relative.to_string_lossy().to_string()),
            });
        }
        Ok(engine_ecs::MaterialRef::debug())
    }

    fn execute_create_mesh_asset(
        &mut self,
        path: &str,
        operations: &[MeshOperationSpec],
        assign_to: Option<&str>,
    ) -> EngineResult<()> {
        let relative = sanitize_project_relative_path(path)?;
        let full_path = self.asset_root.join(&relative);
        write_model_authoring_file(&full_path, "generated_model", None, operations)?;
        self.context.rescan_assets()?;

        if let Some(entity) = assign_to {
            let parsed = self.resolve_entity(entity)?;
            let guid = self
                .context
                .database
                .get_guid_for_path(&relative)
                .map(|guid| guid.as_asset_id());
            let mut renderer = self
                .context
                .scene
                .components(parsed)
                .and_then(|components| {
                    components.iter().find_map(|component| {
                        if let engine_ecs::ComponentData::MeshRenderer(mesh) = component {
                            Some(mesh.clone())
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_default();
            renderer.mesh = guid;
            renderer.builtin_mesh = guid
                .is_none()
                .then(|| relative.to_string_lossy().to_string());
            self.context
                .scene
                .upsert_component(parsed, engine_ecs::ComponentData::MeshRenderer(renderer))?;
        }

        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent-modeling".into(),
                file: Some(full_path),
                line: None,
            },
            message: format!(
                "Created model authoring file: {}",
                relative.to_string_lossy()
            ),
        });
        Ok(())
    }

    fn execute_modify_mesh(
        &mut self,
        source: &str,
        target_path: &str,
        operations: &[MeshOperationSpec],
    ) -> EngineResult<()> {
        let relative = sanitize_project_relative_path(target_path)?;
        let full_path = self.asset_root.join(&relative);
        write_model_authoring_file(&full_path, "modified_model", Some(source), operations)?;
        self.context.rescan_assets()?;
        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent-modeling".into(),
                file: Some(full_path),
                line: None,
            },
            message: format!(
                "Recorded model modification file: {}",
                relative.to_string_lossy()
            ),
        });
        Ok(())
    }

    fn execute_capture_viewport(
        &mut self,
        entity: Option<&str>,
        output_path: Option<&str>,
    ) -> EngineResult<()> {
        if let Some(entity) = entity {
            self.selection
                .select(engine_editor::Selection::Entity(entity.to_owned()));
        }
        if let Some(path) = output_path {
            let _ = sanitize_project_relative_path(path)?;
        }
        self.push_json_console(
            "ai-agent-viewport",
            serde_json::json!({
                "capture_requested": true,
                "entity": entity,
                "output_path": output_path,
                "note": "Viewport capture is queued for the editor host; this operation records the requested evidence."
            }),
        )
    }

    fn validate_scene_json(&self, include_warnings: bool) -> serde_json::Value {
        let mut diagnostics = Vec::new();
        for (entity, object) in self.context.scene.objects() {
            if object.name.trim().is_empty() {
                diagnostics.push(serde_json::json!({
                    "level": "error",
                    "entity": entity_id_string(entity),
                    "message": "object name is empty",
                }));
            }
            for component in &object.components {
                if let engine_ecs::ComponentData::MeshRenderer(mesh) = component {
                    if mesh.mesh.is_none() && mesh.builtin_mesh.is_none() && include_warnings {
                        diagnostics.push(serde_json::json!({
                            "level": "warning",
                            "entity": entity_id_string(entity),
                            "message": "MeshRenderer has no mesh asset or built-in mesh",
                        }));
                    }
                    if let Some(asset) = mesh.mesh
                        && scene_asset_missing(&self.context, asset)
                    {
                        diagnostics.push(serde_json::json!({
                            "level": "error",
                            "entity": entity_id_string(entity),
                            "message": "MeshRenderer references a missing mesh asset",
                            "asset": asset.as_u128().to_string(),
                        }));
                    }
                    if let Some(asset) = mesh.material.asset
                        && scene_asset_missing(&self.context, asset)
                    {
                        diagnostics.push(serde_json::json!({
                            "level": "error",
                            "entity": entity_id_string(entity),
                            "message": "MeshRenderer references a missing material asset",
                            "asset": asset.as_u128().to_string(),
                        }));
                    }
                }
            }
        }
        let error_count = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic["level"] == "error")
            .count();
        serde_json::json!({
            "ok": error_count == 0,
            "error_count": error_count,
            "diagnostics": diagnostics,
        })
    }

    /// Converts a `ComponentSpec` into a `ComponentData` variant.
    fn build_component(&self, spec: &ComponentSpec) -> EngineResult<engine_ecs::ComponentData> {
        use engine_ecs::ComponentData;
        match spec.component_type.as_str() {
            "Camera" => Ok(ComponentData::Camera(
                engine_ecs::CameraComponentData::default(),
            )),
            "MeshRenderer" => Ok(ComponentData::MeshRenderer(
                engine_ecs::MeshRendererComponentData::default(),
            )),
            "Light" => Ok(ComponentData::Light(
                engine_ecs::LightComponentData::default(),
            )),
            "Rigidbody" => Ok(ComponentData::Rigidbody(
                engine_ecs::RigidbodyComponentData::default(),
            )),
            "Collider" => Ok(ComponentData::Collider(
                engine_ecs::ColliderComponentData::default(),
            )),
            "AudioSource" => Ok(ComponentData::AudioSource(
                engine_ecs::AudioSourceComponentData::default(),
            )),
            "ParticleEmitter" => Ok(ComponentData::ParticleEmitter(
                engine_ecs::ParticleEmitterComponentData::default(),
            )),
            "Sprite2D" => Ok(ComponentData::Sprite2D(
                engine_ecs::Sprite2DComponentData::default(),
            )),
            "Script" => Ok(ComponentData::Script(engine_ecs::ScriptComponent {
                source: spec
                    .properties
                    .get("script")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                exported_values: Default::default(),
                state: Default::default(),
            })),
            _ => Err(EngineError::config(format!(
                "unknown component type: {}",
                spec.component_type
            ))),
        }
    }

    /// Applies a property update to a component field on an entity.
    fn apply_property(
        &mut self,
        entity: &str,
        component: &str,
        field: &str,
        value: &serde_json::Value,
    ) -> EngineResult<()> {
        let parsed = self.resolve_entity(entity)?;
        let components = self
            .context
            .scene
            .components(parsed)
            .ok_or_else(|| EngineError::config(format!("entity {entity} has no components")))?;

        // Find the component and read its current state as JSON
        let component_json = components
            .iter()
            .find(|c| c.type_id() == component)
            .map(|c| serde_json::to_value(c))
            .transpose()
            .map_err(|e| EngineError::other(e.to_string()))?
            .ok_or_else(|| {
                EngineError::config(format!("entity {entity} has no {component} component"))
            })?;

        // Apply the field update
        let mut updated = component_json;
        if let Some(obj) = updated.as_object_mut() {
            // Handle nested fields like "color.x" by setting top-level for now
            obj.insert(field.to_string(), value.clone());

            // Deserialize back to the correct ComponentData variant
            let new_component: engine_ecs::ComponentData = serde_json::from_value(updated)
                .map_err(|e| {
                    EngineError::config(format!("invalid value for {component}.{field}: {e}"))
                })?;

            self.context.scene.upsert_component(parsed, new_component)?;
        }

        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent".into(),
                file: None,
                line: None,
            },
            message: format!("Set {component}.{field} on {entity}"),
        });
        Ok(())
    }

    fn push_agent_error(&mut self, message: String) -> ConsoleEntry {
        let entry = ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Error,
            source: ConsoleSource {
                subsystem: "ai-agent".into(),
                file: None,
                line: None,
            },
            message,
        };
        self.console.push(entry.clone());
        entry
    }

    /// Executes a batch of operations with optional rollback.
    fn execute_batch_operation_inner(
        &mut self,
        operations: &[AgentOperation],
        rollback_on_failure: bool,
        depth: u32,
    ) -> EngineResult<()> {
        const MAX_BATCH_DEPTH: u32 = 4;
        if depth >= MAX_BATCH_DEPTH {
            return Err(EngineError::config(format!(
                "Batch operation nesting exceeded maximum depth of {MAX_BATCH_DEPTH}. \
                 Nested BatchOperations are too deep."
            )));
        }

        if rollback_on_failure
            && operations
                .iter()
                .any(|operation| !operation_is_transactional(operation))
        {
            return Err(EngineError::config(
                "rollback batches may only contain in-memory transactional operations",
            ));
        }

        let scene_snapshot = if rollback_on_failure {
            Some(self.context.scene.to_json("batch_snapshot")?)
        } else {
            None
        };
        let scene_dirty_snapshot = self.context.scene_dirty;
        let undo_snapshot = rollback_on_failure.then(|| self.undo_stack.clone());

        let mut executed = 0;
        for (i, op) in operations.iter().enumerate() {
            match self.execute_operation_inner(op, depth + 1) {
                Ok(()) => executed += 1,
                Err(e) => {
                    if rollback_on_failure {
                        self.context.scene = engine_ecs::Scene::from_json(
                            scene_snapshot
                                .as_deref()
                                .expect("transactional batch always captures scene"),
                        )
                        .map_err(|rollback_error| {
                            EngineError::config(format!(
                                "batch operation failed and scene rollback failed: {rollback_error}"
                            ))
                        })?;
                        self.context.scene_dirty = scene_dirty_snapshot;
                        self.undo_stack =
                            undo_snapshot.expect("transactional batch always captures undo state");
                        self.console.push(ConsoleEntry {
                            timestamp: "now".into(),
                            level: ConsoleLevel::Warn,
                            source: ConsoleSource {
                                subsystem: "ai-agent".into(),
                                file: None,
                                line: None,
                            },
                            message: format!(
                                "Batch rolled back after operation {} failed at step {}/{}",
                                executed,
                                i + 1,
                                operations.len()
                            ),
                        });
                    } else {
                        for _ in 0..executed {
                            self.undo_stack.undo();
                        }
                    }
                    return Err(EngineError::config(format!(
                        "Batch operation failed at step {}/{}: {}. \
                         {} operations completed before failure{}.",
                        i + 1,
                        operations.len(),
                        e,
                        executed,
                        if rollback_on_failure {
                            " (rolled back)"
                        } else {
                            ""
                        }
                    )));
                }
            }
        }

        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent".into(),
                file: None,
                line: None,
            },
            message: format!(
                "Batch operation completed: {}/{} operations succeeded",
                executed,
                operations.len()
            ),
        });

        Ok(())
    }

    /// Executes a semantic query on the scene.
    fn execute_semantic_query(&mut self, query: &str) -> EngineResult<()> {
        let results = self.parse_semantic_query(query)?;

        let result_text = if results.is_empty() {
            format!("No entities match query: '{}'", query)
        } else {
            let lines: Vec<String> = results
                .iter()
                .map(|(entity, name)| {
                    format!(
                        "{}:{} - {}",
                        entity.handle().slot(),
                        entity.handle().generation().get(),
                        name
                    )
                })
                .collect();
            format!("Found {} entities:\n{}", results.len(), lines.join("\n"))
        };

        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent".into(),
                file: None,
                line: None,
            },
            message: result_text,
        });

        Ok(())
    }

    /// Parses a natural language query into entity matches.
    fn parse_semantic_query(&self, query: &str) -> EngineResult<Vec<(engine_ecs::Entity, String)>> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        // Simple pattern matching for common queries
        for (entity, go) in self.context.scene.iter_objects() {
            let name_lower = go.name.to_lowercase();
            let mut matches = false;

            // "all X" patterns
            if query_lower.starts_with("all ") {
                let target = query_lower.strip_prefix("all ").unwrap();
                matches = name_lower.contains(target);
            }
            // "entities with X" patterns
            else if query_lower.contains("with ") {
                if let Some(component_name) = query_lower.split("with ").nth(1) {
                    let component_name = component_name.trim();
                    if let Some(components) = self.context.scene.components(entity) {
                        matches = components
                            .iter()
                            .any(|c| c.type_id().to_lowercase().contains(component_name));
                    }
                }
            }
            // "X near Y" patterns (name-based approximation, not spatial)
            else if query_lower.contains(" near ") {
                if let Some((left, right)) = query_lower.split_once(" near ") {
                    let left = left.trim();
                    let right = right.trim();
                    // Match if the entity name contains either the left or right keyword
                    matches = name_lower.contains(left) || name_lower.contains(right);
                }
            }
            // Direct name match
            else {
                matches = name_lower.contains(&query_lower);
            }

            if matches {
                results.push((entity, go.name.clone()));
            }
        }

        Ok(results)
    }

    /// Attaches a behavior to an entity.
    fn execute_attach_behavior(
        &mut self,
        entity_name: &str,
        behavior: &BehaviorSource,
    ) -> EngineResult<()> {
        let entity = self.resolve_entity(entity_name)?;

        match behavior {
            BehaviorSource::File { behavior_path } => {
                self.attach_behavior_file(entity, behavior_path)?;
            }
            BehaviorSource::Inline { behavior_tree } => {
                // For now, log that inline behavior trees aren't fully implemented
                self.console.push(ConsoleEntry {
                    timestamp: "now".into(),
                    level: ConsoleLevel::Warn,
                    source: ConsoleSource {
                        subsystem: "ai-agent".into(),
                        file: None,
                        line: None,
                    },
                    message: format!(
                        "Inline behavior trees not yet fully implemented. Consider using behavior_path instead. Received: {}",
                        serde_json::to_string_pretty(behavior_tree).unwrap_or_default()
                    ),
                });
            }
        }

        Ok(())
    }

    /// Moves an entity to a target position.
    fn execute_move_entity(
        &mut self,
        entity_name: &str,
        position: &[f32; 3],
        animated: bool,
        duration: Option<f32>,
    ) -> EngineResult<()> {
        let entity = self.resolve_entity(entity_name)?;

        use engine_core::math::Vec3;
        let target = Vec3::new(position[0], position[1], position[2]);

        if animated && duration.is_some() {
            // For now, just set position immediately and log that animation isn't implemented
            if let Some(mut t) = self.context.scene.transforms().local(entity) {
                t.translation = target;
                self.context.scene.transforms_mut().set_local(entity, t);
            }

            self.console.push(ConsoleEntry {
                timestamp: "now".into(),
                level: ConsoleLevel::Warn,
                source: ConsoleSource {
                    subsystem: "ai-agent".into(),
                    file: None,
                    line: None,
                },
                message: format!(
                    "Animated movement not yet implemented. Entity '{}' moved instantly to {:?}. Duration {} was ignored.",
                    entity_name, position, duration.unwrap()
                ),
            });
        } else {
            // Immediate movement
            if let Some(mut t) = self.context.scene.transforms().local(entity) {
                t.translation = target;
                self.context.scene.transforms_mut().set_local(entity, t);
            }

            self.console.push(ConsoleEntry {
                timestamp: "now".into(),
                level: ConsoleLevel::Info,
                source: ConsoleSource {
                    subsystem: "ai-agent".into(),
                    file: None,
                    line: None,
                },
                message: format!("Moved entity '{}' to {:?}", entity_name, position),
            });
        }

        Ok(())
    }

    /// Executes a shell command or external process.
    fn execute_run_command(
        &mut self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        timeout_ms: Option<u64>,
        capture_stdout: bool,
        capture_stderr: bool,
    ) -> EngineResult<()> {
        use std::io::Read;
        use std::process::Command;
        use std::time::Duration;

        let mut cmd = Command::new(command);
        cmd.args(args);

        if let Some(dir) = working_dir {
            let work_dir = self.context.root.join(dir);
            cmd.current_dir(&work_dir);
        } else {
            cmd.current_dir(&self.context.root);
        }

        if capture_stdout {
            cmd.stdout(std::process::Stdio::piped());
        }
        if capture_stderr {
            cmd.stderr(std::process::Stdio::piped());
        }

        let timeout = Duration::from_millis(timeout_ms.unwrap_or(30000));

        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent".into(),
                file: None,
                line: None,
            },
            message: format!("Executing: {} {}", command, args.join(" ")),
        });

        let start = std::time::Instant::now();
        let mut child = cmd.spawn().map_err(|e| {
            EngineError::config(format!("Failed to spawn command '{}': {}", command, e))
        })?;
        let stdout_reader = child.stdout.take().map(|mut stdout| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                stdout.read_to_end(&mut bytes).map(|_| bytes)
            })
        });
        let stderr_reader = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                stderr.read_to_end(&mut bytes).map(|_| bytes)
            })
        });

        let status = loop {
            if start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout_reader);
                drop(stderr_reader);
                return Err(EngineError::config(format!(
                    "Command '{}' timed out after {}ms",
                    command,
                    timeout.as_millis()
                )));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => {
                    return Err(EngineError::config(format!(
                        "Error waiting for command '{}': {}",
                        command, e
                    )));
                }
            }
        };
        let stdout = join_output_reader(stdout_reader, "stdout")?;
        let stderr = join_output_reader(stderr_reader, "stderr")?;

        // Log stdout
        if capture_stdout && !stdout.is_empty() {
            let stdout = String::from_utf8_lossy(&stdout);
            self.console.push(ConsoleEntry {
                timestamp: "now".into(),
                level: ConsoleLevel::Info,
                source: ConsoleSource {
                    subsystem: "ai-agent".into(),
                    file: None,
                    line: None,
                },
                message: format!("stdout:\n{}", stdout),
            });
        }

        // Log stderr
        if capture_stderr && !stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&stderr);
            let level = if status.success() {
                ConsoleLevel::Warn
            } else {
                ConsoleLevel::Error
            };
            self.console.push(ConsoleEntry {
                timestamp: "now".into(),
                level,
                source: ConsoleSource {
                    subsystem: "ai-agent".into(),
                    file: None,
                    line: None,
                },
                message: format!("stderr:\n{}", stderr),
            });
        }

        // Check exit status
        if !status.success() {
            let exit_code = status.code().unwrap_or(-1);
            return Err(EngineError::config(format!(
                "Command '{}' failed with exit code {}",
                command, exit_code
            )));
        }

        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent".into(),
                file: None,
                line: None,
            },
            message: format!("Command '{}' completed successfully", command),
        });

        Ok(())
    }

    /// Resolves an entity by name or ID.
    fn resolve_entity(&self, entity_name: &str) -> EngineResult<engine_ecs::Entity> {
        // Try as entity ID first
        if entity_name.contains(':') {
            return parse_entity_id(entity_name);
        }

        // Try as name
        self.context.scene.find_by_name(entity_name).ok_or_else(|| {
            EngineError::config(format!(
                "Entity '{}' not found. Available entities: {}",
                entity_name,
                self.list_entity_names()
            ))
        })
    }

    /// Lists all entity names in the scene (for error messages).
    fn list_entity_names(&self) -> String {
        let names: Vec<String> = self
            .context
            .scene
            .iter_objects()
            .map(|(_, go)| go.name.clone())
            .take(10)
            .collect();

        if names.is_empty() {
            "(no entities in scene)".to_string()
        } else if names.len() == 10 {
            format!("{}, ...", names.join(", "))
        } else {
            names.join(", ")
        }
    }

    /// Attaches a behavior file to an entity.
    fn attach_behavior_file(
        &mut self,
        entity: engine_ecs::Entity,
        behavior_path: &str,
    ) -> EngineResult<()> {
        use engine_ecs::ComponentData;

        let script_component = ComponentData::Script(engine_ecs::ScriptComponent {
            source: behavior_path.to_string(),
            exported_values: Default::default(),
            state: Default::default(),
        });

        self.context
            .scene
            .upsert_component(entity, script_component)?;

        self.console.push(ConsoleEntry {
            timestamp: "now".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "ai-agent".into(),
                file: None,
                line: None,
            },
            message: format!("Attached behavior '{}' to entity", behavior_path),
        });

        Ok(())
    }
}

fn join_output_reader(
    reader: Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    stream: &str,
) -> EngineResult<Vec<u8>> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    reader
        .join()
        .map_err(|_| EngineError::config(format!("{stream} reader thread panicked")))?
        .map_err(|error| EngineError::config(format!("failed to read command {stream}: {error}")))
}

fn sanitize_project_relative_path(path: &str) -> EngineResult<PathBuf> {
    let mut relative = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(EngineError::config(
                    "file path must stay inside the project",
                ));
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(EngineError::config("file path must not be empty"));
    }
    Ok(relative)
}

fn write_varg_script(asset_root: &Path, relative: &Path, source: &str) -> EngineResult<PathBuf> {
    if relative.extension().and_then(|ext| ext.to_str()) != Some("varg") {
        return Err(EngineError::config(format!(
            "write_script path must use .varg extension: {}",
            relative.display()
        )));
    }
    let full_path = asset_root.join(relative);
    let diagnostics = engine_script_varg::diagnose_source(&full_path, source);
    if !diagnostics.is_empty() {
        let details = diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "{} at {}:{}: {} Suggestion: {}",
                    diagnostic.code,
                    diagnostic.line.unwrap_or(1),
                    diagnostic.column.unwrap_or(1),
                    diagnostic.message,
                    diagnostic.suggestion
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(EngineError::config(format!(
            "Varg script validation failed before write:\n{details}"
        )));
    }
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&full_path, source).map_err(|source| EngineError::Filesystem {
        path: full_path.clone(),
        source,
    })?;
    Ok(full_path)
}

fn entity_id_string(entity: engine_ecs::Entity) -> String {
    format!(
        "{}:{}",
        entity.handle().slot(),
        entity.handle().generation().get()
    )
}

fn transform_json(transform: engine_core::math::Transform) -> serde_json::Value {
    serde_json::json!({
        "position": [
            transform.translation.x,
            transform.translation.y,
            transform.translation.z
        ],
        "rotation": [
            transform.rotation.x,
            transform.rotation.y,
            transform.rotation.z,
            transform.rotation.w
        ],
        "scale": [transform.scale.x, transform.scale.y, transform.scale.z],
    })
}

fn component_json(component: &engine_ecs::ComponentData) -> serde_json::Value {
    serde_json::to_value(component)
        .unwrap_or_else(|_| serde_json::json!({ "type": component.type_id() }))
}

fn apply_transform_spec(
    mut transform: engine_core::math::Transform,
    spec: &TransformSpec,
) -> engine_core::math::Transform {
    use engine_core::math::{Quat, Vec3};
    if let Some(position) = spec.position {
        transform.translation = Vec3::new(position[0], position[1], position[2]);
    }
    if let Some(rotation) = spec.rotation {
        transform.rotation = Quat {
            x: rotation[0],
            y: rotation[1],
            z: rotation[2],
            w: rotation[3],
        }
        .normalized();
    }
    if let Some(scale) = spec.scale {
        transform.scale = Vec3::new(scale[0], scale[1], scale[2]);
    }
    transform
}

fn apply_repeated_transform_offset(
    mut transform: engine_core::math::Transform,
    offset: &TransformSpec,
    step: usize,
) -> engine_core::math::Transform {
    use engine_core::math::{Quat, Vec3};
    let multiplier = step as f32;
    if let Some(position) = offset.position {
        transform.translation += Vec3::new(position[0], position[1], position[2]) * multiplier;
    }
    if let Some(rotation) = offset.rotation {
        let delta = Quat {
            x: rotation[0],
            y: rotation[1],
            z: rotation[2],
            w: rotation[3],
        }
        .normalized();
        for _ in 0..step {
            transform.rotation = (transform.rotation * delta).normalized();
        }
    }
    if let Some(scale) = offset.scale {
        let factor = Vec3::new(scale[0], scale[1], scale[2]);
        for _ in 0..step {
            transform.scale = transform.scale * factor;
        }
    }
    transform
}

fn builtin_mesh_for_primitive(primitive: &str) -> EngineResult<&'static str> {
    match primitive.trim().to_ascii_lowercase().as_str() {
        "cube" | "box" => Ok("debug/cube"),
        "sphere" | "uv_sphere" => Ok("debug/sphere"),
        "plane" => Ok("debug/plane"),
        "quad" | "sprite" => Ok("debug/quad"),
        "capsule" => Ok("debug/capsule"),
        "cylinder" => Ok("debug/cylinder"),
        other => Err(EngineError::config(format!(
            "unknown primitive kind: {other}"
        ))),
    }
}

fn material_descriptor_source(material: &MaterialSpec) -> String {
    let name = material.name.as_deref().unwrap_or("AI Material");
    let base_color = material.base_color.unwrap_or([0.8, 0.8, 0.8]);
    let roughness = material.roughness.unwrap_or(0.7);
    let metallic = material.metallic.unwrap_or(0.0);
    format!(
        "schema_version = 1\n\
         type = \"material\"\n\
         name = {name:?}\n\
         shader = \"pbr\"\n\
         base_color = [{:.4}, {:.4}, {:.4}]\n\
         roughness = {:.4}\n\
         metallic = {:.4}\n",
        base_color[0], base_color[1], base_color[2], roughness, metallic
    )
}

#[derive(serde::Serialize)]
struct ModelAuthoringFile<'a> {
    schema_version: u32,
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a str>,
    operations: &'a [MeshOperationSpec],
}

fn write_model_authoring_file(
    full_path: &Path,
    kind: &str,
    source: Option<&str>,
    operations: &[MeshOperationSpec],
) -> EngineResult<()> {
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let descriptor = ModelAuthoringFile {
        schema_version: 1,
        kind,
        source,
        operations,
    };
    std::fs::write(
        full_path,
        toml::to_string_pretty(&descriptor)
            .map_err(|error| EngineError::other(error.to_string()))?,
    )
    .map_err(|source| EngineError::Filesystem {
        path: full_path.to_path_buf(),
        source,
    })
}

fn scene_references_to_asset(
    scene: &engine_ecs::Scene,
    asset: engine_core::AssetId,
) -> Vec<serde_json::Value> {
    let mut references = Vec::new();
    for (entity, object) in scene.objects() {
        for component in &object.components {
            match component {
                engine_ecs::ComponentData::MeshRenderer(mesh) => {
                    if mesh.mesh == Some(asset) || mesh.material.asset == Some(asset) {
                        references.push(serde_json::json!({
                            "entity": entity_id_string(entity),
                            "name": object.name,
                            "component": "MeshRenderer",
                        }));
                    }
                }
                engine_ecs::ComponentData::SkinnedMeshRenderer(mesh) => {
                    if mesh.mesh == Some(asset) || mesh.material.asset == Some(asset) {
                        references.push(serde_json::json!({
                            "entity": entity_id_string(entity),
                            "name": object.name,
                            "component": "SkinnedMeshRenderer",
                        }));
                    }
                }
                _ => {}
            }
        }
    }
    references
}

fn scene_asset_missing(context: &ProjectContext, asset: engine_core::AssetId) -> bool {
    context
        .database
        .resolve_guid(engine_assets::AssetGuid::from_asset_id(asset))
        .is_err()
}

fn default_global_varg_root() -> PathBuf {
    if let Some(path) = std::env::var_os("VARG_HOME") {
        return PathBuf::from(path);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".varg")
}

fn operation_is_transactional(operation: &AgentOperation) -> bool {
    match operation {
        AgentOperation::WriteScript { .. }
        | AgentOperation::WriteFile { .. }
        | AgentOperation::ExecuteCommand { .. }
        | AgentOperation::UpdateProjectMemory { .. }
        | AgentOperation::UpdateUserMemory { .. }
        | AgentOperation::GenerateAsset { .. }
        | AgentOperation::ShowInViewport { .. }
        | AgentOperation::RunCommand { .. } => false,
        AgentOperation::SkillSearch { .. }
        | AgentOperation::SkillRead { .. }
        | AgentOperation::ToolSearch { .. }
        | AgentOperation::ReadFile { .. }
        | AgentOperation::CheckScript { .. }
        | AgentOperation::CreateTask { .. }
        | AgentOperation::UpdateTask { .. }
        | AgentOperation::Complete { .. }
        | AgentOperation::QueryDependencyGraph { .. }
        | AgentOperation::QuerySceneSemantic { .. } => true,
        AgentOperation::BatchOperation {
            operations,
            rollback_on_failure: _,
        } => operations.iter().all(operation_is_transactional),
        _ => true,
    }
}

#[derive(Clone, Copy, Debug)]
struct OperationAccess {
    requires_write: bool,
    requires_filesystem_write: bool,
    requires_process_execution: bool,
}

fn operation_access(operation: &AgentOperation) -> OperationAccess {
    match operation {
        AgentOperation::ReadFile { .. }
        | AgentOperation::CheckScript { .. }
        | AgentOperation::CreateTask { .. }
        | AgentOperation::UpdateTask { .. }
        | AgentOperation::Complete { .. }
        | AgentOperation::QueryDependencyGraph { .. }
        | AgentOperation::QuerySceneSemantic { .. }
        | AgentOperation::ToolSearch { .. }
        | AgentOperation::SkillSearch { .. }
        | AgentOperation::SkillRead { .. }
        | AgentOperation::RequestCapability { .. }
        | AgentOperation::GetSceneInfo { .. }
        | AgentOperation::GetObjectInfo { .. }
        | AgentOperation::GetAssetInfo { .. }
        | AgentOperation::CaptureViewport { .. }
        | AgentOperation::ValidateScene { .. }
        | AgentOperation::ShowInViewport { .. } => OperationAccess {
            requires_write: false,
            requires_filesystem_write: false,
            requires_process_execution: false,
        },
        AgentOperation::WriteScript { .. }
        | AgentOperation::WriteFile { .. }
        | AgentOperation::UpdateProjectMemory { .. }
        | AgentOperation::UpdateUserMemory { .. } => OperationAccess {
            requires_write: true,
            requires_filesystem_write: true,
            requires_process_execution: false,
        },
        AgentOperation::GenerateAsset { .. } => OperationAccess {
            requires_write: true,
            requires_filesystem_write: true,
            requires_process_execution: false,
        },
        AgentOperation::CreateMeshAsset { .. } | AgentOperation::ModifyMesh { .. } => {
            OperationAccess {
                requires_write: true,
                requires_filesystem_write: true,
                requires_process_execution: false,
            }
        }
        AgentOperation::RunCommand { .. } => OperationAccess {
            requires_write: true,
            requires_filesystem_write: false,
            requires_process_execution: true,
        },
        AgentOperation::ExecuteCommand { .. }
        | AgentOperation::CreateObject { .. }
        | AgentOperation::CreatePrimitive { .. }
        | AgentOperation::SetProperty { .. }
        | AgentOperation::SetTransform { .. }
        | AgentOperation::DuplicateObject { .. }
        | AgentOperation::SetMaterial { .. }
        | AgentOperation::RemoveComponent { .. }
        | AgentOperation::DestroyObject { .. }
        | AgentOperation::AttachBehavior { .. }
        | AgentOperation::MoveEntityTo { .. } => OperationAccess {
            requires_write: true,
            requires_filesystem_write: false,
            requires_process_execution: false,
        },
        AgentOperation::BatchOperation { operations, .. } => {
            let any_write = operations
                .iter()
                .any(|op| operation_access(op).requires_write);
            let any_fs = operations
                .iter()
                .any(|op| operation_access(op).requires_filesystem_write);
            let any_proc = operations
                .iter()
                .any(|op| operation_access(op).requires_process_execution);
            OperationAccess {
                requires_write: any_write,
                requires_filesystem_write: any_fs,
                requires_process_execution: any_proc,
            }
        }
    }
}

fn operation_capabilities(operation: &AgentOperation) -> Vec<String> {
    match operation {
        AgentOperation::RequestCapability {
            capabilities,
            tools,
            ..
        } => resolve_requested_capabilities(capabilities, tools),
        other => tools::metadata_for_tool(other.action_name())
            .map(|metadata| metadata.capabilities)
            .unwrap_or_default(),
    }
}

fn resolve_requested_capabilities(capabilities: &[String], tools: &[String]) -> Vec<String> {
    let mut resolved = capabilities
        .iter()
        .map(|capability| capability.trim().to_owned())
        .filter(|capability| !capability.is_empty())
        .collect::<Vec<_>>();

    for tool in tools {
        if let Some(metadata) = tools::metadata_for_tool(tool.trim()) {
            resolved.extend(metadata.capabilities);
        }
    }

    resolved.sort();
    resolved.dedup();
    resolved
}

fn capability_request_result(
    capabilities: &[String],
    policy: &PermissionPolicy,
) -> CapabilityRequestResult {
    let decisions = evaluate_capabilities(capabilities, policy);
    let all_approved = decisions
        .iter()
        .all(|result| result.decision == CapabilityDecision::Approved);
    CapabilityRequestResult {
        decisions,
        all_approved,
    }
}

fn evaluate_capabilities(
    capabilities: &[String],
    policy: &PermissionPolicy,
) -> Vec<CapabilityDecisionResult> {
    capabilities
        .iter()
        .map(|capability| evaluate_capability(capability, policy))
        .collect()
}

fn evaluate_capability(capability: &str, policy: &PermissionPolicy) -> CapabilityDecisionResult {
    let capability = capability.trim().to_owned();
    let (decision, reason) = match capability.as_str() {
        "context.read" | "scene.read" | "asset.read" | "tool.search" | "skill.search"
        | "skill.read" | "viewport.capture" => (
            CapabilityDecision::Approved,
            "read-only or transient capability is allowed by default".to_owned(),
        ),
        "scene.write.entity" | "scene.write.component" => match policy.write_mode {
            AgentWriteMode::ReadOnly => (
                CapabilityDecision::RequiresUserApproval,
                "scene writes require a write-capable policy".to_owned(),
            ),
            AgentWriteMode::Transactional | AgentWriteMode::Worktree => (
                CapabilityDecision::Approved,
                "scene write is allowed by the current write policy".to_owned(),
            ),
            AgentWriteMode::Direct if policy.direct_write => (
                CapabilityDecision::Approved,
                "direct scene write is explicitly allowed".to_owned(),
            ),
            AgentWriteMode::Direct => (
                CapabilityDecision::RequiresUserApproval,
                "direct scene write needs explicit direct-write approval".to_owned(),
            ),
        },
        "asset.write.generated" | "asset.write.mesh" | "asset.write.material" => {
            if policy.write_mode == AgentWriteMode::ReadOnly {
                (
                    CapabilityDecision::RequiresUserApproval,
                    "asset writes require a write-capable policy".to_owned(),
                )
            } else if policy.filesystem_write {
                (
                    CapabilityDecision::Approved,
                    "asset write is allowed by filesystem write policy".to_owned(),
                )
            } else {
                (
                    CapabilityDecision::Denied,
                    "filesystem writes are disabled by the current policy".to_owned(),
                )
            }
        }
        "script.execute.sandboxed" => {
            if policy.process_execution {
                (
                    CapabilityDecision::Approved,
                    "sandboxed script execution is allowed by process policy".to_owned(),
                )
            } else {
                (
                    CapabilityDecision::RequiresQuest,
                    "script execution should run in Quest or another approved sandbox".to_owned(),
                )
            }
        }
        "command.run" => {
            if policy.process_execution {
                (
                    CapabilityDecision::Approved,
                    "external command execution is allowed by process policy".to_owned(),
                )
            } else {
                (
                    CapabilityDecision::RequiresUserApproval,
                    "external command execution needs explicit approval".to_owned(),
                )
            }
        }
        "network.fetch_asset" => {
            if policy.network {
                (
                    CapabilityDecision::Approved,
                    "network asset fetch is allowed by network policy".to_owned(),
                )
            } else {
                (
                    CapabilityDecision::RequiresUserApproval,
                    "network asset fetch needs explicit network approval".to_owned(),
                )
            }
        }
        "quest.create" => (
            CapabilityDecision::RequiresQuest,
            "broad or persistent work should be routed through Quest".to_owned(),
        ),
        _ => (
            CapabilityDecision::Narrowed,
            "unknown capability needs a narrower policy rule before it can be approved".to_owned(),
        ),
    };

    CapabilityDecisionResult {
        capability,
        decision,
        reason,
    }
}

fn validate_operation_policy(
    operation: &AgentOperation,
    access: OperationAccess,
    policy: &PermissionPolicy,
) -> EngineResult<()> {
    if !access.requires_write {
        return Ok(());
    }

    if policy.write_mode == AgentWriteMode::ReadOnly {
        return Err(EngineError::config(format!(
            "{} requires write permission",
            operation.action_name()
        )));
    }

    if access.requires_filesystem_write && !policy.filesystem_write {
        return Err(EngineError::config(format!(
            "{} requires filesystem write permission",
            operation.action_name()
        )));
    }

    if access.requires_process_execution && !policy.process_execution {
        return Err(EngineError::config(format!(
            "{} requires process execution permission",
            operation.action_name()
        )));
    }

    if policy.write_mode == AgentWriteMode::Direct && !policy.direct_write {
        return Err(EngineError::config(format!(
            "{} requires explicit direct write permission",
            operation.action_name()
        )));
    }

    Ok(())
}

fn preview_operation(operation: &AgentOperation) -> String {
    match operation {
        AgentOperation::ExecuteCommand { command, .. } => {
            format!("Execute editor command `{command}`")
        }
        AgentOperation::WriteScript { path, .. } => {
            format!("Create or update Varg script `{path}`")
        }
        AgentOperation::CheckScript { paths } => {
            format!("Validate {} Varg script file(s)", paths.len())
        }
        AgentOperation::WriteFile { path, .. } => {
            format!("Create or update project file `{path}`")
        }
        AgentOperation::CreateObject { name, .. } => format!("Create object `{name}`"),
        AgentOperation::SetProperty {
            entity,
            component,
            field,
            ..
        } => format!("Set `{component}.{field}` on `{entity}`"),
        AgentOperation::RemoveComponent { entity, component } => {
            format!("Remove `{component}` from `{entity}`")
        }
        AgentOperation::DestroyObject { entity } => format!("Destroy object `{entity}`"),
        AgentOperation::ReadFile { path } => format!("Read project file `{path}`"),
        AgentOperation::CreateTask { title, .. } => format!("Create task `{title}`"),
        AgentOperation::UpdateTask { id, title, done } => {
            let mut parts = Vec::new();
            if let Some(title) = title {
                parts.push(format!("title `{title}`"));
            }
            if let Some(done) = done {
                parts.push(if *done {
                    "mark done".to_owned()
                } else {
                    "mark open".to_owned()
                });
            }
            if parts.is_empty() {
                format!("Update task `{id}`")
            } else {
                format!("Update task `{id}`: {}", parts.join(", "))
            }
        }
        AgentOperation::Complete { summary } => summary
            .as_ref()
            .map(|summary| format!("Complete: {summary}"))
            .unwrap_or_else(|| "Complete agent session".to_string()),
        AgentOperation::UpdateProjectMemory {
            append, heading, ..
        } => {
            if *append {
                format!(
                    "Append to project memory: {}",
                    heading.as_deref().unwrap_or("(no heading)")
                )
            } else {
                "Update project memory".to_string()
            }
        }
        AgentOperation::UpdateUserMemory { key, value } => {
            format!("Remember user preference `{key}`: {value}")
        }
        AgentOperation::QueryDependencyGraph { query, target } => {
            if let Some(t) = target {
                format!("Query dependency graph: {query} for `{t}`")
            } else {
                format!("Query dependency graph: {query}")
            }
        }
        AgentOperation::QuerySceneSemantic { query } => {
            format!("Search scene: '{query}'")
        }
        AgentOperation::ToolSearch { query, .. } => {
            format!("Search AI tools: '{query}'")
        }
        AgentOperation::SkillSearch { query, source, .. } => match source {
            Some(source) => format!("Search {source} Varg skills: '{query}'"),
            None => format!("Search Varg skills: '{query}'"),
        },
        AgentOperation::SkillRead { id, path } => match path {
            Some(path) => format!("Read Varg skill `{id}` reference `{path}`"),
            None => format!("Read Varg skill `{id}`"),
        },
        AgentOperation::RequestCapability {
            capabilities,
            tools,
            reason,
        } => {
            let mut parts = Vec::new();
            if !capabilities.is_empty() {
                parts.push(format!("capabilities {}", capabilities.join(", ")));
            }
            if !tools.is_empty() {
                parts.push(format!("tools {}", tools.join(", ")));
            }
            let target = if parts.is_empty() {
                "no requested capabilities".to_owned()
            } else {
                parts.join("; ")
            };
            match reason {
                Some(reason) => format!("Preflight permissions for {target}: {reason}"),
                None => format!("Preflight permissions for {target}"),
            }
        }
        AgentOperation::GetSceneInfo { .. } => "Inspect scene hierarchy".to_owned(),
        AgentOperation::GetObjectInfo { entity } => format!("Inspect object `{entity}`"),
        AgentOperation::GetAssetInfo { asset } => format!("Inspect asset `{asset}`"),
        AgentOperation::CreatePrimitive {
            name, primitive, ..
        } => format!("Create {primitive} primitive `{name}`"),
        AgentOperation::SetTransform { entity, .. } => {
            format!("Set transform for `{entity}`")
        }
        AgentOperation::DuplicateObject { entity, count, .. } => {
            format!("Duplicate `{entity}` {} time(s)", count.unwrap_or(1))
        }
        AgentOperation::SetMaterial { entity, .. } => format!("Set material on `{entity}`"),
        AgentOperation::CreateMeshAsset {
            path, assign_to, ..
        } => match assign_to {
            Some(entity) => format!("Create mesh asset `{path}` and assign to `{entity}`"),
            None => format!("Create mesh asset `{path}`"),
        },
        AgentOperation::ModifyMesh {
            source,
            target_path,
            ..
        } => format!("Record mesh modification from `{source}` to `{target_path}`"),
        AgentOperation::CaptureViewport {
            entity,
            output_path,
        } => match (entity, output_path) {
            (Some(entity), Some(path)) => format!("Capture viewport for `{entity}` to `{path}`"),
            (Some(entity), None) => format!("Capture viewport for `{entity}`"),
            (None, Some(path)) => format!("Capture viewport to `{path}`"),
            (None, None) => "Capture viewport".to_owned(),
        },
        AgentOperation::ValidateScene { .. } => "Validate scene references".to_owned(),
        AgentOperation::GenerateAsset {
            tool, target_path, ..
        } => {
            format!("Generate asset via `{tool}` → `{target_path}`")
        }
        AgentOperation::ShowInViewport {
            entity, highlight, ..
        } => {
            if *highlight {
                format!("Highlight `{entity}` in viewport")
            } else {
                format!("Focus `{entity}` in viewport")
            }
        }
        AgentOperation::BatchOperation {
            operations,
            rollback_on_failure,
        } => {
            let rollback_text = if *rollback_on_failure {
                " (transactional)"
            } else {
                ""
            };
            format!("Execute {} operations{}", operations.len(), rollback_text)
        }
        AgentOperation::AttachBehavior { entity, .. } => {
            format!("Attach behavior to `{entity}`")
        }
        AgentOperation::MoveEntityTo {
            entity,
            position,
            animated,
            ..
        } => {
            let anim_text = if *animated { " (animated)" } else { "" };
            format!("Move `{entity}` to {:?}{}", position, anim_text)
        }
        AgentOperation::RunCommand {
            command,
            args,
            working_dir,
            ..
        } => {
            let args_str = if args.is_empty() {
                String::new()
            } else {
                format!(" {}", args.join(" "))
            };
            let dir_str = working_dir
                .as_ref()
                .map(|d| format!(" in `{d}`"))
                .unwrap_or_default();
            format!("Run `{command}{args_str}`{dir_str}")
        }
    }
}

fn recovery_hint_for_success(operation: &AgentOperation) -> &'static str {
    match operation {
        AgentOperation::ExecuteCommand { .. }
        | AgentOperation::CreateObject { .. }
        | AgentOperation::CreatePrimitive { .. }
        | AgentOperation::SetProperty { .. }
        | AgentOperation::SetTransform { .. }
        | AgentOperation::DuplicateObject { .. }
        | AgentOperation::SetMaterial { .. }
        | AgentOperation::RemoveComponent { .. }
        | AgentOperation::DestroyObject { .. }
        | AgentOperation::AttachBehavior { .. }
        | AgentOperation::MoveEntityTo { .. } => "Use editor undo to revert this operation.",
        AgentOperation::WriteScript { .. } | AgentOperation::WriteFile { .. } => {
            "Review the generated script under the asset root and use version control or file history to revert it."
        }
        AgentOperation::CheckScript { .. } => {
            "No recovery needed; language-service validation is read-only."
        }
        AgentOperation::ReadFile { .. } | AgentOperation::QuerySceneSemantic { .. } => {
            "No recovery needed; this operation only read project data."
        }
        AgentOperation::GetSceneInfo { .. }
        | AgentOperation::GetObjectInfo { .. }
        | AgentOperation::GetAssetInfo { .. }
        | AgentOperation::ValidateScene { .. } => {
            "No recovery needed; this operation only read project data."
        }
        AgentOperation::CaptureViewport { .. } => {
            "No recovery needed; viewport capture requests are transient evidence."
        }
        AgentOperation::ToolSearch { .. } => {
            "No recovery needed; tool discovery does not grant permission."
        }
        AgentOperation::RequestCapability { .. } => {
            "No recovery needed; capability preflight does not grant permission."
        }
        AgentOperation::SkillSearch { .. } | AgentOperation::SkillRead { .. } => {
            "No recovery needed; skill discovery only reads scoped instruction files."
        }
        AgentOperation::CreateTask { .. } | AgentOperation::UpdateTask { .. } => {
            "No recovery needed; Copilot tasks only update transient editor UI."
        }
        AgentOperation::Complete { .. } => {
            "No recovery needed; completion does not mutate the project."
        }
        AgentOperation::UpdateProjectMemory { .. } => {
            "Revert by editing .aster/project.md or using version control."
        }
        AgentOperation::UpdateUserMemory { .. } => {
            "Remove the entry from .aster/memory.md to revert."
        }
        AgentOperation::QueryDependencyGraph { .. } => {
            "No recovery needed; this operation only read project data."
        }
        AgentOperation::GenerateAsset { .. } => {
            "Delete the generated asset file from the project to revert."
        }
        AgentOperation::CreateMeshAsset { .. } | AgentOperation::ModifyMesh { .. } => {
            "Delete the generated mesh descriptor from the asset root to revert."
        }
        AgentOperation::ShowInViewport { .. } => "No recovery needed; viewport state is transient.",
        AgentOperation::BatchOperation { .. } => {
            "Use editor undo to revert the batch, or undo individual operations if rollback was disabled."
        }
        AgentOperation::RunCommand { .. } => {
            "Command execution cannot be undone. Check side effects and revert manually if needed."
        }
    }
}

fn recovery_hint_for_failure(operation: &AgentOperation) -> &'static str {
    match operation {
        AgentOperation::WriteScript { .. } => {
            "Fix the script path or source, then regenerate or reapply the plan."
        }
        AgentOperation::CheckScript { .. } => {
            "Apply each diagnostic suggestion, then run check_script once more as the final acceptance step."
        }
        AgentOperation::WriteFile { .. } => {
            "Fix the file path or content, then regenerate or reapply the plan."
        }
        AgentOperation::ReadFile { .. } => "Check that the file path exists inside the project.",
        AgentOperation::CreateTask { .. } | AgentOperation::UpdateTask { .. } => {
            "Use a non-empty task id and title."
        }
        AgentOperation::ExecuteCommand { .. } => {
            "Check that the command is registered and available."
        }
        AgentOperation::CreateObject { .. }
        | AgentOperation::CreatePrimitive { .. }
        | AgentOperation::SetProperty { .. }
        | AgentOperation::SetTransform { .. }
        | AgentOperation::DuplicateObject { .. }
        | AgentOperation::SetMaterial { .. }
        | AgentOperation::RemoveComponent { .. }
        | AgentOperation::DestroyObject { .. } => {
            "Check entity identifiers, component names, and editor diagnostics before retrying."
        }
        AgentOperation::GetSceneInfo { .. }
        | AgentOperation::GetObjectInfo { .. }
        | AgentOperation::GetAssetInfo { .. }
        | AgentOperation::ValidateScene { .. } => {
            "Check identifiers and asset paths, then retry the inspection."
        }
        AgentOperation::CreateMeshAsset { .. } | AgentOperation::ModifyMesh { .. } => {
            "Check the mesh descriptor path and operation parameters."
        }
        AgentOperation::Complete { .. } => {
            "No recovery needed; completion does not mutate the project."
        }
        AgentOperation::UpdateProjectMemory { .. } | AgentOperation::UpdateUserMemory { .. } => {
            "Check file permissions for .aster/ directory."
        }
        AgentOperation::QueryDependencyGraph { .. } => {
            "Check that the query type and target are valid."
        }
        AgentOperation::QuerySceneSemantic { .. } => {
            "Try rephrasing the query. Supported patterns: 'all X', 'entities with X', 'X near Y', or direct name matches."
        }
        AgentOperation::ToolSearch { .. } => {
            "Try a shorter query or remove strict type, capability, stage, or risk filters."
        }
        AgentOperation::RequestCapability { .. } => {
            "Request a known capability or include tool names from tool_search results."
        }
        AgentOperation::SkillSearch { .. } => "Try a shorter query or remove the source filter.",
        AgentOperation::SkillRead { .. } => {
            "Use a resolved skill id from skill_search and a path inside that skill directory."
        }
        AgentOperation::GenerateAsset { .. } => {
            "Check tool availability, API key configuration, and network connectivity."
        }
        AgentOperation::ShowInViewport { .. } => {
            "Check that the entity identifier is valid and the entity exists in the scene."
        }
        AgentOperation::CaptureViewport { .. } => {
            "Check that the target entity exists and the output path stays inside the project."
        }
        AgentOperation::BatchOperation { .. } => {
            "Review the error message for which operation failed. Fix that operation and retry the batch."
        }
        AgentOperation::AttachBehavior { .. } => {
            "Verify the entity exists and the behavior file path is correct. For inline behaviors, check JSON syntax."
        }
        AgentOperation::MoveEntityTo { .. } => {
            "Check that the entity exists. Verify position coordinates are valid numbers."
        }
        AgentOperation::RunCommand { .. } => {
            "Check that the command exists and is accessible. Verify working directory and arguments."
        }
    }
}

/// Parses an entity identifier string like "1:1" or "entity:1:1" into an Entity.
fn parse_entity_id(entity_str: &str) -> EngineResult<engine_ecs::Entity> {
    let id_part = entity_str.strip_prefix("entity:").unwrap_or(entity_str);
    let parts: Vec<&str> = id_part.split(':').collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err(EngineError::config(format!(
            "invalid entity id: {entity_str}"
        )));
    }
    let slot = parts[0]
        .parse::<u32>()
        .map_err(|_| EngineError::config(format!("invalid entity id: {entity_str}")))?;
    let generation = match parts.get(1) {
        Some(value) => {
            let raw = value
                .parse::<u32>()
                .map_err(|_| EngineError::config(format!("invalid entity id: {entity_str}")))?;
            engine_core::Generation::from_raw(raw)?
        }
        None => engine_core::Generation::FIRST,
    };
    Ok(engine_ecs::Entity::from_handle(engine_core::Handle::new(
        slot, generation,
    )))
}

/// Converts a native `ToolCall` into an `AgentOperation`.
///
/// Maps the tool name to the corresponding operation variant and
/// deserializes the arguments JSON.
fn tool_call_to_operation(tc: &ToolCall) -> EngineResult<AgentOperation> {
    let args = &tc.arguments;
    match tc.name.as_str() {
        "create_object" => {
            let name = args["name"].as_str().unwrap_or("Untitled").to_owned();
            let position = args["position"].as_array().and_then(|a| {
                if a.len() == 3 {
                    Some([
                        a[0].as_f64().unwrap_or(0.0) as f32,
                        a[1].as_f64().unwrap_or(0.0) as f32,
                        a[2].as_f64().unwrap_or(0.0) as f32,
                    ])
                } else {
                    None
                }
            });
            let components: Vec<ComponentSpec> = args["components"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| serde_json::from_value(c.clone()).ok())
                        .collect()
                })
                .unwrap_or_default();
            Ok(AgentOperation::CreateObject {
                name,
                components,
                position,
            })
        }
        "write_script" => {
            let path = args["path"].as_str().unwrap_or("").to_owned();
            let source = args["source"].as_str().unwrap_or("").to_owned();
            Ok(AgentOperation::WriteScript { path, source })
        }
        "check_script" => {
            let paths = args["paths"]
                .as_array()
                .map(|paths| {
                    paths
                        .iter()
                        .filter_map(|path| path.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            Ok(AgentOperation::CheckScript { paths })
        }
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("").to_owned();
            let content = args["content"].as_str().unwrap_or("").to_owned();
            Ok(AgentOperation::WriteFile { path, content })
        }
        "generate_asset" => {
            let tool = args["tool"].as_str().unwrap_or("").to_owned();
            let prompt = args["prompt"].as_str().unwrap_or("").to_owned();
            let target_path = args["target_path"].as_str().unwrap_or("").to_owned();
            let style = args["style"].as_str().map(String::from);
            Ok(AgentOperation::GenerateAsset {
                tool,
                prompt,
                target_path,
                style,
            })
        }
        "set_property" => {
            let entity = args["entity"].as_str().unwrap_or("").to_owned();
            let component = args["component"].as_str().unwrap_or("").to_owned();
            let field = args["field"].as_str().unwrap_or("").to_owned();
            let value = args
                .get("value")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            Ok(AgentOperation::SetProperty {
                entity,
                component,
                field,
                value,
            })
        }
        "remove_component" => {
            let entity = args["entity"].as_str().unwrap_or("").to_owned();
            let component = args["component"].as_str().unwrap_or("").to_owned();
            Ok(AgentOperation::RemoveComponent { entity, component })
        }
        "destroy_object" => {
            let entity = args["entity"].as_str().unwrap_or("").to_owned();
            Ok(AgentOperation::DestroyObject { entity })
        }
        "read_file" => {
            let path = args["path"].as_str().unwrap_or("").to_owned();
            Ok(AgentOperation::ReadFile { path })
        }
        "create_task" => {
            let id = args["id"].as_str().unwrap_or("").to_owned();
            let title = args["title"].as_str().unwrap_or("").to_owned();
            Ok(AgentOperation::CreateTask { id, title })
        }
        "update_task" => {
            let id = args["id"].as_str().unwrap_or("").to_owned();
            let title = args
                .get("title")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            let done = args.get("done").and_then(|value| value.as_bool());
            Ok(AgentOperation::UpdateTask { id, title, done })
        }
        "execute_command" => {
            let command = args["command"].as_str().unwrap_or("").to_owned();
            let params = args
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            Ok(AgentOperation::ExecuteCommand { command, params })
        }
        "query_scene_semantic" => {
            let query = args["query"].as_str().unwrap_or("").to_owned();
            Ok(AgentOperation::QuerySceneSemantic { query })
        }
        "tool_search" => {
            let query = args["query"].as_str().unwrap_or("").to_owned();
            let types = string_array_arg(args, "types");
            let capabilities = string_array_arg(args, "capabilities");
            let stage = args["stage"].as_str().map(String::from);
            let risk_max = args["risk_max"].as_str().map(String::from);
            let limit = args["limit"].as_u64().map(|value| value as usize);
            Ok(AgentOperation::ToolSearch {
                query,
                types,
                capabilities,
                stage,
                risk_max,
                limit,
            })
        }
        "skill_search" => {
            let query = args["query"].as_str().unwrap_or("").to_owned();
            let source = args["source"].as_str().map(String::from);
            let limit = args["limit"].as_u64().map(|value| value as usize);
            Ok(AgentOperation::SkillSearch {
                query,
                source,
                limit,
            })
        }
        "skill_read" => {
            let id = args["id"].as_str().unwrap_or("").to_owned();
            let path = args["path"].as_str().map(String::from);
            Ok(AgentOperation::SkillRead { id, path })
        }
        "request_capability" => {
            let capabilities = string_array_arg(args, "capabilities");
            let tools = string_array_arg(args, "tools");
            let reason = args["reason"].as_str().map(String::from);
            Ok(AgentOperation::RequestCapability {
                capabilities,
                tools,
                reason,
            })
        }
        "get_scene_info" => Ok(AgentOperation::GetSceneInfo {
            include_components: args["include_components"].as_bool().unwrap_or(false),
        }),
        "get_object_info" => Ok(AgentOperation::GetObjectInfo {
            entity: args["entity"].as_str().unwrap_or("").to_owned(),
        }),
        "get_asset_info" => Ok(AgentOperation::GetAssetInfo {
            asset: args["asset"].as_str().unwrap_or("").to_owned(),
        }),
        "create_primitive" => Ok(AgentOperation::CreatePrimitive {
            name: args["name"].as_str().unwrap_or("").to_owned(),
            primitive: args["primitive"].as_str().unwrap_or("cube").to_owned(),
            transform: optional_arg(args, "transform")?,
            material: optional_arg(args, "material")?,
        }),
        "set_transform" => Ok(AgentOperation::SetTransform {
            entity: args["entity"].as_str().unwrap_or("").to_owned(),
            transform: serde_json::from_value(
                args.get("transform")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
            )
            .map_err(|error| EngineError::config(format!("invalid transform: {error}")))?,
        }),
        "duplicate_object" => Ok(AgentOperation::DuplicateObject {
            entity: args["entity"].as_str().unwrap_or("").to_owned(),
            count: args["count"].as_u64().map(|value| value as usize),
            offset: optional_arg(args, "offset")?,
        }),
        "set_material" => Ok(AgentOperation::SetMaterial {
            entity: args["entity"].as_str().unwrap_or("").to_owned(),
            material: serde_json::from_value(
                args.get("material")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
            )
            .map_err(|error| EngineError::config(format!("invalid material: {error}")))?,
        }),
        "create_mesh_asset" => Ok(AgentOperation::CreateMeshAsset {
            path: args["path"].as_str().unwrap_or("").to_owned(),
            operations: mesh_operations_arg(args)?,
            assign_to: args["assign_to"].as_str().map(String::from),
        }),
        "modify_mesh" => Ok(AgentOperation::ModifyMesh {
            source: args["source"].as_str().unwrap_or("").to_owned(),
            target_path: args["target_path"].as_str().unwrap_or("").to_owned(),
            operations: mesh_operations_arg(args)?,
        }),
        "capture_viewport" => Ok(AgentOperation::CaptureViewport {
            entity: args["entity"].as_str().map(String::from),
            output_path: args["output_path"].as_str().map(String::from),
        }),
        "validate_scene" => Ok(AgentOperation::ValidateScene {
            include_warnings: args["include_warnings"].as_bool().unwrap_or(true),
        }),
        "move_entity_to" => {
            let entity = args["entity"].as_str().unwrap_or("").to_owned();
            let position = args["position"]
                .as_array()
                .and_then(|a| {
                    if a.len() == 3 {
                        Some([
                            a[0].as_f64().unwrap_or(0.0) as f32,
                            a[1].as_f64().unwrap_or(0.0) as f32,
                            a[2].as_f64().unwrap_or(0.0) as f32,
                        ])
                    } else {
                        None
                    }
                })
                .unwrap_or([0.0, 0.0, 0.0]);
            let animated = args["animated"].as_bool().unwrap_or(false);
            let duration = args["duration"].as_f64().map(|d| d as f32);
            Ok(AgentOperation::MoveEntityTo {
                entity,
                position,
                animated,
                duration,
            })
        }
        "show_in_viewport" => {
            let entity = args["entity"].as_str().unwrap_or("").to_owned();
            let highlight = args["highlight"].as_bool().unwrap_or(false);
            let frame = args["frame"].as_bool().unwrap_or(false);
            Ok(AgentOperation::ShowInViewport {
                entity,
                highlight,
                frame,
            })
        }
        "attach_behavior" => {
            let entity = args["entity"].as_str().unwrap_or("").to_owned();
            if let Some(path) = args["behavior_path"].as_str() {
                Ok(AgentOperation::AttachBehavior {
                    entity,
                    behavior: BehaviorSource::File {
                        behavior_path: path.to_owned(),
                    },
                })
            } else if let Some(tree) = args.get("behavior_tree") {
                Ok(AgentOperation::AttachBehavior {
                    entity,
                    behavior: BehaviorSource::Inline {
                        behavior_tree: tree.clone(),
                    },
                })
            } else {
                Err(EngineError::config(
                    "attach_behavior requires behavior_path or behavior_tree",
                ))
            }
        }
        "run_command" => {
            let command = args["command"].as_str().unwrap_or("").to_owned();
            let args_vec: Vec<String> = args["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let working_dir = args["working_dir"].as_str().map(String::from);
            let timeout_ms = args["timeout_ms"].as_u64();
            Ok(AgentOperation::RunCommand {
                command,
                args: args_vec,
                working_dir,
                timeout_ms,
                capture_stdout: true,
                capture_stderr: true,
            })
        }
        "update_project_memory" => {
            let content = args["content"].as_str().unwrap_or("").to_owned();
            let append = args["append"].as_bool().unwrap_or(false);
            let heading = args["heading"].as_str().map(String::from);
            Ok(AgentOperation::UpdateProjectMemory {
                content,
                append,
                heading,
            })
        }
        "update_user_memory" => {
            let key = args["key"].as_str().unwrap_or("").to_owned();
            let value = args["value"].as_str().unwrap_or("").to_owned();
            Ok(AgentOperation::UpdateUserMemory { key, value })
        }
        "query_dependency_graph" => {
            let query = args["query"].as_str().unwrap_or("all").to_owned();
            let target = args["target"].as_str().map(String::from);
            Ok(AgentOperation::QueryDependencyGraph { query, target })
        }
        "complete" => {
            let summary = args["summary"].as_str().map(String::from);
            Ok(AgentOperation::Complete { summary })
        }
        other => Err(EngineError::config(format!("unknown tool call: {other}"))),
    }
}

fn string_array_arg(args: &serde_json::Value, key: &str) -> Vec<String> {
    args[key]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn optional_arg<T>(args: &serde_json::Value, key: &str) -> EngineResult<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    match args.get(key) {
        Some(value) if !value.is_null() => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| EngineError::config(format!("invalid {key}: {error}"))),
        _ => Ok(None),
    }
}

fn mesh_operations_arg(args: &serde_json::Value) -> EngineResult<Vec<MeshOperationSpec>> {
    serde_json::from_value(
        args.get("operations")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|error| EngineError::config(format!("invalid mesh operations: {error}")))
}

fn transform_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "position": {
                "type": "array",
                "items": { "type": "number" },
                "minItems": 3,
                "maxItems": 3
            },
            "rotation": {
                "type": "array",
                "items": { "type": "number" },
                "minItems": 4,
                "maxItems": 4,
                "description": "Quaternion [x, y, z, w]"
            },
            "scale": {
                "type": "array",
                "items": { "type": "number" },
                "minItems": 3,
                "maxItems": 3
            }
        }
    })
}

fn material_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "builtin": { "type": "string", "description": "Built-in material name" },
            "asset_path": { "type": "string", "description": "Material descriptor path relative to asset root" },
            "name": { "type": "string", "description": "Generated material display name" },
            "base_color": {
                "type": "array",
                "items": { "type": "number" },
                "minItems": 3,
                "maxItems": 3
            },
            "roughness": { "type": "number" },
            "metallic": { "type": "number" }
        }
    })
}

fn mesh_operations_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "type": { "type": "string", "description": "Operation kind: cube, bevel, inset, extrude, mirror, boolean, array" },
                "params": { "type": "object", "description": "Operation parameters" }
            },
            "required": ["type"]
        }
    })
}

fn modeling_object_schema(properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

/// Returns tool definitions for all supported agent operations.
///
/// These are sent to the model so it can request operations via native
/// tool calling instead of embedding JSON in text.
pub fn agent_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        tools::tool_search_definition(),
        tools::request_capability_definition(),
        skills::skill_search_definition(),
        skills::skill_read_definition(),
        ToolDefinition {
            name: "create_object".into(),
            description: "Create a new game object with optional components and position.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Display name for the object" },
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "description": "Initial position [x, y, z]"
                    },
                    "components": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "description": "Component type: Camera, MeshRenderer, Light, Rigidbody, Collider, AudioSource, Script, Sprite2D, ParticleEmitter" },
                                "properties": { "type": "object", "description": "Initial component properties" }
                            },
                            "required": ["type"]
                        },
                        "description": "Components to attach"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "write_script".into(),
            description: "Create or update a Varg script file in the project.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to asset root, using the .varg extension, e.g. scripts/player.varg" },
                    "source": { "type": "string", "description": "Varg source code" }
                },
                "required": ["path", "source"]
            }),
        },
        ToolDefinition {
            name: "check_script".into(),
            description: "Run strict final acceptance validation for one or more .varg files. Returns precise diagnostics with location, cause, source line, and a concrete fix suggestion. Call once after all script edits, before complete.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "description": "Asset-root-relative .varg paths to validate together"
                    }
                },
                "required": ["paths"]
            }),
        },
        ToolDefinition {
            name: "write_file".into(),
            description: "Create or update a UTF-8 text file relative to the project root. Use this for Rust, TypeScript, docs, schemas, configs, and other non-asset files.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to project root, e.g. crates/foo/src/lib.rs" },
                    "content": { "type": "string", "description": "Complete file content to write" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "generate_asset".into(),
            description: "Request generation of an external asset into a project asset path. Use only when the task specifically needs generated media or imported model assets.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "tool": { "type": "string", "description": "Asset generator name, e.g. gpt-image, suno, meshy" },
                    "prompt": { "type": "string", "description": "Natural-language generation prompt" },
                    "target_path": { "type": "string", "description": "Target path relative to the asset root" },
                    "style": { "type": "string", "description": "Optional style hint or generator parameters" }
                },
                "required": ["tool", "prompt", "target_path"]
            }),
        },
        ToolDefinition {
            name: "set_property".into(),
            description: "Modify a component field on an entity.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity name or ID (e.g. 'Player' or '1:1')" },
                    "component": { "type": "string", "description": "Component type (e.g. 'Rigidbody')" },
                    "field": { "type": "string", "description": "Field name to modify" },
                    "value": { "description": "New value for the field" }
                },
                "required": ["entity", "component", "field", "value"]
            }),
        },
        ToolDefinition {
            name: "remove_component".into(),
            description: "Remove a component from an entity.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity name or ID" },
                    "component": { "type": "string", "description": "Component type to remove" }
                },
                "required": ["entity", "component"]
            }),
        },
        ToolDefinition {
            name: "destroy_object".into(),
            description: "Delete an entity from the scene.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity name or ID" }
                },
                "required": ["entity"]
            }),
        },
        ToolDefinition {
            name: "read_file".into(),
            description: "Read a source file from the project.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to project root" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "create_task".into(),
            description: "Create a short-lived Copilot task for the editor task card. Use this for multi-step short tasks before concrete project operations.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "id": { "type": "string", "description": "Stable short id, e.g. inspect-scene or attach-scripts" },
                    "title": { "type": "string", "description": "Short task title shown to the user" }
                },
                "required": ["id", "title"]
            }),
        },
        ToolDefinition {
            name: "update_task".into(),
            description: "Update a Copilot task title or completion state in the editor task card.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "id": { "type": "string", "description": "Task id from create_task" },
                    "title": { "type": "string", "description": "Optional replacement title" },
                    "done": { "type": "boolean", "description": "Whether the task is complete" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "execute_command".into(),
            description: "Execute a registered editor command.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Command identifier (e.g. 'gameobject.create_empty')" },
                    "params": { "type": "object", "description": "Optional command parameters" }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "query_scene_semantic".into(),
            description: "Search for entities in the scene using natural language.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural language query: 'all enemies', 'entities with Camera', 'Player near Enemy', or direct name" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "get_scene_info".into(),
            description: "Return scene hierarchy, transforms, components, cameras, lights, and object summaries.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "include_components": { "type": "boolean", "description": "Include full component payloads instead of type summaries" }
                }
            }),
        },
        ToolDefinition {
            name: "get_object_info".into(),
            description: "Return detailed component, transform, mesh, material, and bounds information for one object.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity name or ID" }
                },
                "required": ["entity"]
            }),
        },
        ToolDefinition {
            name: "get_asset_info".into(),
            description: "Return asset metadata and scene references for an asset path or GUID.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "asset": { "type": "string", "description": "Asset-root-relative path or GUID" }
                },
                "required": ["asset"]
            }),
        },
        ToolDefinition {
            name: "create_primitive".into(),
            description: "Create a primitive mesh object with transform and material fields.".into(),
            parameters: modeling_object_schema(serde_json::json!({
                "name": { "type": "string", "description": "Object name" },
                "primitive": { "type": "string", "description": "cube, sphere, plane, quad, capsule, or cylinder" },
                "transform": transform_schema(),
                "material": material_schema()
            }), &["name", "primitive"]),
        },
        ToolDefinition {
            name: "set_transform".into(),
            description: "Set an object's transform using structured position, rotation, and scale values.".into(),
            parameters: modeling_object_schema(serde_json::json!({
                "entity": { "type": "string", "description": "Entity name or ID" },
                "transform": transform_schema()
            }), &["entity", "transform"]),
        },
        ToolDefinition {
            name: "duplicate_object".into(),
            description: "Duplicate an object with an optional repeated transform offset.".into(),
            parameters: modeling_object_schema(serde_json::json!({
                "entity": { "type": "string", "description": "Entity name or ID" },
                "count": { "type": "integer", "description": "Number of copies" },
                "offset": transform_schema()
            }), &["entity"]),
        },
        ToolDefinition {
            name: "set_material".into(),
            description: "Create or assign material parameters for an object's MeshRenderer.".into(),
            parameters: modeling_object_schema(serde_json::json!({
                "entity": { "type": "string", "description": "Entity name or ID" },
                "material": material_schema()
            }), &["entity", "material"]),
        },
        ToolDefinition {
            name: "create_mesh_asset".into(),
            description: "Write a structured .vmodel TOML authoring file and optionally assign it to an entity.".into(),
            parameters: modeling_object_schema(serde_json::json!({
                "path": { "type": "string", "description": "Asset-root-relative target path, e.g. models/crate.vmodel" },
                "operations": mesh_operations_schema(),
                "assign_to": { "type": "string", "description": "Optional entity name or ID to receive the mesh" }
            }), &["path", "operations"]),
        },
        ToolDefinition {
            name: "modify_mesh".into(),
            description: "Record structured mesh operations into a derived .vmodel TOML authoring file.".into(),
            parameters: modeling_object_schema(serde_json::json!({
                "source": { "type": "string", "description": "Source asset path/GUID or entity name/ID" },
                "target_path": { "type": "string", "description": "Asset-root-relative target descriptor path" },
                "operations": mesh_operations_schema()
            }), &["source", "target_path", "operations"]),
        },
        ToolDefinition {
            name: "capture_viewport".into(),
            description: "Request viewport preview evidence for visual feedback.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Optional entity name or ID to frame" },
                    "output_path": { "type": "string", "description": "Optional future screenshot path relative to project root" }
                }
            }),
        },
        ToolDefinition {
            name: "validate_scene".into(),
            description: "Check scene references, missing assets, and basic authoring constraints.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "include_warnings": { "type": "boolean", "description": "Include advisory warnings" }
                }
            }),
        },
        ToolDefinition {
            name: "move_entity_to".into(),
            description: "Move an entity to a target position, optionally animated.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity name or ID" },
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "description": "Target position [x, y, z]"
                    },
                    "animated": { "type": "boolean", "description": "Whether to animate the movement" },
                    "duration": { "type": "number", "description": "Animation duration in seconds" }
                },
                "required": ["entity", "position"]
            }),
        },
        ToolDefinition {
            name: "show_in_viewport".into(),
            description: "Highlight or focus an entity in the editor viewport.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity name or ID" },
                    "highlight": { "type": "boolean", "description": "Add outline highlight" },
                    "frame": { "type": "boolean", "description": "Focus camera on entity" }
                },
                "required": ["entity"]
            }),
        },
        ToolDefinition {
            name: "attach_behavior".into(),
            description: "Attach a declarative behavior tree to an entity.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity name or ID" },
                    "behavior_path": { "type": "string", "description": "Path to behavior file relative to asset root" },
                    "behavior_tree": { "type": "object", "description": "Inline behavior tree JSON" }
                },
                "required": ["entity"]
            }),
        },
        ToolDefinition {
            name: "run_command".into(),
            description: "Execute a shell command or external process.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Command to execute (e.g. 'cargo', 'python')" },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "Command arguments" },
                    "working_dir": { "type": "string", "description": "Working directory relative to project root" },
                    "timeout_ms": { "type": "number", "description": "Timeout in milliseconds (default 30000)" }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "update_project_memory".into(),
            description: "Update the project memory file (.aster/project.md).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "New content or section to append" },
                    "append": { "type": "boolean", "description": "Append as new section instead of replacing" },
                    "heading": { "type": "string", "description": "Section heading when appending" }
                },
                "required": ["content"]
            }),
        },
        ToolDefinition {
            name: "update_user_memory".into(),
            description: "Record an observed user pattern or preference.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Memory key (e.g. 'naming', 'style')" },
                    "value": { "type": "string", "description": "Preference description" }
                },
                "required": ["key", "value"]
            }),
        },
        ToolDefinition {
            name: "query_dependency_graph".into(),
            description: "Query the project dependency graph.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "enum": ["all", "entities", "scripts", "edges_for"], "description": "Query type" },
                    "target": { "type": "string", "description": "Filter target for 'edges_for' query" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "complete".into(),
            description: "Signal that the task is complete with an optional summary.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "string", "description": "Summary of what was accomplished" }
                }
            }),
        },
    ]
}

/// Returns the lean native tool set used by short in-editor Copilot turns.
///
/// Copilot operates on the current editor scene and project files. It should not
/// spend context on long-running planning or memory-management tools that do
/// not directly advance the visible editor task.
pub fn copilot_tool_definitions() -> Vec<ToolDefinition> {
    tool_definitions_for_exposure(&[ToolExposure::Direct, ToolExposure::DirectModelOnly])
}

/// Returns tool definitions whose metadata has one of the requested exposures.
pub fn tool_definitions_for_exposure(exposures: &[ToolExposure]) -> Vec<ToolDefinition> {
    agent_tool_definitions()
        .into_iter()
        .filter(|tool| {
            tools::metadata_for_tool(&tool.name)
                .map(|metadata| exposures.contains(&metadata.exposure))
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubModel {
        content: String,
    }

    impl StubModel {
        fn new(content: impl Into<String>) -> Self {
            Self {
                content: content.into(),
            }
        }
    }

    impl AiModel for StubModel {
        fn chat(&self, _request: AiRequest) -> EngineResult<AiResponse> {
            Ok(AiResponse {
                content: self.content.clone(),
                thinking: String::new(),
                tool_calls: Vec::new(),
            })
        }
    }

    fn temp_project_context() -> ProjectContext {
        use engine_ecs::ProjectManifest;

        let scene = engine_ecs::Scene::new();
        let database =
            engine_assets::AssetDatabase::new(std::env::temp_dir(), std::env::temp_dir());

        ProjectContext {
            manifest: ProjectManifest::example(),
            scene,
            database,
            registry: engine_assets::AssetRegistry::default(),
            assets: Vec::new(),
            asset_imports: Vec::new(),
            scene_dirty: false,
            root: std::env::temp_dir(),
            scene_path: std::env::temp_dir().join("main.vscene"),
        }
    }

    fn temp_project_context_at(root: PathBuf) -> ProjectContext {
        use engine_ecs::ProjectManifest;

        let manifest = ProjectManifest::example();
        std::fs::create_dir_all(root.join(&manifest.asset_root)).unwrap();
        let scene = engine_ecs::Scene::new();
        let database = engine_assets::AssetDatabase::new(
            root.join(&manifest.asset_root),
            root.join("builtin"),
        );

        ProjectContext {
            manifest,
            scene,
            database,
            registry: engine_assets::AssetRegistry::default(),
            assets: Vec::new(),
            asset_imports: Vec::new(),
            scene_dirty: false,
            root: root.clone(),
            scene_path: root.join("main.vscene"),
        }
    }

    #[test]
    fn agent_session_initializes_with_project() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let project_root = manifest_dir.join("../engine-editor/../../examples/project");

        let ctx = ProjectContext::open(&project_root).unwrap();
        let session = AgentSession::new(ctx).unwrap();

        assert!(session.commands.list_executable().count() > 0);
        assert!(session.undo_stack.can_undo() == false);
    }

    #[test]
    fn parse_entity_id_handles_both_formats() {
        let entity = parse_entity_id("1:1").unwrap();
        assert_eq!(entity.handle().slot(), 1);

        let entity = parse_entity_id("entity:2:3").unwrap();
        assert_eq!(entity.handle().slot(), 2);
    }

    #[test]
    fn check_script_tool_is_exposed_as_read_only_final_validation() {
        let tool = agent_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "check_script")
            .expect("check_script tool should be exposed");
        assert!(tool.description.contains("final acceptance"));
        assert_eq!(tool.parameters["required"][0], "paths");

        let operation = tool_call_to_operation(&ToolCall {
            id: "check-1".into(),
            name: "check_script".into(),
            arguments: serde_json::json!({
                "paths": ["scripts/player.varg", "scripts/enemy.varg"]
            }),
        })
        .unwrap();
        assert!(matches!(
            operation,
            AgentOperation::CheckScript { ref paths } if paths.len() == 2
        ));
        assert!(!operation_access(&operation).requires_write);
    }

    #[test]
    fn copilot_tools_are_short_task_focused() {
        let tools = copilot_tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<std::collections::BTreeSet<_>>();

        assert!(tools.contains("write_script"));
        assert!(tools.contains("create_object"));
        assert!(tools.contains("create_task"));
        assert!(tools.contains("update_task"));
        assert!(tools.contains("tool_search"));
        assert!(tools.contains("request_capability"));
        assert!(tools.contains("skill_search"));
        assert!(tools.contains("skill_read"));
        assert!(tools.contains("check_script"));
        assert!(tools.contains("complete"));
        assert!(!tools.contains("update_project_memory"));
        assert!(!tools.contains("update_user_memory"));
        assert!(!tools.contains("query_dependency_graph"));
        assert!(!tools.contains("attach_behavior"));
        assert!(!tools.contains("generate_asset"));
    }

    #[test]
    fn non_hidden_registry_tools_have_native_definitions() {
        let definitions = agent_tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<std::collections::BTreeSet<_>>();

        for metadata in tools::tool_registry() {
            if metadata.exposure == ToolExposure::Hidden {
                continue;
            }
            assert!(
                definitions.contains(&metadata.name),
                "tool registry metadata lacks native definition for {}",
                metadata.name
            );
        }
    }

    #[test]
    fn generate_asset_tool_call_maps_to_operation() {
        let operation = tool_call_to_operation(&ToolCall {
            id: "asset-1".into(),
            name: "generate_asset".into(),
            arguments: serde_json::json!({
                "tool": "gpt-image",
                "prompt": "small mossy rock texture",
                "target_path": "textures/mossy_rock.png",
                "style": "pbr albedo"
            }),
        })
        .unwrap();

        assert!(matches!(
            operation,
            AgentOperation::GenerateAsset { ref tool, ref target_path, .. }
                if tool == "gpt-image" && target_path == "textures/mossy_rock.png"
        ));
    }

    #[test]
    fn skill_tool_calls_map_to_read_only_operations() {
        let search = tool_call_to_operation(&ToolCall {
            id: "skill-search-1".into(),
            name: "skill_search".into(),
            arguments: serde_json::json!({
                "query": "combat authoring",
                "source": "project"
            }),
        })
        .unwrap();
        assert!(matches!(
            search,
            AgentOperation::SkillSearch {
                ref query,
                source: Some(ref source),
                ..
            } if query == "combat authoring" && source == "project"
        ));
        assert!(!operation_access(&search).requires_write);

        let read = tool_call_to_operation(&ToolCall {
            id: "skill-read-1".into(),
            name: "skill_read".into(),
            arguments: serde_json::json!({
                "id": "project://skills/combat",
                "path": "references/moves.md"
            }),
        })
        .unwrap();
        assert!(matches!(
            read,
            AgentOperation::SkillRead {
                ref id,
                path: Some(ref path),
            } if id == "project://skills/combat" && path == "references/moves.md"
        ));
        assert!(!operation_access(&read).requires_write);
    }

    #[test]
    fn request_capability_tool_call_preflights_without_granting_permission() {
        let op = tool_call_to_operation(&ToolCall {
            id: "cap-1".into(),
            name: "request_capability".into(),
            arguments: serde_json::json!({
                "capabilities": ["scene.write.entity"],
                "tools": ["generate_asset"],
                "reason": "Create a generated crate and place it in scene"
            }),
        })
        .unwrap();

        assert!(matches!(
            op,
            AgentOperation::RequestCapability {
                ref capabilities,
                ref tools,
                ..
            } if capabilities == &vec!["scene.write.entity".to_owned()]
                && tools == &vec!["generate_asset".to_owned()]
        ));
        assert!(!operation_access(&op).requires_write);

        let requested = operation_capabilities(&op);
        assert!(requested.contains(&"scene.write.entity".to_owned()));
        assert!(requested.contains(&"asset.write.generated".to_owned()));
        let decisions = evaluate_capabilities(&requested, &PermissionPolicy::read_only());
        assert!(decisions.iter().any(|result| {
            result.capability == "scene.write.entity"
                && result.decision == CapabilityDecision::RequiresUserApproval
        }));
    }

    #[test]
    fn modeling_tool_calls_map_to_operations_and_capabilities() {
        let primitive = tool_call_to_operation(&ToolCall {
            id: "primitive-1".into(),
            name: "create_primitive".into(),
            arguments: serde_json::json!({
                "name": "Crate",
                "primitive": "cube",
                "transform": { "position": [1.0, 2.0, 3.0], "scale": [2.0, 2.0, 2.0] },
                "material": { "builtin": "debug/default" }
            }),
        })
        .unwrap();
        assert!(matches!(
            primitive,
            AgentOperation::CreatePrimitive {
                ref name,
                ref primitive,
                ..
            } if name == "Crate" && primitive == "cube"
        ));
        let capabilities = operation_capabilities(&primitive);
        assert!(capabilities.contains(&"scene.write.entity".to_owned()));
        assert!(capabilities.contains(&"scene.write.component".to_owned()));

        let mesh = tool_call_to_operation(&ToolCall {
            id: "mesh-1".into(),
            name: "create_mesh_asset".into(),
            arguments: serde_json::json!({
                "path": "models/crate.vmodel",
                "operations": [
                    { "type": "cube", "params": { "size": [2.0, 1.0, 1.0] } },
                    { "type": "bevel", "params": { "amount": 0.05 } }
                ]
            }),
        })
        .unwrap();
        assert!(matches!(
            mesh,
            AgentOperation::CreateMeshAsset { ref operations, .. } if operations.len() == 2
        ));
        assert!(operation_capabilities(&mesh).contains(&"asset.write.mesh".to_owned()));
    }

    #[test]
    fn modeling_operations_create_scene_and_asset_evidence() {
        let root = std::env::temp_dir().join(format!("varg-ai-modeling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ctx = temp_project_context_at(root.clone());
        let mut session = AgentSession::new(ctx).unwrap();

        session
            .execute_operation(&AgentOperation::CreatePrimitive {
                name: "AI_Crate".into(),
                primitive: "cube".into(),
                transform: Some(TransformSpec {
                    position: Some([1.0, 2.0, 3.0]),
                    rotation: None,
                    scale: Some([2.0, 2.0, 2.0]),
                }),
                material: Some(MaterialSpec {
                    builtin: Some("debug/default".into()),
                    ..MaterialSpec::default()
                }),
            })
            .unwrap();
        let entity = session.context.scene.find_by_name("AI_Crate").unwrap();
        let transform = session.context.scene.transforms().local(entity).unwrap();
        assert!((transform.translation.z - 3.0).abs() < 0.001);
        assert!(
            session
                .context
                .scene
                .components(entity)
                .unwrap()
                .iter()
                .any(|component| component.type_id() == "MeshRenderer")
        );

        session
            .execute_operation(&AgentOperation::CreateMeshAsset {
                path: "models/crate.vmodel".into(),
                operations: vec![MeshOperationSpec {
                    operation_type: "cube".into(),
                    params: serde_json::json!({ "size": [1.0, 1.0, 1.0] }),
                }],
                assign_to: Some("AI_Crate".into()),
            })
            .unwrap();
        let model_path = root.join("assets/models/crate.vmodel");
        assert!(model_path.exists());
        let model_source = std::fs::read_to_string(model_path).unwrap();
        assert!(model_source.contains("schema_version = 1"));
        assert!(model_source.contains("kind = \"generated_model\""));
        assert!(model_source.contains("[[operations]]"));

        session
            .execute_operation(&AgentOperation::ValidateScene {
                include_warnings: true,
            })
            .unwrap();
        assert!(
            session
                .console
                .entries()
                .iter()
                .any(|entry| entry.source.subsystem == "ai-agent-validation")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tool_search_discovery_does_not_approve_discovered_write_capability() {
        let mut session = AgentSession::new(temp_project_context()).unwrap();
        let model = StubModel::new(
            r#"[{"action":"tool_search","query":"create scene object","capabilities":["scene.write.entity"]},{"action":"create_object","name":"Denied"}]"#,
        );

        let result = session.plan(&model, "find then create", PermissionPolicy::read_only());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("create_object requires write permission"));
    }

    #[test]
    fn task_tools_are_read_only_copilot_ui_updates() {
        let create = tool_call_to_operation(&ToolCall {
            id: "task-1".into(),
            name: "create_task".into(),
            arguments: serde_json::json!({
                "id": "write-ocean",
                "title": "Write ocean scripts"
            }),
        })
        .unwrap();
        assert!(matches!(create, AgentOperation::CreateTask { .. }));
        assert!(!operation_access(&create).requires_write);

        let update = tool_call_to_operation(&ToolCall {
            id: "task-2".into(),
            name: "update_task".into(),
            arguments: serde_json::json!({
                "id": "write-ocean",
                "done": true
            }),
        })
        .unwrap();
        assert!(matches!(
            update,
            AgentOperation::UpdateTask {
                done: Some(true),
                ..
            }
        ));
        assert!(!operation_access(&update).requires_write);
    }

    #[test]
    fn write_script_rescans_assets_after_file_creation() {
        let root = std::env::temp_dir().join(format!(
            "varg-ai-write-script-rescan-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let ctx = temp_project_context_at(root.clone());
        let mut session = AgentSession::new(ctx).unwrap();

        session
            .execute_operation(&AgentOperation::WriteScript {
                path: "scripts/ocean_surface.varg".to_owned(),
                source: "script OceanSurface {\n    func update(_ dt: Float) {\n    }\n}\n"
                    .to_owned(),
            })
            .unwrap();

        assert!(root.join("assets/scripts/ocean_surface.varg").exists());
        assert!(
            session
                .context
                .assets
                .iter()
                .any(|asset| asset.source_path == PathBuf::from("scripts/ocean_surface.varg"))
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn build_component_creates_known_types() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let project_root = manifest_dir.join("../engine-editor/../../examples/project");
        let ctx = ProjectContext::open(&project_root).unwrap();
        let session = AgentSession::new(ctx).unwrap();

        let spec = ComponentSpec {
            component_type: "Camera".into(),
            properties: serde_json::Value::Null,
        };
        let component = session.build_component(&spec).unwrap();
        assert_eq!(component.type_id(), "Camera");

        let spec = ComponentSpec {
            component_type: "Rigidbody".into(),
            properties: serde_json::Value::Null,
        };
        let component = session.build_component(&spec).unwrap();
        assert_eq!(component.type_id(), "Rigidbody");
    }

    #[test]
    fn execute_create_object_adds_to_scene() {
        use engine_ecs::ProjectManifest;

        let mut scene = engine_ecs::Scene::new();
        // Pre-populate with one object so the scene is non-trivial
        scene.create_object("Existing").unwrap();

        let database =
            engine_assets::AssetDatabase::new(std::env::temp_dir(), std::env::temp_dir());

        let ctx = ProjectContext {
            manifest: ProjectManifest::example(),
            scene,
            database,
            registry: engine_assets::AssetRegistry::default(),
            assets: Vec::new(),
            asset_imports: Vec::new(),
            scene_dirty: false,
            root: std::env::temp_dir(),
            scene_path: std::env::temp_dir().join("main.vscene"),
        };

        let mut session = AgentSession::new(ctx).unwrap();
        let op = AgentOperation::CreateObject {
            name: "AI_Player".into(),
            components: vec![ComponentSpec {
                component_type: "Rigidbody".into(),
                properties: serde_json::Value::Null,
            }],
            position: Some([0.0, 5.0, 0.0]),
        };

        session.execute_operation(&op).unwrap();

        let entity = session.context.scene.find_by_name("AI_Player").unwrap();
        let transform = session.context.scene.transforms().local(entity).unwrap();
        assert!((transform.translation.y - 5.0).abs() < 0.001);

        let components = session.context.scene.components(entity).unwrap();
        assert!(components.iter().any(|c| c.type_id() == "Rigidbody"));
    }

    #[test]
    fn plan_accepts_read_only_operations_under_read_only_policy() {
        let ctx = temp_project_context();
        let mut session = AgentSession::new(ctx).unwrap();
        let model = StubModel::new(
            r#"[
                {"action": "read_file", "path": "README.md"},
                {"action": "complete", "summary": "inspected project"}
            ]"#,
        );

        let plan = session
            .plan(
                &model,
                "what is in this project?",
                PermissionPolicy::read_only(),
            )
            .unwrap();

        assert!(plan.read_only);
        assert!(!plan.requires_write);
        assert_eq!(plan.operations.len(), 2);
        assert_eq!(plan.operations[0].preview, "Read project file `README.md`");
        assert!(session.context.scene.find_by_name("AI_Player").is_none());
    }

    #[test]
    fn plan_rejects_write_operations_under_read_only_policy() {
        let ctx = temp_project_context();
        let mut session = AgentSession::new(ctx).unwrap();
        let model = StubModel::new(r#"[{"action": "create_object", "name": "AI_Player"}]"#);

        let result = session.plan(&model, "create a player", PermissionPolicy::read_only());

        assert!(result.is_err());
        assert!(session.context.scene.find_by_name("AI_Player").is_none());
    }

    #[test]
    fn apply_plan_executes_only_after_approval() {
        let ctx = temp_project_context();
        let mut session = AgentSession::new(ctx).unwrap();
        let model = StubModel::new(
            r#"[
                {"action": "create_object", "name": "AI_Player"},
                {"action": "complete", "summary": "created player"}
            ]"#,
        );

        let plan = session
            .plan(
                &model,
                "create a player",
                PermissionPolicy::transactional_write(),
            )
            .unwrap();

        assert!(plan.requires_write);
        assert!(session.context.scene.find_by_name("AI_Player").is_none());

        let outcome = session.apply_plan(&plan).unwrap();

        assert_eq!(outcome.operations_performed, 1);
        assert!(outcome.completed);
        assert_eq!(outcome.summary.as_deref(), Some("created player"));
        assert!(session.context.scene.find_by_name("AI_Player").is_some());
        assert_eq!(outcome.trace_entries.len(), 2);
    }

    #[test]
    fn malformed_model_output_records_console_diagnostic() {
        let ctx = temp_project_context();
        let mut session = AgentSession::new(ctx).unwrap();
        let model =
            StubModel::new("I will create a player.\n```aster_operations\n[{invalid json}]\n```");

        let result = session.plan(
            &model,
            "create a player",
            PermissionPolicy::transactional_write(),
        );

        assert!(result.is_err());
        assert_eq!(session.console.entries().len(), 1);
        assert!(
            session.console.entries()[0]
                .message
                .contains("parse_response")
        );
    }

    #[test]
    fn rollback_batch_restores_context_and_undo_state() {
        let ctx = temp_project_context();
        let mut session = AgentSession::new(ctx).unwrap();
        let operations = vec![
            AgentOperation::CreateObject {
                name: "Temporary".into(),
                components: Vec::new(),
                position: None,
            },
            AgentOperation::SetProperty {
                entity: "missing".into(),
                component: "Transform".into(),
                field: "translation".into(),
                value: serde_json::json!([1.0, 2.0, 3.0]),
            },
        ];

        let result = session.execute_batch_operation_inner(&operations, true, 0);

        assert!(result.is_err());
        assert!(session.context.scene.find_by_name("Temporary").is_none());
        assert!(!session.undo_stack.can_undo());
    }

    #[test]
    fn rollback_batch_rejects_external_side_effects() {
        let ctx = temp_project_context();
        let mut session = AgentSession::new(ctx).unwrap();
        let operations = vec![AgentOperation::RunCommand {
            command: "ignored".into(),
            args: Vec::new(),
            working_dir: None,
            timeout_ms: None,
            capture_stdout: true,
            capture_stderr: true,
        }];

        let result = session.execute_batch_operation_inner(&operations, true, 0);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("in-memory transactional")
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_with_output_larger_than_pipe_capacity_completes() {
        let ctx = temp_project_context();
        let mut session = AgentSession::new(ctx).unwrap();

        let result = session.execute_run_command(
            "sh",
            &["-c".into(), "yes x | head -c 262144".into()],
            None,
            Some(5_000),
            true,
            true,
        );

        assert!(result.is_ok(), "{result:?}");
    }
}
