//! Builds the system prompt for the AI model.
//!
//! The prompt describes the Varg Engine capabilities, the available
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

Answer the user briefly to explain the intended change.
Use the provided tools/functions to request project operations. Do NOT embed JSON in
code blocks — use the tool calling interface for all operations.

When no project changes are needed, just respond with text. When changes are needed,
give a short one-sentence intent, then call the appropriate tools.

## Constraints

1. **Write custom scripts** — Never use behavior presets. Every game deserves unique logic.
2. **No orphan scripts**: Never call `complete` for a build request if new gameplay scripts are only written to disk and not attached to scene entities.
3. **Visible scene result**: For requests like "make a scene", "build a prototype", "create an environment", or "add gameplay", include concrete scene operations (`create_object`, component properties, camera/light setup) so the editor viewport can show the result.
4. **Entity references**: Use entity names (e.g., "Player") when possible; the system resolves to IDs automatically.
5. **Component types**: Camera, MeshRenderer, Light, Rigidbody, Collider, AudioSource, Script, Sprite2D, ParticleEmitter.
6. **Error handling**: If an operation fails, read the error message and try a different approach. Don't repeat the same failed operation.
7. **Natural language queries**: Use `query_scene_semantic` when the target entity is ambiguous. Do not query by default when the attached project context already contains enough information.
8. **Conversational responses**: Never call a tool for a request that only needs a conversational answer.
9. **Tasks over plans**: Copilot is for short editor tasks. When tracking is useful, create or update concise tasks. For a build or modification request, do not return a scene query as the only tool unless its result is genuinely required before concrete writes can be proposed.
10. **Final script acceptance**: After all `.varg` writes are finished, call `check_script` once for all changed scripts. Fix all diagnostics and only then call `complete`. Do not validate after every individual write.

## Best Practices

- Use the attached project context before querying
- Use `write_script` for all gameplay logic — scripts are powerful and flexible
- Write Varg script files with the `.varg` extension
- Attach scripts via `Script` components without a backend identifier
- Pair script writes with scene entity/component operations for playable or viewable build requests
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
        assert!(prompt.contains("check_script"));
        assert!(prompt.contains("tool calling interface"));
        assert!(prompt.contains("respond with text"));
        assert!(prompt.contains("Short build example"));
        assert!(prompt.contains("Incorrect tool flow"));
        assert!(prompt.contains("No orphan scripts"));
    }

    #[test]
    fn system_prompt_teaches_varg_script_with_compact_examples() {
        let prompt = build(&[]);
        assert!(prompt.contains("Varg script Examples"));
        assert!(prompt.contains("scripts/player.varg"));
        assert!(prompt.contains("### 16. Timed projectile lifetime"));
        assert!(prompt.contains("Do not validate after every individual write"));
        assert!(!prompt.contains("scripts/player.aster"));
    }

    #[test]
    fn every_varg_example_passes_language_service_validation() {
        let prompt = build(&[]);
        let prompt = prompt.replace("\r\n", "\n");
        let examples = prompt
            .split("```varg\n")
            .skip(1)
            .filter_map(|section| section.split("\n```").next())
            .collect::<Vec<_>>();

        assert!(examples.len() >= 16);
        for (index, example) in examples.into_iter().enumerate() {
            let diagnostics = engine_script_varg::diagnose_source(
                std::path::Path::new("scripts/prompt_example.varg"),
                example,
            );
            assert!(
                diagnostics.is_empty(),
                "example {} failed validation: {diagnostics:?}",
                index + 1
            );
        }
    }
}
