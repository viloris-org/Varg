//! Simplified example to test core functionality.

use engine_core::math::Transform;
use engine_ecs::Scene;
use engine_platform::InputState;
use engine_script_declarative::{
    ActionExpr, BehaviorNode, BehaviorSchema, DeclarativeScriptBackend,
};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Declarative Script Backend - Core Test\n");

    // Create backend
    let mut backend = DeclarativeScriptBackend::new();
    println!("✓ Created declarative script backend");

    // Create behavior programmatically instead of from JSON
    let behavior = BehaviorNode::Action {
        action: ActionExpr::Idle,
    };

    let schema = BehaviorSchema::new("TestEntity", vec![behavior]);

    // Validate
    schema.validate()?;
    println!("✓ Created and validated behavior schema");

    // Serialize to JSON to show format
    let json = serde_json::to_string_pretty(&schema)?;
    println!("\nGenerated JSON schema:");
    println!("{}", json);

    // Compile from source
    let logical_path = Path::new("test_behavior.json");
    backend.compile_source(logical_path, &json)?;
    println!("\n✓ Compiled behavior from JSON");

    // Create test scene
    let mut scene = Scene::new();
    let entity = scene.create_object("TestEntity")?;
    scene
        .transforms_mut()
        .set_local(entity, Transform::IDENTITY);
    println!("✓ Created test scene");

    // Execute
    backend.set_input_state(InputState::default());
    backend.set_scene(scene);
    let result = backend.execute(entity, logical_path, 0.016)?;
    println!("✓ Executed behavior: {:?}", result);

    println!("\n✅ All tests passed!");
    println!("\nThe declarative behavior system is working.");
    println!("Note: Complex JSON examples need schema alignment - see IMPLEMENTATION_SUMMARY.md");

    Ok(())
}
