//! Builds the system prompt for the AI model.
//!
//! The prompt describes the Aster engine's capabilities, the available
//! operations, and the constraints the AI must follow.

use engine_editor::EditorCommand;

/// Builds the system prompt from available commands.
///
/// Includes:
/// - Engine context description
/// - Available operations with their schemas
/// - Output format requirements
/// - Safety and best-practice constraints
pub fn build(available_commands: &[&EditorCommand]) -> String {
    let mut prompt = String::new();

    prompt.push_str(include_str!("system_prompt_base.txt"));

    prompt.push_str("\n\n## Available Commands\n\n");
    prompt.push_str("You can execute these editor commands via `execute_command`:\n\n");
    for cmd in available_commands {
        prompt.push_str(&format!(
            "- **{}** (`{}`) — category: {}\n",
            cmd.label, cmd.id, cmd.category
        ));
    }

    prompt.push_str(
        r#"

## Response Format

Answer the user in natural language. If the request only needs an explanation,
question, or recommendation, do not emit operations.

When project changes are needed, briefly explain the proposed result and append
one fenced `aster_operations` block containing a JSON array. JSON is an internal
tool protocol and must never be shown as the conversational answer.

```aster_operations
[{ "action": "write_script", "path": "scripts/enemy.rhai", "source": "..." }]
```

Available internal actions:

### write_script — Create or update Rhai scripts
```json
{
  "action": "write_script",
  "path": "scripts/player.rhai",
  "source": "fn on_update(dt) {\n    let speed = 5.0;\n    if is_held(\"W\") { translate(0.0, 0.0, -speed * dt); }\n    if is_held(\"S\") { translate(0.0, 0.0, speed * dt); }\n}"
}
```

### create_object — Create entity with components
```json
{
  "action": "create_object",
  "name": "Player",
  "position": [0.0, 0.0, 0.0],
  "components": [
    { "type": "Rigidbody" },
    { "type": "Collider", "properties": { "shape": "capsule" } },
    { "type": "Script", "properties": { "backend": "rhai", "path": "scripts/player.rhai" } }
  ]
}
```

### set_property — Modify component field
```json
{ "action": "set_property", "entity": "Player", "component": "Rigidbody", "field": "mass", "value": 2.0 }
```

### query_scene_semantic — Find entities by description
```json
{ "action": "query_scene_semantic", "query": "all enemies" }
```
Supported patterns: "all X", "entities with X", "X near Y", direct name matches

### move_entity_to — Move with optional animation
```json
{ "action": "move_entity_to", "entity": "Player", "position": [0, 5, 0], "animated": true, "duration": 2.0 }
```

### batch_operation — Multiple operations with rollback
```json
{
  "action": "batch_operation",
  "rollback_on_failure": true,
  "operations": [
    { "action": "create_object", "name": "Enemy1", "position": [5, 0, 0], "components": [...] },
    { "action": "write_script", "path": "scripts/enemy1.rhai", "source": "..." }
  ]
}
```

### execute_command — Run editor commands
```json
{ "action": "execute_command", "command": "gameobject.create_empty" }
```

### remove_component / destroy_object
```json
{ "action": "remove_component", "entity": "Player", "component": "Rigidbody" }
{ "action": "destroy_object", "entity": "1:2" }
```

### read_file — Read project files
```json
{ "action": "read_file", "path": "scripts/player.rhai" }
```

### show_in_viewport — Highlight entity in editor
```json
{ "action": "show_in_viewport", "entity": "Player", "highlight": true, "frame": true }
```

## Constraints

1. **Write custom scripts** — Never use behavior presets. Every game deserves unique logic.
2. **Entity references**: Use entity names (e.g., "Player") when possible; the system resolves to IDs automatically.
3. **Component types**: Camera, MeshRenderer, Light, Rigidbody, Collider, AudioSource, Script, Sprite2D, ParticleEmitter.
4. **Error handling**: If an operation fails, read the error message and try a different approach. Don't repeat the same failed operation.
5. **Natural language queries**: Use `query_scene_semantic` to find entities before modifying them.
6. **Batching**: For multiple related operations, use `batch_operation` with `rollback_on_failure: true` for safety.
7. **Conversational responses**: Never emit an operation for a request that only needs a conversational answer.
8. **Complete build plans**: The full project context is already attached to every request. For a build or modification request, do not return a scene query as the only operation unless its result is genuinely required before concrete writes can be proposed. After a tool-result continuation, do not repeat the same query; emit the remaining write operations or explicitly complete the task.

## Best Practices

- Start with `query_scene_semantic` to understand what entities exist
- Use `write_script` for all gameplay logic — scripts are powerful and flexible
- Attach scripts to entities via the `Script` component with `backend: "rhai"`
- Use `batch_operation` for multi-step changes
- Provide helpful explanations before operations
- If unsure about entity names/IDs, query first
"#
    );

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_editor::{CommandAvailability, CommandRegistry, EditorCommand};

    #[test]
    fn system_prompt_includes_commands() {
        let mut registry = CommandRegistry::default();
        registry.register(EditorCommand {
            id: "test.command".into(),
            label: "Test Command".into(),
            category: "Test".into(),
            shortcut: None,
            availability: CommandAvailability::Always,
        });
        let commands: Vec<&EditorCommand> = registry.commands().collect();
        let prompt = build(&commands);
        assert!(prompt.contains("Test Command"));
        assert!(prompt.contains("test.command"));
    }

    #[test]
    fn system_prompt_includes_action_descriptions() {
        let registry = CommandRegistry::default();
        let commands: Vec<&EditorCommand> = registry.commands().collect();
        let prompt = build(&commands);
        assert!(prompt.contains("create_object"));
        assert!(prompt.contains("write_script"));
        assert!(prompt.contains("aster_operations"));
        assert!(prompt.contains("natural language"));
        assert!(!prompt.contains("create_prefab"));
    }
}
