//! Copilot engine — bridges the Copilot panel UI with AgentSession and AiModel.
//!
//! Processing is synchronous for the MVP. Future iterations will move to
//! background-task execution for non-blocking model calls.

use engine_ai::{
    AgentOperation, AgentPlan, AgentSession, AiModel, AiRequest, AiResponse, PlannedOperation,
};
use engine_core::{EngineError, EngineResult};
use engine_editor::agent::PermissionPolicy;

use super::types::{CopilotStatus, PlanPreviewItem, ShellUiState};

/// A stub model that returns a canned response for testing.
/// Replace with a real provider adapter in Step 4.
pub struct StubModel {
    response: String,
}

impl StubModel {
    /// Creates a stub model that always returns the given JSON response.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl AiModel for StubModel {
    fn chat(&self, _request: AiRequest) -> EngineResult<AiResponse> {
        Ok(AiResponse {
            content: self.response.clone(),
        })
    }
}

/// Clones a Scene via JSON round-trip (Scene doesn't derive Clone).
fn clone_scene(scene: &engine_ecs::Scene) -> EngineResult<engine_ecs::Scene> {
    let json = scene.to_json("clone")?;
    engine_ecs::Scene::from_json(&json)
        .map_err(|e| EngineError::other(format!("scene clone failed: {e}")))
}

/// Runs a full Copilot mode cycle: plan → display.
///
/// Returns parsed operations on success for the UI to display.
pub fn process_copilot_prompt(
    shell: &mut crate::EditorShell,
    ui_state: &mut ShellUiState,
    prompt: &str,
) -> EngineResult<Vec<PlannedOperation>> {
    let project_ui = shell
        .project_mut()
        .ok_or_else(|| EngineError::config("no project is open"))?;

    // Clone scene via JSON round-trip (Scene doesn't derive Clone)
    let scene_clone = clone_scene(&project_ui.scene)?;

    let project_ctx = engine_editor::ProjectContext {
        manifest: project_ui.manifest.clone(),
        scene: scene_clone,
        asset_db: project_ui.database.clone(),
        project_root: project_ui.root.clone(),
    };

    let mut session = AgentSession::new(project_ctx)?;

    let policy = if ui_state.copilot.auto_accept {
        PermissionPolicy {
            write_mode: engine_editor::agent::AgentWriteMode::Transactional,
            filesystem_write: true,
            process_execution: false,
            network: false,
            direct_write: false,
        }
    } else {
        PermissionPolicy::transactional_write()
    };

    // Build the plan
    let stub = StubModel::new(generate_agent_json(prompt));
    let plan = session.plan(&stub, prompt, policy)?;

    // Store plan preview in UI state
    ui_state.copilot.plan_preview = plan
        .operations
        .iter()
        .map(|planned| PlanPreviewItem {
            index: planned.operation.action_name().len(),
            preview: planned.preview.clone(),
            requires_write: planned.requires_write,
            approved: !planned.requires_write,
        })
        .collect();

    ui_state.copilot.status = if ui_state.copilot.plan_preview.is_empty() {
        CopilotStatus::Error("No operations to preview".to_owned())
    } else {
        CopilotStatus::ReadyForReview
    };

    Ok(plan.operations)
}

