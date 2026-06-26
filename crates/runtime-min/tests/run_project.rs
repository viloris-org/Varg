//! Integration test for run_project command.

use runtime_min::load_runtime_project;
use std::path::PathBuf;

/// Verifies that load_runtime_project can load the example project.
#[test]
fn load_runtime_project_loads_example() {
    let workspace_root = find_workspace_root();
    let project_path = workspace_root.join("examples/project");

    let project = load_runtime_project(&project_path).expect("load example project");

    assert_eq!(project.manifest.name, "Varg Example");
    assert_eq!(project.manifest.asset_root, "assets");
    assert_eq!(project.manifest.default_scene, "scenes/example.vscene");
    assert!(
        !project.scene.objects().is_empty(),
        "scene should have objects"
    );
}

/// Verifies that load_runtime_project returns an error for non-existent projects.
#[test]
fn load_runtime_project_fails_for_missing_project() {
    let result = load_runtime_project("/nonexistent/project");
    assert!(result.is_err(), "should fail for missing project");
}

/// Verifies that load_runtime_project returns an error for invalid manifest.
#[test]
fn load_runtime_project_fails_for_invalid_manifest() {
    let temp_dir = std::env::temp_dir().join("aster_test_invalid_manifest");
    let _ = std::fs::create_dir_all(&temp_dir);
    let manifest_path = temp_dir.join("Varg.toml");
    std::fs::write(&manifest_path, "invalid toml content {{{").unwrap();

    let result = load_runtime_project(&temp_dir);
    assert!(result.is_err(), "should fail for invalid manifest");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn find_workspace_root() -> PathBuf {
    let mut current = std::env::current_dir().expect("get current dir");
    loop {
        if current.join("Cargo.toml").is_file() && current.join("examples").is_dir() {
            return current;
        }
        if !current.pop() {
            panic!("could not find workspace root");
        }
    }
}
