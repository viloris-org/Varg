//! AI tool registry metadata, exposure policy, and lightweight discovery.
//!
//! Search is intentionally discovery-only. Returning a tool here does not grant
//! permission to execute it; execution still flows through the deterministic
//! operation and permission checks in the agent session.

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use crate::ToolDefinition;

/// Whether an AI tool is initially model-visible, searchable, or internal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExposure {
    /// Included in the initial model-visible tool list.
    Direct,
    /// Searchable, but omitted from the initial tool list until selected.
    Deferred,
    /// Visible to the primary model, but not to nested or secondary surfaces.
    DirectModelOnly,
    /// Registered for trusted dispatch only.
    Hidden,
}

/// Broad tool category used for discovery and policy planning.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolType {
    /// Context and project state.
    Context,
    /// Scene-level queries and edits.
    Scene,
    /// Entity-level queries and edits.
    Entity,
    /// Component-level queries and edits.
    Component,
    /// Asset metadata and writes.
    Asset,
    /// Mesh/modeling operations.
    Mesh,
    /// Material operations.
    Material,
    /// Viewport feedback.
    Viewport,
    /// Script authoring or validation.
    Script,
    /// Validation and acceptance checks.
    Validation,
    /// Project filesystem access.
    Filesystem,
    /// External commands.
    Command,
    /// Memory tools.
    Memory,
    /// Quest delegation.
    Quest,
    /// Skill discovery or reading.
    Skill,
}

impl ToolType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Scene => "scene",
            Self::Entity => "entity",
            Self::Component => "component",
            Self::Asset => "asset",
            Self::Mesh => "mesh",
            Self::Material => "material",
            Self::Viewport => "viewport",
            Self::Script => "script",
            Self::Validation => "validation",
            Self::Filesystem => "filesystem",
            Self::Command => "command",
            Self::Memory => "memory",
            Self::Quest => "quest",
            Self::Skill => "skill",
        }
    }
}

/// Stage in an AI tool chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStage {
    /// Inspect state before editing.
    Inspect,
    /// Author new content.
    Author,
    /// Refine existing content.
    Refine,
    /// Verify the result.
    Verify,
    /// Repair errors.
    Repair,
    /// Review proposed changes.
    Review,
    /// Apply changes.
    Apply,
}

impl ToolStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Author => "author",
            Self::Refine => "refine",
            Self::Verify => "verify",
            Self::Repair => "repair",
            Self::Review => "review",
            Self::Apply => "apply",
        }
    }
}

/// Risk class used by discovery and policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    /// Read-only or transient editor UI updates.
    Low,
    /// Local project mutations with bounded scope.
    Medium,
    /// Broad writes, external execution, or hard-to-revert operations.
    High,
    /// Disabled by default.
    Critical,
}

impl RiskClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// Evidence expected from an AI-authored operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// Operation preview.
    OperationPreview,
    /// Scene diff.
    SceneDiff,
    /// Asset diff.
    AssetDiff,
    /// Asset reference check.
    AssetReferenceCheck,
    /// Viewport preview.
    ViewportPreview,
    /// Validation log.
    ValidationLog,
    /// Rollback plan.
    RollbackPlan,
}

impl EvidenceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::OperationPreview => "operation_preview",
            Self::SceneDiff => "scene_diff",
            Self::AssetDiff => "asset_diff",
            Self::AssetReferenceCheck => "asset_reference_check",
            Self::ViewportPreview => "viewport_preview",
            Self::ValidationLog => "validation_log",
            Self::RollbackPlan => "rollback_plan",
        }
    }
}

/// Planning and policy metadata for a Varg AI tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VargToolMetadata {
    /// Tool name.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Exposure policy.
    pub exposure: ToolExposure,
    /// Broad type for filtering.
    pub tool_type: ToolType,
    /// Workflow stages where the tool is useful.
    pub stage: Vec<ToolStage>,
    /// Required capability strings.
    pub capabilities: Vec<String>,
    /// Risk class.
    pub risk: RiskClass,
    /// Required or expected evidence.
    pub evidence: Vec<EvidenceKind>,
    /// Related skill/reference paths.
    pub skill_refs: Vec<String>,
    /// Extra searchable terms.
    pub keywords: Vec<String>,
}

