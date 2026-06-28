//! Runtime dispatch for local AI tools.
//!
//! The registry is intentionally narrower than tool metadata discovery: it owns
//! execution for concrete operations, while policy validation still happens when
//! plans are built.

use std::collections::BTreeMap;
use std::sync::Arc;

use engine_core::{EngineError, EngineResult};
use engine_editor::{
    ConsoleEntry, ConsoleLevel, ConsoleService, ConsoleSource, ProjectContext, SelectionService,
    agent::PermissionPolicy,
};

use crate::{
    AgentOperation, capability_request_result, component_json, entity_id_string,
    resolve_entity_in_project, resolve_requested_capabilities, sanitize_project_relative_path,
    scene_asset_missing, scene_references_to_asset, skills, tools, transform_json,
};

/// A validated tool invocation ready for local execution.
pub(crate) struct AgentToolInvocation<'a> {
    /// Operation being dispatched.
    pub(crate) operation: &'a AgentOperation,
    /// Active permission policy for preflight-style tools.
    pub(crate) policy: &'a PermissionPolicy,
}

/// Mutable state exposed to tool runtimes.
pub(crate) struct AgentToolSession<'a> {
    /// Active project state.
    pub(crate) project: &'a mut ProjectContext,
    /// Console sink for model-visible tool results.
    pub(crate) console: &'a mut ConsoleService,
    /// Current editor selection.
    #[allow(dead_code)]
    pub(crate) selection: &'a mut SelectionService,
}

/// Runtime implementation for one or more AI operation variants.
pub(crate) trait AgentToolRuntime: Send + Sync {
    /// Stable operation names handled by this runtime.
    fn operation_names(&self) -> &'static [&'static str];

    /// Executes the operation.
    fn execute(
        &self,
        invocation: &AgentToolInvocation<'_>,
        session: &mut AgentToolSession<'_>,
    ) -> EngineResult<()>;
}

/// Registry mapping operation names to runtime implementations.
#[derive(Clone, Default)]
pub(crate) struct AgentToolRuntimeRegistry {
    runtimes: BTreeMap<&'static str, Arc<dyn AgentToolRuntime>>,
}

impl AgentToolRuntimeRegistry {
    /// Builds the default local runtime registry.
    pub(crate) fn with_default_tools() -> Self {
        let mut registry = Self::default();
        registry.register(Arc::new(ReadFileRuntime));
        registry.register(Arc::new(ToolSearchRuntime));
        registry.register(Arc::new(ToolLoadRuntime));
        registry.register(Arc::new(SkillRuntime));
        registry.register(Arc::new(RequestCapabilityRuntime));
        registry.register(Arc::new(ProjectContextRuntime));
        registry
    }

    fn register(&mut self, runtime: Arc<dyn AgentToolRuntime>) {
        for name in runtime.operation_names() {
            self.runtimes.insert(name, Arc::clone(&runtime));
        }
    }

    /// Returns the runtime for an operation name.
    pub(crate) fn get(&self, operation_name: &str) -> Option<Arc<dyn AgentToolRuntime>> {
        self.runtimes.get(operation_name).cloned()
    }
}

struct ReadFileRuntime;

impl AgentToolRuntime for ReadFileRuntime {
    fn operation_names(&self) -> &'static [&'static str] {
        &["read_file"]
    }

    fn execute(
        &self,
        invocation: &AgentToolInvocation<'_>,
        session: &mut AgentToolSession<'_>,
    ) -> EngineResult<()> {
        let AgentOperation::ReadFile { path } = invocation.operation else {
            return Err(unhandled_operation(invocation.operation));
        };

        let relative = sanitize_project_relative_path(path)?;
        let full_path = session.project.root.join(relative);
        let content =
            std::fs::read_to_string(&full_path).map_err(|source| EngineError::Filesystem {
                path: full_path,
                source,
            })?;
        push_console(session.console, "ai-agent", content);
        Ok(())
    }
}

struct ToolSearchRuntime;

impl AgentToolRuntime for ToolSearchRuntime {
    fn operation_names(&self) -> &'static [&'static str] {
        &["tool_search"]
    }

    fn execute(
        &self,
        invocation: &AgentToolInvocation<'_>,
        session: &mut AgentToolSession<'_>,
    ) -> EngineResult<()> {
        let AgentOperation::ToolSearch {
            query,
            types,
            capabilities,
            stage,
            risk_max,
            limit,
        } = invocation.operation
        else {
            return Err(unhandled_operation(invocation.operation));
        };

        let results = tools::search_tools(&tools::ToolSearchQuery {
            query: query.clone(),
            types: types.clone(),
            capabilities: capabilities.clone(),
            stage: stage.clone(),
            risk_max: risk_max.clone(),
            limit: *limit,
        })?;
        push_json_console(session.console, "ai-agent-tools", &results)
    }
}

