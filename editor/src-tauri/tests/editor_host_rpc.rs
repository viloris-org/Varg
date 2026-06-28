//! Integration tests for the Tauri editor RPC backend.
//!
//! Tests the `EditorHost` RPC dispatch directly (headless, no Tauri window).
use engine_editor::{DurableEditorState, EditorPreferences, FileEditorStore};
use varg_editor_tauri_lib::EditorHost;

fn temp_store() -> (tempfile::TempDir, FileEditorStore) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let store = FileEditorStore::new(&dir.path().join("varg-test-state.toml"));
    (dir, store)
}

fn create_host() -> EditorHost {
    let (_dir, store) = temp_store();
    EditorHost::new(store).expect("create editor host")
}

fn create_project(host: &mut EditorHost) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir");
    host.handle(
        "hub/create_project",
        &serde_json::json!({
            "name": "TestProject",
            "location": tmp.path().to_str().unwrap(),
            "template_id": "three_d",
            "toolchain_version": "0.1.0",
        }),
    )
    .expect("create project");
    tmp
}

fn open_project(host: &mut EditorHost, tmp: &tempfile::TempDir) -> String {
    let path = tmp.path().join("TestProject");
    host.handle(
        "hub/open_project",
        &serde_json::json!({ "path": path.to_str().unwrap() }),
    )
    .expect("open project");
    path.to_string_lossy().to_string()
}

#[test]
fn host_initializes_with_empty_hub() {
    let mut host = create_host();
    let state = host
        .handle("hub/get_state", &serde_json::json!({}))
        .expect("get state");
    assert_eq!(state["page"], "projects", "starts on projects page");
    assert_eq!(state["theme"], "system", "default theme is system");
}

#[test]
fn hub_get_state_returns_recent_projects() {
    let mut host = create_host();
    let state = host
        .handle("hub/get_state", &serde_json::json!({}))
        .expect("get state");
    let projects = state["recent_projects"].as_array().unwrap();
    assert!(projects.is_empty(), "no projects initially");
}

#[test]
fn host_init_defers_last_project_reopen() {
    let (tmp, store) = temp_store();
    let project_root = tmp.path().join("DeferredProject");
    let state = DurableEditorState {
        preferences: EditorPreferences::default(),
        last_open_project: Some(project_root.clone()),
        ..DurableEditorState::default()
    };
    store.save(&state).expect("save durable state");

    let mut host = EditorHost::new(store).expect("create editor host");
    let hub = host
        .handle("hub/get_state", &serde_json::json!({}))
        .expect("get hub state");
    let shell = host
        .handle("shell/get_state", &serde_json::json!({}))
        .expect("get shell state");

    assert_eq!(hub["open_project"], serde_json::Value::Null);
    assert_eq!(
        hub["last_open_project"].as_str(),
        Some(project_root.to_string_lossy().as_ref())
    );
    assert_eq!(hub["reopen_last_project"], true);
    assert_eq!(shell["has_project"], false);
}

#[test]
fn hub_set_theme_toggles_preference() {
    let mut host = create_host();

    let light = host
        .handle("hub/set_theme", &serde_json::json!({ "theme": "light" }))
        .expect("set theme light");
    assert_eq!(light["theme"], "light");

    let state = host
        .handle("hub/get_state", &serde_json::json!({}))
        .expect("get state");
    assert_eq!(state["theme"], "light");
}

#[test]
fn hub_set_page_changes_page() {
    let mut host = create_host();

    let result = host
        .handle("hub/set_page", &serde_json::json!({ "page": "settings" }))
        .expect("set page settings");
    assert_eq!(result["page"], "settings");

    let state = host
        .handle("hub/get_state", &serde_json::json!({}))
        .expect("get state");
    assert_eq!(state["page"], "settings");
}