/// Applies the currently approved operations from the plan preview.
pub fn apply_approved_operations(
    shell: &mut crate::EditorShell,
    ui_state: &mut ShellUiState,
) -> EngineResult<String> {
    let approved_ops: Vec<AgentOperation> = ui_state
        .copilot
        .plan_preview
        .iter()
        .filter(|p| p.approved)
        .map(|p| AgentOperation::Complete {
            summary: Some(p.preview.clone()),
        })
        .collect();

    if approved_ops.is_empty() {
        return Err(EngineError::config("no approved operations to apply"));
    }

    let project_ui = shell
        .project_mut()
        .ok_or_else(|| EngineError::config("no project is open"))?;

    // Clone scene via JSON round-trip
    let scene_clone = clone_scene(&project_ui.scene)?;

    let project_ctx = engine_editor::ProjectContext {
        manifest: project_ui.manifest.clone(),
        scene: scene_clone,
        asset_db: project_ui.database.clone(),
        project_root: project_ui.root.clone(),
    };

    let mut session = AgentSession::new(project_ctx)?;

    let plan = AgentPlan {
        operations: approved_ops
            .into_iter()
            .map(|op| PlannedOperation {
                operation: op,
                preview: String::new(),
                requires_write: true,
            })
            .collect(),
        read_only: false,
        requires_write: true,
        policy: PermissionPolicy::transactional_write(),
    };

    let outcome = session.apply_plan(&plan)?;

    // Sync the modified scene back to the shell
    if let Some(outcome_summary) = &outcome.summary {
        project_ui.scene_dirty = true;
        return Ok(outcome_summary.clone());
    }

    // Record trace
    ui_state.copilot.trace_entries = outcome
        .trace_entries
        .iter()
        .map(|e| format!("{}: {} — {}", e.tool, e.result, e.recovery_hint))
        .collect();
    ui_state.copilot.console_entry_count = outcome.console_entries.len();
    ui_state.copilot.console_error_count = outcome
        .console_entries
        .iter()
        .filter(|e| e.level == engine_editor::ConsoleLevel::Error)
        .count();

    Ok(format!(
        "Applied {} operations",
        outcome.operations_performed
    ))
}

/// Generates a JSON agent command string for the stub model.
fn generate_agent_json(prompt: &str) -> String {
    let prompt_lower = prompt.to_lowercase();

    if prompt_lower.contains("create player") || prompt_lower.contains("add player") {
        r#"[
  {
    "action": "create_object",
    "name": "Player",
    "components": [
      { "type": "Rigidbody" },
      { "type": "Collider" }
    ],
    "position": [0.0, 1.0, 0.0]
  },
  {
    "action": "write_script",
    "path": "scripts/player_controller.rhai",
    "source": "// Player controller\nfn update(dt) {\n    let input = input::get_axis(\"horizontal\");\n    transform.translate_x(input * 5.0 * dt);\n}\n"
  },
  {
    "action": "complete",
    "summary": "Created Player object with Rigidbody+Collider components and player controller script"
  }
]"#
        .to_owned()
    } else if prompt_lower.contains("create camera") || prompt_lower.contains("add camera") {
        r#"[
  {
    "action": "create_object",
    "name": "Camera",
    "components": [
      { "type": "Camera" }
    ],
    "position": [0.0, 1.5, -6.0]
  },
  {
    "action": "complete",
    "summary": "Created Camera object"
  }
]"#
        .to_owned()
    } else if prompt_lower.contains("light") || prompt_lower.contains("directional") {
        r#"[
  {
    "action": "create_object",
    "name": "Directional Light",
    "components": [
      { "type": "Light" }
    ],
    "position": [5.0, 10.0, 5.0]
  },
  {
    "action": "complete",
    "summary": "Created Directional Light"
  }
]"#
        .to_owned()
    } else if prompt_lower.contains("script") || prompt_lower.contains("rhai") {
        r#"[
  {
    "action": "write_script",
    "path": "scripts/my_script.rhai",
    "source": "// Auto-generated script\nfn start() {\n    print('Script started!');\n}\n\nfn update(dt) {\n}\n"
  },
  {
    "action": "complete",
    "summary": "Created Rhai script at scripts/my_script.rhai"
  }
]"#
        .to_owned()
    } else if prompt_lower.contains("explain") || prompt_lower.contains("what") {
        r#"[
  {
    "action": "read_file",
    "path": "scenes/main.aster_scene.json"
  },
  {
    "action": "complete",
    "summary": "Read the scene file for inspection"
  }
]"#
        .to_owned()
    } else if prompt_lower.contains("undo") || prompt_lower.contains("revert") {
        r#"[
  {
    "action": "execute_command",
    "command": "edit.undo",
    "params": {}
  },
  {
    "action": "complete",
    "summary": "Undo the last operation"
  }
]"#
        .to_owned()
    } else {
        r#"[
  {
    "action": "complete",
    "summary": "I'm ready to help. You can ask me to create objects, write scripts, add components, or explain the scene."
  }
]"#
        .to_owned()
    }
}