struct ToolLoadRuntime;

impl AgentToolRuntime for ToolLoadRuntime {
    fn operation_names(&self) -> &'static [&'static str] {
        &["load_tool"]
    }

    fn execute(
        &self,
        invocation: &AgentToolInvocation<'_>,
        session: &mut AgentToolSession<'_>,
    ) -> EngineResult<()> {
        let AgentOperation::LoadTool { name } = invocation.operation else {
            return Err(unhandled_operation(invocation.operation));
        };

        let metadata = tools::metadata_for_tool(name)
            .ok_or_else(|| EngineError::config(format!("unknown tool: {name}")))?;
        if metadata.exposure == tools::ToolExposure::Hidden {
            return Err(EngineError::config(format!(
                "tool is not model-loadable: {name}"
            )));
        }
        let definition = crate::agent_tool_definitions()
            .into_iter()
            .find(|definition| definition.name == name.as_str())
            .ok_or_else(|| EngineError::config(format!("tool definition not found: {name}")))?;
        let result = tools::ToolLoadResult {
            name: name.clone(),
            definition,
            metadata,
        };
        push_json_console(session.console, "ai-agent-tools", &result)
    }
}

struct SkillRuntime;

impl AgentToolRuntime for SkillRuntime {
    fn operation_names(&self) -> &'static [&'static str] {
        &["skill_search", "skill_read"]
    }

    fn execute(
        &self,
        invocation: &AgentToolInvocation<'_>,
        session: &mut AgentToolSession<'_>,
    ) -> EngineResult<()> {
        let registry_config = skills::SkillRegistryConfig::new(
            &session.project.root,
            crate::default_global_varg_root(),
        );

        match invocation.operation {
            AgentOperation::SkillSearch {
                query,
                source,
                limit,
            } => {
                let results = skills::search_skills(
                    &registry_config,
                    &skills::SkillSearchQuery {
                        query: query.clone(),
                        source: source.clone(),
                        limit: *limit,
                    },
                )?;
                push_json_console(session.console, "ai-agent-skills", &results)
            }
            AgentOperation::SkillRead { id, path } => {
                let result = skills::read_skill(
                    &registry_config,
                    &skills::SkillReadRequest {
                        id: id.clone(),
                        path: path.clone(),
                    },
                )?;
                push_console(session.console, "ai-agent-skills", result.content);
                Ok(())
            }
            _ => Err(unhandled_operation(invocation.operation)),
        }
    }
}

struct RequestCapabilityRuntime;

impl AgentToolRuntime for RequestCapabilityRuntime {
    fn operation_names(&self) -> &'static [&'static str] {
        &["request_capability"]
    }

    fn execute(
        &self,
        invocation: &AgentToolInvocation<'_>,
        session: &mut AgentToolSession<'_>,
    ) -> EngineResult<()> {
        let AgentOperation::RequestCapability {
            capabilities,
            tools,
            reason,
        } = invocation.operation
        else {
            return Err(unhandled_operation(invocation.operation));
        };

        let requested = resolve_requested_capabilities(capabilities, tools);
        let result = capability_request_result(&requested, invocation.policy);
        let message = serde_json::to_string_pretty(&serde_json::json!({
            "reason": reason,
            "requested": requested,
            "result": result,
            "note": "request_capability is a preflight only; it does not grant permission."
        }))
        .map_err(|error| EngineError::other(error.to_string()))?;
        push_console(session.console, "ai-agent-permissions", message);
        Ok(())
    }
}

struct ProjectContextRuntime;