#[test]
fn hub_set_locale_toggles_language() {
    let mut host = create_host();

    let zh = host
        .handle("hub/set_locale", &serde_json::json!({ "locale": "zh" }))
        .expect("set locale zh");
    assert_eq!(zh["locale"], "zh");

    let state = host
        .handle("hub/get_state", &serde_json::json!({}))
        .expect("get state");
    assert_eq!(state["locale"], "zh");

    let en = host
        .handle("hub/set_locale", &serde_json::json!({ "locale": "en" }))
        .expect("set locale en");
    assert_eq!(en["locale"], "en");

    let state = host
        .handle("hub/get_state", &serde_json::json!({}))
        .expect("get state");
    assert_eq!(state["locale"], "en");
}

#[test]
fn hub_create_project_returns_plan() {
    let mut host = create_host();
    let tmp = tempfile::tempdir().expect("create temp dir");
    let location = tmp.path().to_path_buf();

    let result = host
        .handle(
            "hub/create_project",
            &serde_json::json!({
                "name": "TestProject",
                "location": location.to_str().unwrap(),
                "template_id": "three_d",
                "toolchain_version": "0.1.0",
            }),
        )
        .expect("create project");
    assert_eq!(result["name"], "TestProject");
    let path = result["path"].as_str().unwrap();
    assert!(
        path.contains("TestProject"),
        "path should contain project name: {path}"
    );

    // Verify project appears in recent list
    let state = host
        .handle("hub/get_state", &serde_json::json!({}))
        .expect("get state");
    let projects = state["recent_projects"].as_array().unwrap();
    assert!(
        projects.iter().any(|p| p["name"] == "TestProject"),
        "project should appear in recent list"
    );

    // TempDir drops here, cleaning up automatically
}

#[test]
fn shell_mutations_are_dirty_and_undoable() {
    let mut host = create_host();
    let tmp = create_project(&mut host);
    open_project(&mut host, &tmp);

    let before = host
        .handle("shell/get_scene_tree", &serde_json::json!({}))
        .expect("scene tree");
    let before_len = before["objects"].as_array().unwrap().len();

    host.handle("shell/create_object", &serde_json::json!({}))
        .expect("create object");

    let state = host
        .handle("shell/get_state", &serde_json::json!({}))
        .expect("shell state");
    assert!(state["scene_dirty"].as_bool().unwrap());
    assert!(state["can_undo"].as_bool().unwrap());

    let after = host
        .handle("shell/get_scene_tree", &serde_json::json!({}))
        .expect("scene tree");
    assert_eq!(after["objects"].as_array().unwrap().len(), before_len + 1);

    host.handle("shell/undo", &serde_json::json!({}))
        .expect("undo");
    let undone = host
        .handle("shell/get_scene_tree", &serde_json::json!({}))
        .expect("scene tree");
    assert_eq!(undone["objects"].as_array().unwrap().len(), before_len);
}

#[test]
fn shell_save_clears_dirty_state() {
    let mut host = create_host();
    let tmp = create_project(&mut host);
    open_project(&mut host, &tmp);

    host.handle("shell/create_object", &serde_json::json!({}))
        .expect("create object");
    host.handle("shell/save_scene", &serde_json::json!({}))
        .expect("save scene");

    let state = host
        .handle("shell/get_state", &serde_json::json!({}))
        .expect("shell state");
    assert!(!state["scene_dirty"].as_bool().unwrap());
}

#[test]
fn shell_delete_object_removes_it_from_scene_tree() {
    let mut host = create_host();
    let tmp = create_project(&mut host);
    open_project(&mut host, &tmp);

    let created = host
        .handle("shell/create_object", &serde_json::json!({}))
        .expect("create object");
    let id = created["id"].as_str().unwrap();

    host.handle("shell/delete_object", &serde_json::json!({ "id": id }))
        .expect("delete object");

    let tree = host
        .handle("shell/get_scene_tree", &serde_json::json!({}))
        .expect("scene tree");
    let objects = tree["objects"].as_array().unwrap();
    assert!(
        objects.iter().all(|object| object["id"] != id),
        "deleted object should not remain in scene tree"
    );
}

