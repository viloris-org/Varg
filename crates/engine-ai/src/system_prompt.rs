//! Builds the system prompt for the AI model.
//!
//! The prompt describes the Aster engine's capabilities, the available
//! operations, and the constraints the AI must follow.

use engine_editor::EditorCommand;

/// Builds the system prompt from available commands.
///
/// Includes:
/// - Engine context description
/// - Available commands
/// - Output format requirements
/// - Safety and best-practice constraints
pub fn build(available_commands: &[&EditorCommand]) -> String {
    let mut prompt = String::new();

    prompt.push_str(include_str!("system_prompt_base.txt"));

    prompt.push_str("\n\n## Available Commands\n\n");
    prompt.push_str("You can execute these editor commands via the `execute_command` tool:\n\n");
    for cmd in available_commands {
        prompt.push_str(&format!(
            "- **{}** (`{}`) — category: {}\n",
            cmd.label, cmd.id, cmd.category
        ));
    }

    prompt.push_str(
        r#"

## Response Format

Answer the user in natural language to explain your reasoning and proposed changes.
Use the provided tools/functions to request project operations. Do NOT embed JSON in
code blocks — use the tool calling interface for all operations.

When no project changes are needed, just respond with text. When changes are needed,
explain what you plan to do, then call the appropriate tools.

## Constraints

1. **Write custom scripts** — Never use behavior presets. Every game deserves unique logic.
2. **Entity references**: Use entity names (e.g., "Player") when possible; the system resolves to IDs automatically.
3. **Component types**: Camera, MeshRenderer, Light, Rigidbody, Collider, AudioSource, Script, Sprite2D, ParticleEmitter.
4. **Error handling**: If an operation fails, read the error message and try a different approach. Don't repeat the same failed operation.
5. **Natural language queries**: Use `query_scene_semantic` to find entities before modifying them.
6. **Conversational responses**: Never call a tool for a request that only needs a conversational answer.
7. **Complete build plans**: The full project context is already attached to every request. For a build or modification request, do not return a scene query as the only tool unless its result is genuinely required before concrete writes can be proposed.

## Best Practices

- Start with `query_scene_semantic` to understand what entities exist
- Use `write_script` for all gameplay logic — scripts are powerful and flexible
- Attach scripts to entities via the `Script` component with `backend: "rhai"`
- Provide helpful explanations before calling tools
- If unsure about entity names/IDs, query first
- When multiple operations are needed, call them in logical order
- Call `complete` with a summary when the task is done
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
    fn system_prompt_uses_tool_calling_format() {
        let registry = CommandRegistry::default();
        let commands: Vec<&EditorCommand> = registry.commands().collect();
        let prompt = build(&commands);
        assert!(prompt.contains("create_object"));
        assert!(prompt.contains("write_script"));
        assert!(prompt.contains("tool calling interface"));
        assert!(prompt.contains("natural language"));
    }
}
