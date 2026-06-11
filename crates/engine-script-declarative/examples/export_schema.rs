//! Binary to export the JSON Schema for LLM tool use.

use engine_script_declarative::generate_json_schema;
use std::io::Write;

fn main() {
    let schema = generate_json_schema();
    let json = serde_json::to_string_pretty(&schema).expect("Failed to serialize schema");

    // Write to stdout
    println!("{}", json);

    // Also write to file
    let output_path = "schema/aster-behavior-schema.json";
    if let Some(parent) = std::path::Path::new(output_path).parent() {
        std::fs::create_dir_all(parent).expect("Failed to create schema directory");
    }

    let mut file = std::fs::File::create(output_path).expect("Failed to create schema file");
    file.write_all(json.as_bytes())
        .expect("Failed to write schema");

    eprintln!("✓ JSON Schema exported to {}", output_path);
}