#[test]
fn play_mode_starts_from_open_project_snapshot() {
    let mut host = create_host();
    let tmp = create_project(&mut host);
    open_project(&mut host, &tmp);

    let started = host
        .handle("play/start", &serde_json::json!({}))
        .expect("start play mode");
    assert!(started["playing"].as_bool().unwrap());

    let state = host
        .handle("play/get_state", &serde_json::json!({}))
        .expect("play state");
    assert!(state["playing"].as_bool().unwrap());

    host.handle("play/stop", &serde_json::json!({}))
        .expect("stop play mode");
    let stopped = host
        .handle("play/get_state", &serde_json::json!({}))
        .expect("play state");
    assert!(!stopped["playing"].as_bool().unwrap());
}

#[test]
fn project_creates_material_prefab_and_scene_assets() {
    let mut host = create_host();
    let tmp = create_project(&mut host);
    let project_root = open_project(&mut host, &tmp);
    let asset_root = std::path::Path::new(&project_root).join("assets");

    let material = host
        .handle(
            "project/create_material",
            &serde_json::json!({ "name": "new_material" }),
        )
        .expect("create material");
    assert_eq!(material["path"], "materials/new_material.vasset");
    let material_text =
        std::fs::read_to_string(asset_root.join("materials/new_material.vasset")).unwrap();
    let diagnostics =
        engine_script_varg::diagnose_source("materials/new_material.vasset", &material_text);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");

    let prefab = host
        .handle(
            "project/create_prefab",
            &serde_json::json!({ "name": "new_prefab" }),
        )
        .expect("create prefab");
    assert_eq!(prefab["path"], "prefabs/new_prefab.vscene");
    let prefab_text =
        std::fs::read_to_string(asset_root.join("prefabs/new_prefab.vscene")).unwrap();
    let diagnostics =
        engine_script_varg::diagnose_source("prefabs/new_prefab.vscene", &prefab_text);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");

    let scene = host
        .handle(
            "project/create_scene",
            &serde_json::json!({ "name": "new_scene" }),
        )
        .expect("create scene");
    assert_eq!(scene["path"], "scenes/new_scene.vscene");
    let scene_text = std::fs::read_to_string(asset_root.join("scenes/new_scene.vscene")).unwrap();
    let (scene, diagnostics) =
        engine_script_varg::compile_vscene_source_to_scene("scenes/new_scene.vscene", &scene_text);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    scene.expect("scene parses");

    let assets = host
        .handle("project/list_assets", &serde_json::json!({}))
        .expect("list assets");
    let asset_rows = assets["assets"].as_array().unwrap();
    assert!(asset_rows.iter().any(
        |asset| asset["source_path"] == "materials/new_material.vasset"
            && asset["kind"] == "Material"
    ));
    assert!(asset_rows.iter().any(
        |asset| asset["source_path"] == "prefabs/new_prefab.vscene" && asset["kind"] == "Scene"
    ));
    assert!(
        asset_rows
            .iter()
            .any(|asset| asset["source_path"] == "scenes/new_scene.vscene"
                && asset["kind"] == "Scene")
    );
}

