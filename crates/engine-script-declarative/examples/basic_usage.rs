//! Example of using the declarative scripting backend.

use engine_core::math::Transform;
use engine_ecs::Scene;
use engine_platform::InputState;
use engine_script_declarative::DeclarativeScriptBackend;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Declarative Script Backend Example\n");

    // Create backend
    let mut backend = DeclarativeScriptBackend::new();
    println!("✓ Created declarative script backend");

    // Compile from inline JSON with simple structure
    let json = r#"{
        "entity": "Player",
        "description": "Simple test behavior",
        "behaviors": [
            {
                "type": "Action",
                "do": "Idle"
            }
        ]
    }"#;

    let logical_path = Path::new("inline_behavior.json");
    backend.compile_source(logical_path, json)?;
    println!("✓ Compiled inline behavior");

    // Create a test scene
    let mut scene = Scene::new();
    let player = scene.create_object("Player")?;
    scene
        .transforms_mut()
        .set_local(player, Transform::IDENTITY);
    println!("✓ Created test scene with player entity");

    // Set up input state
    let input = InputState::default();
    backend.set_input_state(input);
    backend.set_scene(scene);
    println!("✓ Configured backend with scene and input");

    // Execute behavior (read-only)
    let result = backend.execute(player, logical_path, 0.016)?;
    println!("✓ Executed behavior tree: {:?}", result);

    println!("\n✓ Example completed successfully!");
    println!("\nTo see more complex examples, check:");
    println!("  - examples/behaviors/player_controller.json");
    println!("  - examples/behaviors/enemy_patrol.json");
    println!("  - examples/behaviors/fleeing_npc.json");

    Ok(())
}