impl AgentToolRuntime for ProjectContextRuntime {
    fn operation_names(&self) -> &'static [&'static str] {
        &[
            "get_scene_info",
            "get_object_info",
            "get_asset_info",
            "capture_viewport",
            "validate_scene",
        ]
    }

    fn execute(
        &self,
        invocation: &AgentToolInvocation<'_>,
        session: &mut AgentToolSession<'_>,
    ) -> EngineResult<()> {
        match invocation.operation {
            AgentOperation::GetSceneInfo { include_components } => {
                let info = scene_info_json(session.project, *include_components);
                push_json_console(session.console, "ai-agent-scene", &info)
            }
            AgentOperation::GetObjectInfo { entity } => {
                let parsed = resolve_entity_in_project(session.project, entity)?;
                let info = object_info_json(session.project, parsed)?;
                push_json_console(session.console, "ai-agent-scene", &info)
            }
            AgentOperation::GetAssetInfo { asset } => {
                let info = asset_info_json(session.project, asset);
                push_json_console(session.console, "ai-agent-assets", &info)
            }
            AgentOperation::CaptureViewport {
                entity,
                output_path,
            } => execute_capture_viewport(session, entity.as_deref(), output_path.as_deref()),
            AgentOperation::ValidateScene { include_warnings } => {
                let report = validate_scene_json(session.project, *include_warnings);
                push_json_console(session.console, "ai-agent-validation", &report)
            }
            _ => Err(unhandled_operation(invocation.operation)),
        }
    }
}

fn scene_info_json(context: &ProjectContext, include_components: bool) -> serde_json::Value {
    let objects = context
        .scene
        .objects()
        .into_iter()
        .map(|(entity, object)| {
            let transform = context.scene.transforms().local(entity);
            let parent = context.scene.transforms().parent(entity);
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

    serde_json::json!({
        "scene_path": context.scene_path,
        "mode": format!("{:?}", context.scene.mode()),
        "structure_version": context.scene.structure_version(),
        "object_count": objects.len(),
        "objects": objects,
    })
}

fn object_info_json(
    context: &ProjectContext,
    entity: engine_ecs::Entity,
) -> EngineResult<serde_json::Value> {
    let object = context
        .scene
        .object(entity)
        .ok_or_else(|| EngineError::config("object not found"))?;
    let transform = context.scene.transforms().local(entity);
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
        "parent": context.scene.transforms().parent(entity).map(entity_id_string),
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

fn asset_info_json(context: &ProjectContext, asset: &str) -> serde_json::Value {
    let query = asset.trim();
    let by_path = context
        .assets
        .iter()
        .find(|candidate| candidate.source_path.to_string_lossy() == query);
    let by_guid = context
        .assets
        .iter()
        .find(|candidate| candidate.guid.to_string() == query);
    let meta = by_path.or(by_guid);
    let references = meta
        .map(|meta| scene_references_to_asset(&context.scene, meta.guid.as_asset_id()))
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

fn execute_capture_viewport(
    session: &mut AgentToolSession<'_>,
    entity: Option<&str>,
    output_path: Option<&str>,
) -> EngineResult<()> {
    if let Some(entity) = entity {
        session
            .selection
            .select(engine_editor::Selection::Entity(entity.to_owned()));
    }
    if let Some(path) = output_path {
        let _ = sanitize_project_relative_path(path)?;
    }
    push_json_console(
        session.console,
        "ai-agent-viewport",
        &serde_json::json!({
            "capture_requested": true,
            "entity": entity,
            "output_path": output_path,
            "note": "Viewport capture is queued for the editor host; this operation records the requested evidence."
        }),
    )
}

fn validate_scene_json(context: &ProjectContext, include_warnings: bool) -> serde_json::Value {
    let mut diagnostics = Vec::new();
    for (entity, object) in context.scene.objects() {
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
                    && scene_asset_missing(context, asset)
                {
                    diagnostics.push(serde_json::json!({
                        "level": "error",
                        "entity": entity_id_string(entity),
                        "message": "MeshRenderer references a missing mesh asset",
                        "asset": asset.as_u128().to_string(),
                    }));
                }
                if let Some(asset) = mesh.material.asset
                    && scene_asset_missing(context, asset)
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

fn push_json_console<T: serde::Serialize>(
    console: &mut ConsoleService,
    subsystem: &'static str,
    value: &T,
) -> EngineResult<()> {
    let message = serde_json::to_string_pretty(value)
        .map_err(|error| EngineError::other(error.to_string()))?;
    push_console(console, subsystem, message);
    Ok(())
}

fn push_console(
    console: &mut ConsoleService,
    subsystem: impl Into<String>,
    message: impl Into<String>,
) {
    console.push(ConsoleEntry {
        timestamp: "now".into(),
        level: ConsoleLevel::Info,
        source: ConsoleSource {
            subsystem: subsystem.into(),
            file: None,
            line: None,
        },
        message: message.into(),
    });
}

fn unhandled_operation(operation: &AgentOperation) -> EngineError {
    EngineError::config(format!(
        "tool runtime cannot execute {}",
        operation.action_name()
    ))
}
