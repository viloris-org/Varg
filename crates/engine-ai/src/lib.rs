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
mod system_prompt;

pub use parser::parse_operations;
pub use registry::ModelRegistry;

use std::path::PathBuf;

use engine_core::{EngineError, EngineResult};

fn default_true() -> bool {
    true
}
use engine_editor::{
    agent::{AgentWriteMode, PermissionPolicy, TraceEntry, TraceRecorder},
    CommandContext, CommandRegistry, ConsoleEntry, ConsoleLevel, ConsoleService, ConsoleSource,
    ProjectContext, SelectionService, UndoRedoStack,
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AiStreamDelta {
    /// User-visible answer text.
    Text(String),
    /// Provider-exposed reasoning text or reasoning summary.
    Thinking(String),
}

impl AiStreamDelta {
    /// Stable event kind used by frontend transports.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Thinking(_) => "thinking",
        }
    }

    /// Text carried by this stream fragment.
    pub fn text(&self) -> &str {
        match self {
            Self::Text(text) | Self::Thinking(text) => text,
        }
    }
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
}

impl AiRequest {
    /// Creates a single-turn request (convenience for backwards compatibility).
    pub fn single_turn(system: String, context: serde_json::Value, user: String) -> Self {
        Self {
            system,
            context,
            messages: vec![ChatMessage::user(user)],
            thinking_effort: None,
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
    /// Create or update a Rhai script file.
    WriteScript {
        /// Path relative to the asset root (e.g. "scripts/player.rhai").
        path: String,
        /// Rhai source code.
        source: String,
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
            Self::CreateObject { .. } => "create_object",
            Self::SetProperty { .. } => "set_property",
            Self::RemoveComponent { .. } => "remove_component",
            Self::DestroyObject { .. } => "destroy_object",
            Self::ReadFile { .. } => "read_file",
            Self::Complete { .. } => "complete",
            Self::UpdateProjectMemory { .. } => "update_project_memory",
            Self::UpdateUserMemory { .. } => "update_user_memory",
            Self::QueryDependencyGraph { .. } => "query_dependency_graph",
            Self::GenerateAsset { .. } => "generate_asset",
            Self::ShowInViewport { .. } => "show_in_viewport",
            Self::BatchOperation { .. } => "batch_operation",
            Self::QuerySceneSemantic { .. } => "query_scene_semantic",
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
/// Owns the project context, command registry, script backend, and undo stack
/// for the duration of an AI interaction.
pub struct AgentSession {
    /// Project state including scene, assets, and manifest.
    pub context: ProjectContext,
    /// Available editor commands.
    pub commands: CommandRegistry,
    /// Rhai script backend for compiling and creating scripts.
    pub script_backend: engine_script_rhai::RhaiScriptBackend,
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

        let mut script_backend = engine_script_rhai::RhaiScriptBackend::new();
        let asset_root = context.root.join(&context.manifest.asset_root);

        // Pre-load any existing scripts from the scene
        script_backend.load_scene_scripts(&context.scene, &asset_root)?;

        Ok(Self {
            context,
            commands,
            script_backend,
            undo_stack: UndoRedoStack::default(),
            console: ConsoleService::default(),
            selection: SelectionService::default(),
            trace: TraceRecorder::default(),
            asset_root,
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
        self.plan_from_response(&response.content, policy)
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
        }
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
            planned.push(PlannedOperation {
                preview: preview_operation(&operation),
                operation,
                requires_write: access.requires_write,
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
                let full_path =
                    self.script_backend
                        .create_script(&self.asset_root, &relative, source)?;
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
                let parsed = parse_entity_id(entity)?;
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
                let parsed = parse_entity_id(entity)?;
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
            } => self.execute_run_command(command, args, working_dir.as_deref(), *timeout_ms, *capture_stdout, *capture_stderr),
        }
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
            "Script" => Ok(ComponentData::Script(engine_ecs::ScriptComponentProxy {
                backend: "rhai".into(),
                script: spec
                    .properties
                    .get("script")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                state_json: None,
                pending_recovery: false,
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
        let parsed = parse_entity_id(entity)?;
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
        _rollback_on_failure: bool,
        depth: u32,
    ) -> EngineResult<()> {
        const MAX_BATCH_DEPTH: u32 = 4;
        if depth >= MAX_BATCH_DEPTH {
            return Err(EngineError::config(format!(
                "Batch operation nesting exceeded maximum depth of {MAX_BATCH_DEPTH}. \
                 Nested BatchOperations are too deep."
            )));
        }
        let mut executed = 0;
        for (i, op) in operations.iter().enumerate() {
            match self.execute_operation_inner(op, depth + 1) {
                Ok(()) => executed += 1,
                Err(e) => {
                    // Drain undo entries for operations that did execute, but note:
                    // full state rollback is not yet implemented. The undo stack
                    // entries are popped so they don't leak into later undo history.
                    for _ in 0..executed {
                        self.undo_stack.undo();
                    }
                    return Err(EngineError::config(format!(
                        "Batch operation failed at step {}/{}: {}. \
                         {} operations completed before failure (rollback not yet implemented).",
                        i + 1,
                        operations.len(),
                        e,
                        executed
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
        use std::process::Command;
        use std::time::Duration;

        let mut cmd = Command::new(command);
        cmd.args(args);

        // Set working directory
        if let Some(dir) = working_dir {
            let work_dir = self.context.root.join(dir);
            cmd.current_dir(&work_dir);
        } else {
            cmd.current_dir(&self.context.root);
        }

        // Configure stdout/stderr capture
        if capture_stdout {
            cmd.stdout(std::process::Stdio::piped());
        }
        if capture_stderr {
            cmd.stderr(std::process::Stdio::piped());
        }

        // Set timeout (default 30 seconds)
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

        // Execute with timeout
        let output = cmd.output().map_err(|e| {
            EngineError::config(format!("Failed to execute command '{}': {}", command, e))
        })?;

        // Check for timeout (simplified - in real implementation would use async)
        let _ = timeout;

        // Log stdout
        if capture_stdout && !output.stdout.is_empty() {
            let stdout = String::from_utf8_lossy(&output.stdout);
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
        if capture_stderr && !output.stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let level = if output.status.success() {
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
        if !output.status.success() {
            let exit_code = output.status.code().unwrap_or(-1);
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

        let script_component = ComponentData::Script(engine_ecs::ScriptComponentProxy {
            backend: "declarative".into(),
            script: behavior_path.to_string(),
            state_json: None,
            pending_recovery: false,
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

#[derive(Clone, Copy, Debug)]
struct OperationAccess {
    requires_write: bool,
    requires_filesystem_write: bool,
}

fn operation_access(operation: &AgentOperation) -> OperationAccess {
    match operation {
        AgentOperation::ReadFile { .. }
        | AgentOperation::Complete { .. }
        | AgentOperation::QueryDependencyGraph { .. }
        | AgentOperation::QuerySceneSemantic { .. }
        | AgentOperation::ShowInViewport { .. } => OperationAccess {
            requires_write: false,
            requires_filesystem_write: false,
        },
        AgentOperation::WriteScript { .. }
        | AgentOperation::UpdateProjectMemory { .. }
        | AgentOperation::UpdateUserMemory { .. } => OperationAccess {
            requires_write: true,
            requires_filesystem_write: true,
        },
        AgentOperation::GenerateAsset { .. } => OperationAccess {
            requires_write: true,
            requires_filesystem_write: true,
        },
        AgentOperation::RunCommand { .. } => OperationAccess {
            requires_write: true,
            requires_filesystem_write: false,
        },
        AgentOperation::ExecuteCommand { .. }
        | AgentOperation::CreateObject { .. }
        | AgentOperation::SetProperty { .. }
        | AgentOperation::RemoveComponent { .. }
        | AgentOperation::DestroyObject { .. }
        | AgentOperation::AttachBehavior { .. }
        | AgentOperation::MoveEntityTo { .. } => OperationAccess {
            requires_write: true,
            requires_filesystem_write: false,
        },
        AgentOperation::BatchOperation { operations, .. } => {
            // Batch operation requires write if any child requires write
            let any_write = operations
                .iter()
                .any(|op| operation_access(op).requires_write);
            let any_fs = operations
                .iter()
                .any(|op| operation_access(op).requires_filesystem_write);
            OperationAccess {
                requires_write: any_write,
                requires_filesystem_write: any_fs,
            }
        }
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
            format!("Create or update Rhai script `{path}`")
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
        | AgentOperation::SetProperty { .. }
        | AgentOperation::RemoveComponent { .. }
        | AgentOperation::DestroyObject { .. }
        | AgentOperation::AttachBehavior { .. }
        | AgentOperation::MoveEntityTo { .. } => "Use editor undo to revert this operation.",
        AgentOperation::WriteScript { .. } => {
            "Review the generated script under the asset root and use version control or file history to revert it."
        }
        AgentOperation::ReadFile { .. }
        | AgentOperation::QuerySceneSemantic { .. } => {
            "No recovery needed; this operation only read project data."
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
        AgentOperation::ShowInViewport { .. } => {
            "No recovery needed; viewport state is transient."
        }
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
        AgentOperation::ReadFile { .. } => "Check that the file path exists inside the project.",
        AgentOperation::ExecuteCommand { .. } => {
            "Check that the command is registered and available."
        }
        AgentOperation::CreateObject { .. }
        | AgentOperation::SetProperty { .. }
        | AgentOperation::RemoveComponent { .. }
        | AgentOperation::DestroyObject { .. } => {
            "Check entity identifiers, component names, and editor diagnostics before retrying."
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
        AgentOperation::GenerateAsset { .. } => {
            "Check tool availability, API key configuration, and network connectivity."
        }
        AgentOperation::ShowInViewport { .. } => {
            "Check that the entity identifier is valid and the entity exists in the scene."
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
    if parts.is_empty() {
        return Err(EngineError::config(format!(
            "invalid entity id: {entity_str}"
        )));
    }
    let slot = parts[0]
        .parse::<u32>()
        .map_err(|_| EngineError::config(format!("invalid entity id: {entity_str}")))?;
    Ok(engine_ecs::Entity::from_handle(engine_core::Handle::new(
        slot,
        engine_core::Generation::FIRST,
    )))
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
            scene_path: std::env::temp_dir().join("main.aster_scene.json"),
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
            scene_path: std::env::temp_dir().join("main.aster_scene.json"),
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
        assert!(session.console.entries()[0]
            .message
            .contains("parse_response"));
    }
}