/// Tool search request.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToolSearchQuery {
    /// Natural-language query text.
    pub query: String,
    /// Optional type filters.
    #[serde(default)]
    pub types: Vec<String>,
    /// Optional capability filters. A result must contain every requested value.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Optional stage filter.
    #[serde(default)]
    pub stage: Option<String>,
    /// Optional maximum risk class.
    #[serde(default)]
    pub risk_max: Option<String>,
    /// Maximum result count.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Search result returned to the model/editor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSearchResult {
    /// Tool name.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Tool type.
    pub tool_type: ToolType,
    /// Matching stages.
    pub stage: Vec<ToolStage>,
    /// Risk class.
    pub risk: RiskClass,
    /// Required capabilities.
    pub capabilities: Vec<String>,
    /// Related skills.
    pub skill_refs: Vec<String>,
    /// Whether the full schema is currently model-visible.
    pub schema_loaded: bool,
    /// Lightweight relevance score.
    pub score: u32,
}

/// Capability preflight request.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapabilityRequest {
    /// Capabilities the model expects to need.
    pub capabilities: Vec<String>,
    /// Optional tool names that imply capabilities through registry metadata.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Optional human-readable reason shown in logs or approval UI.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Deterministic policy decision for a requested capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDecision {
    /// The current policy allows this capability.
    Approved,
    /// A narrower policy or target scope is required before approval.
    Narrowed,
    /// The current policy denies this capability.
    Denied,
    /// User approval may allow this capability.
    RequiresUserApproval,
    /// This capability should be routed to Quest.
    RequiresQuest,
}

/// One capability decision.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityDecisionResult {
    /// Capability string.
    pub capability: String,
    /// Decision for the current policy.
    pub decision: CapabilityDecision,
    /// Short explanation suitable for logs.
    pub reason: String,
}

/// Result returned by request_capability.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityRequestResult {
    /// Decisions for each requested capability.
    pub decisions: Vec<CapabilityDecisionResult>,
    /// Whether every requested capability is approved.
    pub all_approved: bool,
}

/// Returns the direct discovery tool definition.
pub fn tool_search_definition() -> ToolDefinition {
    ToolDefinition {
        name: "tool_search".into(),
        description: "Search Varg's deferred AI tools by query, type, capability, stage, and risk. Discovery does not grant permission.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "query": { "type": "string", "description": "Natural-language search text" },
                "types": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional tool type filters such as scene, mesh, material, viewport, script, validation"
                },
                "capabilities": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Required capability filters such as scene.write.entity or asset.write.mesh"
                },
                "stage": { "type": "string", "description": "Optional workflow stage such as inspect, author, refine, verify, repair, review, apply" },
                "risk_max": { "type": "string", "description": "Optional maximum risk: low, medium, high, critical" },
                "limit": { "type": "integer", "description": "Maximum number of results" }
            },
            "required": ["query"]
        }),
    }
}

/// Returns the direct capability preflight tool definition.
pub fn request_capability_definition() -> ToolDefinition {
    ToolDefinition {
        name: "request_capability".into(),
        description: "Preflight whether the current policy allows requested Varg capabilities. This explains approval state but does not grant permission.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "capabilities": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Capability strings such as scene.write.entity, asset.write.mesh, viewport.capture, or command.run"
                },
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional tool names whose declared capabilities should be included"
                },
                "reason": { "type": "string", "description": "Why these capabilities are needed" }
            },
            "required": ["capabilities"]
        }),
    }
}

