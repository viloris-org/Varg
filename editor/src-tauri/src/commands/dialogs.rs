#[tauri::command]
pub(crate) async fn select_project_location() -> Result<Option<String>, String> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Select Project Location")
        .pick_folder()
        .await;

    Ok(folder.map(|f| f.path().to_string_lossy().into_owned()))
}

#[tauri::command]
pub(crate) async fn open_scene_dialog() -> Result<Option<String>, String> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Scene JSON", &["json", "scene"])
        .pick_file()
        .await;

    Ok(file.map(|f| f.path().to_string_lossy().into_owned()))
}

#[tauri::command]
pub(crate) async fn save_scene_as_dialog() -> Result<Option<String>, String> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Scene JSON", &["json", "scene"])
        .set_file_name("scene.json")
        .save_file()
        .await;

    Ok(file.map(|f| f.path().to_string_lossy().into_owned()))
}

#[tauri::command]
pub(crate) async fn import_asset_dialog() -> Result<Option<String>, String> {
    let file = rfd::AsyncFileDialog::new()
        .set_title("Import Asset")
        .pick_file()
        .await;

    Ok(file.map(|f| f.path().to_string_lossy().into_owned()))
}