#[test]
fn project_lists_scene_references_for_script_assets() {
    let mut host = create_host();
    let tmp = create_project(&mut host);
    open_project(&mut host, &tmp);

    let script = host
        .handle(
            "project/create_script",
            &serde_json::json!({ "name": "controller" }),
        )
        .expect("create script");
    let script_path = script["path"].as_str().unwrap();

    let object = host
        .handle(
            "shell/create_object",
            &serde_json::json!({ "name": "Scripted Object" }),
        )
        .expect("create object");
    let object_id = object["id"].as_str().unwrap();
    host.handle(
        "shell/add_component",
        &serde_json::json!({ "id": object_id, "component_type": "Script" }),
    )
    .expect("add script component");
    host.handle(
        "shell/update_component",
        &serde_json::json!({
            "id": object_id,
            "component_type": "Script",
            "data": { "script": script_path },
        }),
    )
    .expect("set script component path");

    let references = host
        .handle(
            "project/list_asset_references",
            &serde_json::json!({ "path": script_path }),
        )
        .expect("list references");
    let rows = references["references"].as_array().unwrap();
    assert!(rows.iter().any(|row| {
        row["kind"] == "scene"
            && row["label"] == "Script component"
            && row["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("Scripted Object"))
    }));
}

#[test]
fn console_push_and_retrieve_entries() {
    let mut host = create_host();

    // Push an error entry
    host.handle(
        "console/push_entry",
        &serde_json::json!({
            "level": "error",
            "message": "test error message",
            "subsystem": "test",
        }),
    )
    .expect("push entry");

    // Retrieve entries
    let entries = host
        .handle("console/get_entries", &serde_json::json!({}))
        .expect("get entries");
    let list = entries["entries"].as_array().unwrap();
    assert!(!list.is_empty(), "should have at least one entry");

    let entry = &list[0];
    assert_eq!(entry["level"], "error");
    assert_eq!(entry["message"], "test error message");
}

#[test]
fn console_clear_removes_entries() {
    let mut host = create_host();

    host.handle(
        "console/push_entry",
        &serde_json::json!({
            "level": "info",
            "message": "temp message",
            "subsystem": "test",
        }),
    )
    .expect("push entry");

    host.handle("console/clear", &serde_json::json!({}))
        .expect("clear");

    let entries = host
        .handle("console/get_entries", &serde_json::json!({}))
        .expect("get entries");
    let list = entries["entries"].as_array().unwrap();
    assert!(list.is_empty(), "console should be empty after clear");
}

#[test]
fn console_push_trace_debug_warn_levels() {
    let mut host = create_host();

    for (level, label) in &[
        ("trace", "trace"),
        ("debug", "debug"),
        ("warn", "warn"),
        ("error", "error"),
        ("info", "info"),
    ] {
        host.handle(
            "console/push_entry",
            &serde_json::json!({
                "level": level,
                "message": format!("{label} message"),
                "subsystem": "test",
            }),
        )
        .expect("push entry");
    }

    let entries = host
        .handle("console/get_entries", &serde_json::json!({}))
        .expect("get entries");
    let list = entries["entries"].as_array().unwrap();
    assert_eq!(list.len(), 5);
    assert_eq!(list[0]["level"], "trace");
    assert_eq!(list[1]["level"], "debug");
    assert_eq!(list[2]["level"], "warn");
    assert_eq!(list[3]["level"], "error");
    assert_eq!(list[4]["level"], "info");
}

#[test]
fn unknown_rpc_method_returns_error() {
    let mut host = create_host();
    let result = host.handle("no/such_method", &serde_json::json!({}));
    assert!(result.is_err(), "unknown method should error");
    assert!(
        result.unwrap_err().to_string().contains("unknown method"),
        "error should mention unknown method"
    );
}

#[test]
fn missing_required_params_returns_error() {
    let mut host = create_host();

    // hub/create_project without name
    let result = host.handle("hub/create_project", &serde_json::json!({}));
    assert!(result.is_err(), "missing name should error");
}

#[test]
fn shell_get_state_before_project_open() {
    let mut host = create_host();
    let state = host
        .handle("shell/get_state", &serde_json::json!({}))
        .expect("get shell state");
    assert!(!state["has_project"].as_bool().unwrap(), "no project open");
    assert_eq!(
        state["project_name"],
        serde_json::Value::Null,
        "no project name"
    );
    assert!(!state["can_undo"].as_bool().unwrap());
    assert!(!state["can_redo"].as_bool().unwrap());
}

#[test]
fn hub_get_desktop_integration() {
    let mut host = create_host();
    let di = host
        .handle("app/get_desktop_integration", &serde_json::json!({}))
        .expect("get desktop integration");
    assert!(di["desktop_environment"].is_string());
    assert!(di["prefers_native_chrome"].is_boolean());
    assert!(di["window_background"].is_string());
}