/// Returns metadata for every registered model-visible or trusted tool.
pub fn tool_registry() -> Vec<VargToolMetadata> {
    vec![
        meta(
            "tool_search",
            "Search deferred Varg AI tools without granting execution permission.",
            ToolExposure::Direct,
            ToolType::Skill,
            &[ToolStage::Inspect],
            &["tool.search"],
            RiskClass::Low,
            &[],
            &[],
            &["discovery", "registry", "find tools", "search tools"],
        ),
        meta(
            "skill_search",
            "Search Varg project skills in .varg/skills and user-global skills in ~/.varg/skills.",
            ToolExposure::Direct,
            ToolType::Skill,
            &[ToolStage::Inspect],
            &["skill.search"],
            RiskClass::Low,
            &[],
            &["varg-permissions"],
            &[
                "skills",
                "instructions",
                "references",
                "project skills",
                "global skills",
            ],
        ),
        meta(
            "skill_read",
            "Read a selected Varg skill file after skill_search resolves its source.",
            ToolExposure::Direct,
            ToolType::Skill,
            &[ToolStage::Inspect],
            &["skill.read"],
            RiskClass::Low,
            &[],
            &["varg-permissions"],
            &[
                "skills",
                "instructions",
                "references",
                "read skill",
                "skill md",
            ],
        ),
        meta(
            "request_capability",
            "Ask the policy gate for a scoped capability preflight without granting access.",
            ToolExposure::Direct,
            ToolType::Context,
            &[ToolStage::Review],
            &["context.read"],
            RiskClass::Low,
            &[],
            &["varg-permissions"],
            &[
                "approval",
                "permission",
                "capability",
                "policy",
                "preflight",
            ],
        ),
        meta(
            "create_object",
            "Create a new game object with optional components and position.",
            ToolExposure::DirectModelOnly,
            ToolType::Scene,
            &[ToolStage::Author, ToolStage::Apply],
            &["scene.write.entity", "scene.write.component"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-scene-authoring"],
            &["entity", "gameobject", "primitive", "spawn", "place"],
        ),
        meta(
            "write_script",
            "Create or update a Varg script file in the project.",
            ToolExposure::DirectModelOnly,
            ToolType::Script,
            &[ToolStage::Author, ToolStage::Repair],
            &["asset.write.generated"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::ValidationLog],
            &["varg-behavior-scripting"],
            &["behavior", "gameplay", "script", "varg"],
        ),
        meta(
            "check_script",
            "Run strict final acceptance validation for one or more .varg files.",
            ToolExposure::Direct,
            ToolType::Validation,
            &[ToolStage::Verify, ToolStage::Repair],
            &["asset.read"],
            RiskClass::Low,
            &[EvidenceKind::ValidationLog],
            &["varg-behavior-scripting"],
            &["diagnostics", "language service", "acceptance", "validate"],
        ),
        meta(
            "write_file",
            "Create or update a UTF-8 text file relative to the project root.",
            ToolExposure::DirectModelOnly,
            ToolType::Filesystem,
            &[ToolStage::Author, ToolStage::Apply],
            &["asset.write.generated"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::AssetDiff],
            &["varg-asset-pipeline"],
            &["docs", "config", "schema", "project file"],
        ),
        meta(
            "generate_asset",
            "Request generation of an external asset into a project asset path.",
            ToolExposure::Deferred,
            ToolType::Asset,
            &[ToolStage::Author, ToolStage::Apply],
            &["asset.write.generated"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::AssetDiff],
            &["varg-asset-pipeline"],
            &["image", "audio", "model", "texture", "generated asset"],
        ),
        meta(
            "set_property",
            "Modify a component field on an entity.",
            ToolExposure::DirectModelOnly,
            ToolType::Component,
            &[ToolStage::Refine, ToolStage::Apply],
            &["scene.write.component"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-scene-authoring"],
            &["component", "field", "property", "tune"],
        ),
        meta(
            "remove_component",
            "Remove a component from an entity.",
            ToolExposure::DirectModelOnly,
            ToolType::Component,
            &[ToolStage::Refine, ToolStage::Apply],
            &["scene.write.component"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-scene-authoring"],
            &["component", "delete", "remove"],
        ),
        meta(
            "destroy_object",
            "Delete an entity from the scene.",
            ToolExposure::DirectModelOnly,
            ToolType::Entity,
            &[ToolStage::Refine, ToolStage::Apply],
            &["scene.write.entity"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-scene-authoring"],
            &["delete", "remove", "entity", "gameobject"],
        ),
        meta(
            "read_file",
            "Read a source file from the project.",
            ToolExposure::Direct,
            ToolType::Filesystem,
            &[ToolStage::Inspect],
            &["asset.read"],
            RiskClass::Low,
            &[],
            &["varg-asset-pipeline"],
            &["inspect", "open", "source"],
        ),
        meta(
            "create_task",
            "Create a short-lived Copilot task for the editor task card.",
            ToolExposure::Direct,
            ToolType::Context,
            &[ToolStage::Review],
            &["context.read"],
            RiskClass::Low,
            &[],
            &[],
            &["todo", "task", "progress"],
        ),
        meta(
            "update_task",
            "Update a Copilot task title or completion state in the editor task card.",
            ToolExposure::Direct,
            ToolType::Context,
            &[ToolStage::Review],
            &["context.read"],
            RiskClass::Low,
            &[],
            &[],
            &["todo", "task", "progress", "done"],
        ),
        meta(
            "execute_command",
            "Execute a registered editor command.",
            ToolExposure::DirectModelOnly,
            ToolType::Command,
            &[ToolStage::Apply],
            &["scene.write.entity", "scene.write.component"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview],
            &["varg-permissions"],
            &["editor command", "menu command"],
        ),
        meta(
            "query_scene_semantic",
            "Search for entities in the scene using natural language.",
            ToolExposure::Direct,
            ToolType::Scene,
            &[ToolStage::Inspect],
            &["scene.read"],
            RiskClass::Low,
            &[],
            &["varg-scene-authoring"],
            &["find entity", "search scene", "selection", "object info"],
        ),
        meta(
            "get_scene_info",
            "Return scene hierarchy, entities, transforms, components, cameras, lights, and selected object summaries.",
            ToolExposure::Direct,
            ToolType::Scene,
            &[ToolStage::Inspect],
            &["scene.read"],
            RiskClass::Low,
            &[],
            &["varg-scene-authoring", "varg-modeling"],
            &[
                "hierarchy",
                "scene info",
                "objects",
                "components",
                "inspect",
            ],
        ),
        meta(
            "get_object_info",
            "Return detailed component, transform, mesh, material, and bounds information for one object.",
            ToolExposure::Direct,
            ToolType::Entity,
            &[ToolStage::Inspect],
            &["scene.read"],
            RiskClass::Low,
            &[],
            &["varg-scene-authoring", "varg-modeling"],
            &[
                "object info",
                "entity details",
                "bounds",
                "mesh",
                "material",
            ],
        ),
        meta(
            "get_asset_info",
            "Return metadata and scene references for an asset path or GUID.",
            ToolExposure::Direct,
            ToolType::Asset,
            &[ToolStage::Inspect],
            &["asset.read"],
            RiskClass::Low,
            &[],
            &["varg-asset-pipeline"],
            &["asset info", "references", "guid", "metadata"],
        ),
        meta(
            "create_primitive",
            "Create a primitive mesh object with transform and material fields.",
            ToolExposure::Deferred,
            ToolType::Mesh,
            &[ToolStage::Author, ToolStage::Apply],
            &["scene.write.entity", "scene.write.component"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-modeling", "varg-materials"],
            &["cube", "sphere", "plane", "primitive", "modeling", "mesh"],
        ),
        meta(
            "create_mesh_asset",
            "Generate a structured .vmodel TOML authoring file from primitives or mesh operations.",
            ToolExposure::Deferred,
            ToolType::Mesh,
            &[ToolStage::Author, ToolStage::Apply],
            &["asset.write.mesh"],
            RiskClass::Medium,
            &[
                EvidenceKind::OperationPreview,
                EvidenceKind::AssetDiff,
                EvidenceKind::AssetReferenceCheck,
            ],
            &["varg-modeling", "varg-asset-pipeline"],
            &[
                "mesh asset",
                "vmodel",
                "bevel",
                "inset",
                "extrude",
                "structured mesh",
            ],
        ),
        meta(
            "modify_mesh",
            "Apply structured mesh operations by recording a derived .vmodel TOML authoring file.",
            ToolExposure::Deferred,
            ToolType::Mesh,
            &[ToolStage::Refine, ToolStage::Apply],
            &["asset.write.mesh"],
            RiskClass::Medium,
            &[
                EvidenceKind::OperationPreview,
                EvidenceKind::AssetDiff,
                EvidenceKind::AssetReferenceCheck,
            ],
            &["varg-modeling"],
            &[
                "bevel",
                "inset",
                "extrude",
                "mirror",
                "boolean",
                "array",
                "mesh operations",
            ],
        ),
        meta(
            "set_material",
            "Create or assign material parameters for an object's MeshRenderer.",
            ToolExposure::Deferred,
            ToolType::Material,
            &[ToolStage::Refine, ToolStage::Apply],
            &["scene.write.component", "asset.write.material"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-materials", "varg-modeling"],
            &[
                "pbr",
                "base color",
                "roughness",
                "metallic",
                "assign material",
            ],
        ),
        meta(
            "set_transform",
            "Set an object's transform using structured position, rotation, and scale values.",
            ToolExposure::Deferred,
            ToolType::Component,
            &[ToolStage::Refine, ToolStage::Apply],
            &["scene.write.component"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-scene-authoring", "varg-modeling"],
            &["transform", "position", "rotation", "scale"],
        ),
        meta(
            "duplicate_object",
            "Duplicate an entity with optional repeated transform offsets.",
            ToolExposure::Deferred,
            ToolType::Scene,
            &[ToolStage::Author, ToolStage::Apply],
            &["scene.write.entity", "scene.write.component"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-scene-authoring", "varg-modeling"],
            &["duplicate", "copy", "array placement", "repeat"],
        ),
        meta(
            "capture_viewport",
            "Request a viewport preview for visual feedback evidence.",
            ToolExposure::Direct,
            ToolType::Viewport,
            &[ToolStage::Inspect, ToolStage::Verify],
            &["viewport.capture"],
            RiskClass::Low,
            &[EvidenceKind::ViewportPreview],
            &["varg-modeling", "varg-scene-authoring"],
            &["screenshot", "preview", "visual feedback", "capture"],
        ),
        meta(
            "validate_scene",
            "Check references, missing assets, schema validity, and basic scene constraints.",
            ToolExposure::Direct,
            ToolType::Validation,
            &[ToolStage::Verify, ToolStage::Repair],
            &["scene.read", "asset.read"],
            RiskClass::Low,
            &[
                EvidenceKind::ValidationLog,
                EvidenceKind::AssetReferenceCheck,
            ],
            &["varg-scene-authoring", "varg-asset-pipeline"],
            &[
                "validate",
                "missing assets",
                "references",
                "scene constraints",
            ],
        ),
        meta(
            "move_entity_to",
            "Move an entity to a target position, optionally animated.",
            ToolExposure::DirectModelOnly,
            ToolType::Entity,
            &[ToolStage::Refine, ToolStage::Apply],
            &["scene.write.entity", "scene.write.component"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-scene-authoring"],
            &["transform", "position", "translate", "move"],
        ),
        meta(
            "show_in_viewport",
            "Highlight or focus an entity in the editor viewport.",
            ToolExposure::Direct,
            ToolType::Viewport,
            &[ToolStage::Inspect, ToolStage::Verify],
            &["viewport.capture"],
            RiskClass::Low,
            &[EvidenceKind::ViewportPreview],
            &["varg-scene-authoring"],
            &["frame", "highlight", "preview", "camera"],
        ),
        meta(
            "attach_behavior",
            "Attach a declarative behavior tree to an entity.",
            ToolExposure::Deferred,
            ToolType::Script,
            &[ToolStage::Author, ToolStage::Apply],
            &["scene.write.component", "asset.read"],
            RiskClass::Medium,
            &[EvidenceKind::OperationPreview, EvidenceKind::SceneDiff],
            &["varg-behavior-scripting"],
            &["behavior tree", "attach", "script component"],
        ),
        meta(
            "run_command",
            "Execute a shell command or external process.",
            ToolExposure::DirectModelOnly,
            ToolType::Command,
            &[ToolStage::Verify, ToolStage::Repair, ToolStage::Apply],
            &["command.run"],
            RiskClass::High,
            &[EvidenceKind::ValidationLog, EvidenceKind::RollbackPlan],
            &["varg-permissions"],
            &["shell", "process", "cargo", "external command"],
        ),
        meta(
            "update_project_memory",
            "Update the project memory file.",
            ToolExposure::Deferred,
            ToolType::Memory,
            &[ToolStage::Review, ToolStage::Apply],
            &["asset.write.generated"],
            RiskClass::Medium,
            &[EvidenceKind::AssetDiff],
            &[],
            &["memory", "project notes", "context"],
        ),
        meta(
            "update_user_memory",
            "Record an observed user pattern or preference.",
            ToolExposure::Deferred,
            ToolType::Memory,
            &[ToolStage::Review, ToolStage::Apply],
            &["asset.write.generated"],
            RiskClass::Medium,
            &[EvidenceKind::AssetDiff],
            &[],
            &["memory", "preference", "style"],
        ),
        meta(
            "query_dependency_graph",
            "Query the project dependency graph.",
            ToolExposure::Deferred,
            ToolType::Context,
            &[ToolStage::Inspect],
            &["context.read", "scene.read", "asset.read"],
            RiskClass::Low,
            &[],
            &["varg-scene-authoring"],
            &["dependencies", "references", "graph"],
        ),
        meta(
            "complete",
            "Signal that the task is complete with an optional summary.",
            ToolExposure::Direct,
            ToolType::Context,
            &[ToolStage::Review],
            &["context.read"],
            RiskClass::Low,
            &[],
            &[],
            &["finish", "done", "summary"],
        ),
    ]
}

/// Returns metadata for one tool name.
pub fn metadata_for_tool(name: &str) -> Option<VargToolMetadata> {
    tool_registry()
        .into_iter()
        .find(|metadata| metadata.name == name)
}

/// Searches registered tools with typed filters and lightweight ranking.
pub fn search_tools(query: &ToolSearchQuery) -> EngineResult<Vec<ToolSearchResult>> {
    let risk_max = match &query.risk_max {
        Some(value) => Some(parse_risk(value)?),
        None => None,
    };
    let stage_filter = query.stage.as_deref().map(normalize_filter);
    let type_filters = query
        .types
        .iter()
        .map(|value| normalize_filter(value))
        .collect::<Vec<_>>();
    let capability_filters = query
        .capabilities
        .iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let terms = tokenize(&query.query);
    let limit = query.limit.unwrap_or(8).clamp(1, 32);

    let mut scored = Vec::new();
    for metadata in tool_registry() {
        if metadata.exposure == ToolExposure::Hidden {
            continue;
        }
        if !type_filters.is_empty()
            && !type_filters
                .iter()
                .any(|filter| filter == metadata.tool_type.as_str())
        {
            continue;
        }
        if let Some(max) = risk_max
            && metadata.risk > max
        {
            continue;
        }
        if let Some(stage) = &stage_filter
            && !metadata
                .stage
                .iter()
                .any(|candidate| candidate.as_str() == stage)
        {
            continue;
        }
        if !capability_filters.iter().all(|required| {
            metadata
                .capabilities
                .iter()
                .any(|capability| capability == required)
        }) {
            continue;
        }

        let search_text = indexed_search_text(&metadata);
        let mut score = score_terms(&search_text, &terms);
        if let Some(stage) = &stage_filter
            && metadata
                .stage
                .iter()
                .any(|candidate| candidate.as_str() == stage)
        {
            score += 4;
        }
        if terms.is_empty() {
            score += 1;
        }
        if score == 0 {
            continue;
        }
        scored.push((
            score,
            ToolSearchResult {
                name: metadata.name,
                description: metadata.description,
                tool_type: metadata.tool_type,
                stage: metadata.stage,
                risk: metadata.risk,
                capabilities: metadata.capabilities,
                skill_refs: metadata.skill_refs,
                schema_loaded: matches!(
                    metadata.exposure,
                    ToolExposure::Direct | ToolExposure::DirectModelOnly
                ),
                score,
            },
        ));
    }

    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.risk.cmp(&right.risk))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(scored
        .into_iter()
        .take(limit)
        .map(|(_, result)| result)
        .collect())
}

fn meta(
    name: &str,
    description: &str,
    exposure: ToolExposure,
    tool_type: ToolType,
    stage: &[ToolStage],
    capabilities: &[&str],
    risk: RiskClass,
    evidence: &[EvidenceKind],
    skill_refs: &[&str],
    keywords: &[&str],
) -> VargToolMetadata {
    VargToolMetadata {
        name: name.to_owned(),
        description: description.to_owned(),
        exposure,
        tool_type,
        stage: stage.to_vec(),
        capabilities: capabilities
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        risk,
        evidence: evidence.to_vec(),
        skill_refs: skill_refs.iter().map(|value| (*value).to_owned()).collect(),
        keywords: keywords.iter().map(|value| (*value).to_owned()).collect(),
    }
}

fn parse_risk(value: &str) -> EngineResult<RiskClass> {
    match normalize_filter(value).as_str() {
        "low" => Ok(RiskClass::Low),
        "medium" => Ok(RiskClass::Medium),
        "high" => Ok(RiskClass::High),
        "critical" => Ok(RiskClass::Critical),
        other => Err(EngineError::config(format!("unknown risk class: {other}"))),
    }
}

fn indexed_search_text(metadata: &VargToolMetadata) -> String {
    let expanded_name = metadata.name.replace('_', " ");
    let stages = metadata
        .stage
        .iter()
        .map(|stage| stage.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let evidence = metadata
        .evidence
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{} {} {} {} {} {} {} {} {}",
        metadata.name,
        expanded_name,
        metadata.description,
        metadata.tool_type.as_str(),
        stages,
        metadata.risk.as_str(),
        evidence,
        metadata.capabilities.join(" "),
        metadata.keywords.join(" ")
    )
    .to_lowercase()
}

fn score_terms(search_text: &str, terms: &[String]) -> u32 {
    terms
        .iter()
        .map(|term| {
            if search_text.contains(term) {
                if search_text
                    .split_whitespace()
                    .any(|candidate| candidate == term)
                {
                    3
                } else {
                    1
                }
            } else {
                0
            }
        })
        .sum()
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.' && ch != '_')
        .map(normalize_filter)
        .filter(|term| !term.is_empty())
        .collect()
}

fn normalize_filter(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_search_filters_by_type_capability_risk_and_stage() {
        let results = search_tools(&ToolSearchQuery {
            query: "create scene object".into(),
            types: vec!["scene".into()],
            capabilities: vec!["scene.write.entity".into()],
            stage: Some("author".into()),
            risk_max: Some("medium".into()),
            limit: Some(8),
        })
        .unwrap();

        assert!(results.iter().any(|result| result.name == "create_object"));
        assert!(
            results
                .iter()
                .all(|result| result.tool_type == ToolType::Scene)
        );
        assert!(
            results
                .iter()
                .all(|result| result.risk <= RiskClass::Medium)
        );
    }

    #[test]
    fn high_risk_tools_are_hidden_by_medium_risk_filter() {
        let results = search_tools(&ToolSearchQuery {
            query: "run shell command".into(),
            risk_max: Some("medium".into()),
            ..ToolSearchQuery::default()
        })
        .unwrap();

        assert!(!results.iter().any(|result| result.name == "run_command"));
    }

    #[test]
    fn search_results_mark_direct_schema_loaded() {
        let results = search_tools(&ToolSearchQuery {
            query: "validate scripts".into(),
            ..ToolSearchQuery::default()
        })
        .unwrap();
        let check_script = results
            .iter()
            .find(|result| result.name == "check_script")
            .expect("check_script should match validation query");

        assert!(check_script.schema_loaded);
    }

    #[test]
    fn deferred_asset_generation_is_discoverable_without_loaded_schema() {
        let results = search_tools(&ToolSearchQuery {
            query: "generate texture asset".into(),
            types: vec!["asset".into()],
            capabilities: vec!["asset.write.generated".into()],
            risk_max: Some("medium".into()),
            ..ToolSearchQuery::default()
        })
        .unwrap();
        let generate_asset = results
            .iter()
            .find(|result| result.name == "generate_asset")
            .expect("generate_asset should be discoverable");

        assert!(!generate_asset.schema_loaded);
    }

    #[test]
    fn structured_modeling_tools_are_deferred_and_discoverable() {
        let results = search_tools(&ToolSearchQuery {
            query: "create sci fi door bevel inset mesh".into(),
            types: vec!["mesh".into()],
            capabilities: vec!["asset.write.mesh".into()],
            stage: Some("author".into()),
            risk_max: Some("medium".into()),
            ..ToolSearchQuery::default()
        })
        .unwrap();
        let create_mesh = results
            .iter()
            .find(|result| result.name == "create_mesh_asset")
            .expect("create_mesh_asset should be discoverable");

        assert_eq!(create_mesh.tool_type, ToolType::Mesh);
        assert!(!create_mesh.schema_loaded);
    }
}
