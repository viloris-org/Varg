#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Native Hub and editor shell state for the first Aster UI surface.
//!
//! When compiled with the `editor` feature, the `hub` and `shell` modules
//! provide egui rendering functions for [`HubState`] and [`EditorShell`].

#[cfg(feature = "editor")]
pub mod hub;
#[cfg(feature = "editor")]
pub mod shell;

#[cfg(feature = "editor")]
pub mod fonts;
#[cfg(feature = "editor")]
pub use fonts::setup_egui_fonts;
#[cfg(feature = "editor")]
pub use hub::draw_hub;
#[cfg(feature = "editor")]
pub use shell::{draw_shell, PlayModeRequest, ShellUiState};

use std::{
    fs,
    path::{Path, PathBuf},
};

use engine_assets::{
    import_builtin_asset, scan_project_assets, AssetDatabase, AssetGuid, AssetRegistry, ImportTask,
    ResourceKind, ResourceMetaFormat,
};
use engine_core::{
    math::{Transform, Vec3},
    EngineError, EngineResult,
};
use engine_ecs::{
    CameraComponentData, CameraRole, ComponentData, LightComponentData, MeshRendererComponentData,
    ProjectManifest, Scene,
};
use engine_editor::{
    register_core_commands, register_core_panels, CommandRegistry, ConsoleEntry, ConsoleLevel,
    ConsoleService, ConsoleSource, DurableEditorState, EditorPreferences, MemoryProjectStore,
    NewProjectRequest, PanelRegistry, ProjectCreationPlan, ProjectMetadata, ProjectStore,
    Selection, SelectionService, ThemePreference, ToolchainInstall, UndoCommand, UndoRedoStack,
};
use engine_i18n::Translations;

/// UI color tokens for a dense tool layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesignTokens {
    /// Window background.
    pub base: &'static str,
    /// Inputs, rows, and cards.
    pub surface: &'static str,
    /// Hovered rows and controls.
    pub surface_hover: &'static str,
    /// Separators and low-emphasis outlines.
    pub border: &'static str,
    /// Main text.
    pub text_primary: &'static str,
    /// Secondary metadata.
    pub text_secondary: &'static str,
    /// Primary action color.
    pub accent: &'static str,
    /// Destructive action color.
    pub danger: &'static str,
}

impl DesignTokens {
    /// Returns tokens for a theme preference, resolving system to dark until host integration exists.
    pub const fn for_theme(theme: ThemePreference) -> Self {
        match theme {
            ThemePreference::Light => Self {
                base: "#ffffff",
                surface: "#f7f7f5",
                surface_hover: "#efefed",
                border: "#e6e6e3",
                text_primary: "#37352f",
                text_secondary: "#787774",
                accent: "#37352f",
                danger: "#eb5757",
            },
            ThemePreference::System | ThemePreference::Dark => Self {
                base: "#181818",
                surface: "#202020",
                surface_hover: "#2a2a2a",
                border: "#303030",
                text_primary: "#d4d4d4",
                text_secondary: "#8a8a8a",
                accent: "#f2f2f2",
                danger: "#eb5757",
            },
        }
    }
}

/// Hub sidebar pages.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HubPage {
    /// Recent and created projects.
    #[default]
    Projects,
    /// Installed versions and local build artifacts.
    Installs,
    /// Preferences once they outgrow theme and paths.
    Settings,
}

/// Project deletion mode selected by the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectDeletionMode {
    /// Remove only from the recent-project list.
    RemoveRecent,
    /// Delete project files from disk after confirmation.
    DeleteFiles,
}

/// Result of a project deletion request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectDeletionDecision {
    /// A confirmation prompt must be shown.
    NeedsConfirmation {
        /// Project path being removed.
        path: PathBuf,
        /// Chosen deletion mode.
        mode: ProjectDeletionMode,
    },
    /// Deletion cannot proceed because the project is open.
    RefusedOpenProject {
        /// Project path that is currently open.
        path: PathBuf,
    },
    /// The recent list entry was removed.
    RemovedFromRecent {
        /// Project path removed from recents.
        path: PathBuf,
    },
    /// The caller may delete files and then remove the recent entry.
    DeleteFilesApproved {
        /// Project path to delete.
        path: PathBuf,
    },
}

/// Hub launch action for platform adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubAction {
    /// Open a folder in the host file browser.
    OpenFolder(PathBuf),
    /// Ask the host platform to select a parent directory for a new project.
    SelectProjectLocation,
    /// Launch the editor with a project and toolchain version.
    LaunchEditor {
        /// Project root path.
        project_path: PathBuf,
        /// Toolchain version to launch.
        toolchain_version: String,
    },
}

/// New-project dialog transient state.
#[derive(Clone, Debug, Default)]
pub struct NewProjectDialog {
    /// Project name input.
    pub name: String,
    /// Location input (string form for editing).
    pub location: String,
    /// Selected project template index.
    pub template_idx: usize,
    /// Selected toolchain version index.
    pub version_idx: usize,
    /// Validation error to display.
    pub error: Option<String>,
}

/// Confirm-delete dialog transient state.
#[derive(Clone, Debug)]
pub struct ConfirmDeleteDialog {
    /// Path being deleted.
    pub path: PathBuf,
    /// Deletion mode.
    pub mode: ProjectDeletionMode,
}

/// First Hub state model.
#[derive(Clone, Debug)]
pub struct HubState {
    page: HubPage,
    search: String,
    project_store: MemoryProjectStore,
    installs: Vec<ToolchainInstall>,
    preferences: EditorPreferences,
    open_project: Option<PathBuf>,
    new_project_error: Option<String>,
    /// Currently selected project path (transient UI state).
    pub selected_project: Option<PathBuf>,
    /// Open new-project dialog state; `None` means dialog is closed.
    pub new_project_dialog: Option<NewProjectDialog>,
    /// Open confirm-delete dialog state; `None` means dialog is closed.
    pub confirm_delete: Option<ConfirmDeleteDialog>,
    /// Pending hub action produced by the UI this frame.
    pub pending_action: Option<HubAction>,
}

impl HubState {
    /// Creates a Hub state object that starts on the Projects page.
    pub fn new(preferences: EditorPreferences) -> Self {
        Self {
            page: HubPage::Projects,
            search: String::new(),
            project_store: MemoryProjectStore::new(),
            installs: Vec::new(),
            preferences,
            open_project: None,
            new_project_error: None,
            selected_project: None,
            new_project_dialog: None,
            confirm_delete: None,
            pending_action: None,
        }
    }

    /// Creates Hub state from durable editor state loaded from local config.
    pub fn from_durable_state(state: DurableEditorState) -> Self {
        let mut hub = Self::new(state.preferences);
        for project in state.recent_projects.into_iter().rev() {
            hub.upsert_project(project);
        }
        hub.open_project = state.last_open_project;
        hub
    }

    /// Captures preferences, recents, layout, and last-open project for local persistence.
    pub fn durable_state(&self) -> DurableEditorState {
        let mut state = DurableEditorState::from_parts(
            self.preferences.clone(),
            &self.project_store,
            self.open_project.clone(),
        );
        state.layout = self.preferences.layout.clone();
        state
    }

    /// Returns the current page.
    pub const fn page(&self) -> HubPage {
        self.page
    }

    /// Switches sidebar page.
    pub fn set_page(&mut self, page: HubPage) {
        self.page = page;
    }

    /// Returns active design tokens.
    pub fn design_tokens(&self) -> DesignTokens {
        DesignTokens::for_theme(self.preferences.theme)
    }

    /// Switches theme immediately.
    pub fn set_theme(&mut self, theme: ThemePreference) {
        self.preferences.theme = theme;
    }

    /// Switches UI locale.
    pub fn set_locale(&mut self, locale: engine_i18n::Locale) {
        self.preferences.locale = locale;
    }

    /// Returns preferences.
    pub fn preferences(&self) -> &EditorPreferences {
        &self.preferences
    }

    /// Adds or updates a project card.
    pub fn upsert_project(&mut self, project: ProjectMetadata) {
        self.project_store.upsert_project(project);
    }

    /// Adds an installed toolchain.
    pub fn add_install(&mut self, install: ToolchainInstall) {
        self.installs
            .retain(|existing| existing.version != install.version);
        self.installs.push(install);
        self.installs
            .sort_by(|left, right| left.version.cmp(&right.version));
    }

    /// Returns installed toolchains.
    pub fn installs(&self) -> &[ToolchainInstall] {
        &self.installs
    }

    /// Sets the Projects page search query.
    pub fn set_search(&mut self, query: impl Into<String>) {
        self.search = query.into();
    }

    /// Returns project cards matching the current search query.
    pub fn filtered_projects(&self) -> Vec<&ProjectMetadata> {
        let query = self.search.trim().to_lowercase();
        self.project_store
            .projects()
            .iter()
            .filter(|project| {
                query.is_empty()
                    || project.name.to_lowercase().contains(&query)
                    || project
                        .path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&query)
                    || project.toolchain_version.to_lowercase().contains(&query)
            })
            .collect()
    }

    /// Returns the last visible new-project validation error.
    pub fn new_project_error(&self) -> Option<&str> {
        self.new_project_error.as_deref()
    }

    /// Validates a project creation request, remembers the location, and clears prior error state.
    pub fn create_project_plan(
        &mut self,
        request: &NewProjectRequest,
    ) -> EngineResult<ProjectCreationPlan> {
        self.new_project_error = None;
        match engine_editor::validate_new_project(request) {
            Ok(plan) => {
                self.preferences.last_project_location = request.location.clone();
                Ok(plan)
            }
            Err(error) => {
                self.new_project_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    /// Creates the project directory, manifest, asset root, and default scene from a validated plan.
    pub fn create_project_files(&self, plan: &ProjectCreationPlan) -> EngineResult<()> {
        if plan.path.exists() {
            let mut entries =
                fs::read_dir(&plan.path).map_err(|source| EngineError::Filesystem {
                    path: plan.path.clone(),
                    source,
                })?;
            if entries.next().is_some() {
                return Err(EngineError::config(format!(
                    "project directory already exists and is not empty: {}",
                    plan.path.display()
                )));
            }
        }

        let scenes_dir = plan.path.join("scenes");
        let assets_dir = plan.path.join("assets");
        create_dir_all(&scenes_dir)?;
        create_dir_all(&assets_dir)?;

        let mut manifest = ProjectManifest::example();
        manifest.name = plan.name.clone();
        manifest.asset_root = "assets".to_owned();
        manifest.default_scene = "scenes/main.aster_scene.json".to_owned();

        write_file(&plan.path.join("aster.project.toml"), &manifest.to_toml()?)?;

        let scene = scene_for_template(&plan.template_id)?;
        write_file(
            &plan.path.join(&manifest.default_scene),
            &scene.to_json(&plan.name)?,
        )?;

        Ok(())
    }

    /// Opens a project in this Hub session.
    pub fn mark_project_open(&mut self, path: impl Into<PathBuf>) {
        self.open_project = Some(path.into());
    }

    /// Builds an open-folder action.
    pub fn open_folder_action(&self, path: impl Into<PathBuf>) -> HubAction {
        HubAction::OpenFolder(path.into())
    }

    /// Builds an editor launch action after checking the requested toolchain exists.
    pub fn launch_editor_action(&self, project: &ProjectMetadata) -> EngineResult<HubAction> {
        if self
            .installs
            .iter()
            .any(|install| install.version == project.toolchain_version && install.editor_available)
        {
            Ok(HubAction::LaunchEditor {
                project_path: project.path.clone(),
                toolchain_version: project.toolchain_version.clone(),
            })
        } else {
            Err(EngineError::config(format!(
                "engine/toolchain `{}` is not installed or cannot launch the editor",
                project.toolchain_version
            )))
        }
    }

    /// Handles a deletion request while distinguishing recents from file deletion.
    pub fn request_project_deletion(
        &mut self,
        path: &Path,
        mode: ProjectDeletionMode,
        confirmed: bool,
    ) -> ProjectDeletionDecision {
        if self.open_project.as_deref() == Some(path) {
            return ProjectDeletionDecision::RefusedOpenProject {
                path: path.to_path_buf(),
            };
        }
        if !confirmed {
            return ProjectDeletionDecision::NeedsConfirmation {
                path: path.to_path_buf(),
                mode,
            };
        }
        match mode {
            ProjectDeletionMode::RemoveRecent => {
                self.project_store.remove_recent(path);
                ProjectDeletionDecision::RemovedFromRecent {
                    path: path.to_path_buf(),
                }
            }
            ProjectDeletionMode::DeleteFiles => ProjectDeletionDecision::DeleteFilesApproved {
                path: path.to_path_buf(),
            },
        }
    }
}

fn create_dir_all(path: &Path) -> EngineResult<()> {
    fs::create_dir_all(path).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })
}

fn write_file(path: &Path, contents: &str) -> EngineResult<()> {
    fs::write(path, contents).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })
}

fn scene_for_template(template_id: &str) -> EngineResult<Scene> {
    let mut scene = Scene::new();
    let camera = scene.create_object("Main Camera")?;
    let camera_object = scene
        .object_mut(camera)
        .ok_or_else(|| EngineError::invalid_handle("camera object metadata is missing"))?;
    camera_object.tag = "MainCamera".to_owned();
    camera_object.camera_role = Some(CameraRole::Main);
    scene.upsert_component(
        camera,
        ComponentData::Camera(CameraComponentData::default()),
    )?;

    let camera_z = if template_id == "two_d" { -10.0 } else { -6.0 };
    scene.transforms_mut().set_local(
        camera,
        Transform {
            translation: Vec3::new(0.0, 1.5, camera_z),
            ..Transform::IDENTITY
        },
    );

    match template_id {
        "three_d" => {
            let light = scene.create_object("Directional Light")?;
            scene.upsert_component(light, ComponentData::Light(LightComponentData::default()))?;

            let player = scene.create_object("Player")?;
            scene
                .object_mut(player)
                .ok_or_else(|| EngineError::invalid_handle("player object metadata is missing"))?
                .tag = "Player".to_owned();
            scene.upsert_component(
                player,
                ComponentData::MeshRenderer(MeshRendererComponentData::default()),
            )?;
        }
        "two_d" => {
            let root = scene.create_object("Scene Root")?;
            scene.transforms_mut().set_local(root, Transform::IDENTITY);
        }
        other => {
            return Err(EngineError::config(format!(
                "unknown project template `{other}`"
            )));
        }
    }

    Ok(scene)
}

/// First native editor shell state.
#[derive(Debug, Default)]
pub struct EditorShell {
    panels: PanelRegistry,
    commands: CommandRegistry,
    selection: SelectionService,
    console: ConsoleService,
    undo: UndoRedoStack,
    preferences: EditorPreferences,
    project: Option<ProjectContext>,
}

impl EditorShell {
    /// Creates an editor shell with core panels and commands registered.
    pub fn with_core_services(preferences: EditorPreferences) -> Self {
        let mut shell = Self {
            preferences,
            ..Self::default()
        };
        register_core_panels(&mut shell.panels);
        register_core_commands(&mut shell.commands);
        shell
    }

    /// Opens a project folder, loads its default scene, and scans its asset root.
    pub fn open_project(&mut self, project_root: impl Into<PathBuf>) -> EngineResult<()> {
        let project_root = project_root.into();
        let manifest_path = project_root.join("aster.project.toml");
        let manifest_text =
            fs::read_to_string(&manifest_path).map_err(|source| EngineError::Filesystem {
                path: manifest_path.clone(),
                source,
            })?;
        let manifest = toml::from_str::<ProjectManifest>(&manifest_text).map_err(|error| {
            EngineError::config(format!("project manifest parse failed: {error}"))
        })?;
        if let Some(diagnostic) = manifest.diagnostics().into_iter().next() {
            return Err(EngineError::config(format!(
                "{}: {}",
                diagnostic.path, diagnostic.message
            )));
        }
        let scene_path = project_root.join(&manifest.default_scene);
        let scene_text =
            fs::read_to_string(&scene_path).map_err(|source| EngineError::Filesystem {
                path: scene_path.clone(),
                source,
            })?;
        let scene = Scene::from_json(&scene_text)?;
        let mut database = AssetDatabase::new(
            project_root.join(&manifest.asset_root),
            project_root.join("builtin"),
        );
        let scan = scan_project_assets(project_root.join(&manifest.asset_root), &mut database)?;
        self.project = Some(ProjectContext {
            root: project_root.clone(),
            manifest,
            scene,
            database,
            registry: AssetRegistry::default(),
            assets: scan.metas,
            asset_imports: Vec::new(),
            scene_dirty: false,
            scene_path,
        });
        self.selection.clear();
        self.console.push(ConsoleEntry {
            timestamp: "now".to_string(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "editor".to_string(),
                file: None,
                line: None,
            },
            message: format!("opened project {}", project_root.display()),
        });
        Ok(())
    }

    /// Saves the active scene to the project's default scene path.
    pub fn save_scene(&mut self) -> EngineResult<()> {
        let Some(project) = self.project.as_mut() else {
            return Err(EngineError::config("no project is open"));
        };
        let scene_name = project
            .scene_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Scene");
        let json = project.scene.to_json(scene_name)?;
        fs::write(&project.scene_path, json).map_err(|source| EngineError::Filesystem {
            path: project.scene_path.clone(),
            source,
        })?;
        project.scene_dirty = false;
        Ok(())
    }

    /// Records an undoable editor command.
    pub fn push_undo(&mut self, command: UndoCommand) {
        self.undo.push(command);
    }

    /// Returns the undo/redo command stack.
    pub const fn undo_stack(&self) -> &UndoRedoStack {
        &self.undo
    }

    /// Pops an undo command for the host/editor tool to apply.
    pub fn pop_undo(&mut self) -> Option<UndoCommand> {
        self.undo.undo()
    }

    /// Pops a redo command for the host/editor tool to apply.
    pub fn pop_redo(&mut self) -> Option<UndoCommand> {
        self.undo.redo()
    }

    /// Applies the latest undo command when it contains a serialized scene snapshot.
    pub fn undo_scene_command(&mut self) -> EngineResult<bool> {
        let Some(command) = self.pop_undo() else {
            return Ok(false);
        };
        self.restore_scene_snapshot(&command.before)?;
        Ok(true)
    }

    /// Applies the latest redo command when it contains a serialized scene snapshot.
    pub fn redo_scene_command(&mut self) -> EngineResult<bool> {
        let Some(command) = self.pop_redo() else {
            return Ok(false);
        };
        self.restore_scene_snapshot(&command.after)?;
        Ok(true)
    }

    fn restore_scene_snapshot(&mut self, snapshot: &str) -> EngineResult<()> {
        let Some(project) = self.project.as_mut() else {
            return Err(EngineError::config("no project is open"));
        };
        project.scene = Scene::from_json(snapshot)?;
        project.scene_dirty = true;
        Ok(())
    }

    /// Returns the open project context.
    pub const fn project(&self) -> Option<&ProjectContext> {
        self.project.as_ref()
    }

    /// Returns the open project context mutably.
    pub fn project_mut(&mut self) -> Option<&mut ProjectContext> {
        self.project.as_mut()
    }

    /// Selects a scene object by stable ID.
    pub fn select_entity_id(&mut self, id: engine_core::EntityId) {
        self.selection
            .select(Selection::Entity(format!("{:032x}", id.as_u128())));
    }

    /// Returns the selected scene object ID, if the selection is an entity.
    pub fn selected_entity_id(&self) -> Option<engine_core::EntityId> {
        let Selection::Entity(value) = self.selection.selected()? else {
            return None;
        };
        u128::from_str_radix(value, 16)
            .ok()
            .map(engine_core::EntityId::from_u128)
    }

    /// Returns panel registry.
    pub const fn panels(&self) -> &PanelRegistry {
        &self.panels
    }

    /// Returns command registry.
    pub const fn commands(&self) -> &CommandRegistry {
        &self.commands
    }

    /// Returns selection service.
    pub const fn selection(&self) -> &SelectionService {
        &self.selection
    }

    /// Returns mutable selection service.
    pub fn selection_mut(&mut self) -> &mut SelectionService {
        &mut self.selection
    }

    /// Returns console service.
    pub const fn console(&self) -> &ConsoleService {
        &self.console
    }

    /// Returns mutable console service.
    pub fn console_mut(&mut self) -> &mut ConsoleService {
        &mut self.console
    }

    /// Returns preferences.
    pub const fn preferences(&self) -> &EditorPreferences {
        &self.preferences
    }
}

/// Open editor project data bound to shell panels.
#[derive(Debug)]
pub struct ProjectContext {
    /// Project root path.
    pub root: PathBuf,
    /// Parsed project manifest.
    pub manifest: ProjectManifest,
    /// Editable scene.
    pub scene: Scene,
    /// Asset database for GUID/path resolution.
    pub database: AssetDatabase,
    /// CPU/GPU resource registry used by import and preview workflows.
    pub registry: AssetRegistry,
    /// Last asset scan results shown by the Project panel.
    pub assets: Vec<ResourceMetaFormat>,
    /// Recent import/rescan status messages.
    pub asset_imports: Vec<String>,
    /// Whether the scene has unsaved edits.
    pub scene_dirty: bool,
    /// Absolute path to the loaded scene.
    pub scene_path: PathBuf,
}

impl ProjectContext {
    /// Returns a display name for the project.
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// Returns assets sorted by source path.
    pub fn sorted_assets(&self) -> Vec<&ResourceMetaFormat> {
        let mut assets = self.assets.iter().collect::<Vec<_>>();
        assets.sort_by(|left, right| left.source_path.cmp(&right.source_path));
        assets
    }

    /// Rescans the project asset root and updates the project panel data.
    pub fn rescan_assets(&mut self) -> EngineResult<()> {
        let report = scan_project_assets(
            self.root.join(&self.manifest.asset_root),
            &mut self.database,
        )?;
        self.asset_imports.push(format!(
            "scan: {} assets, {} ignored",
            report.metas.len(),
            report.ignored.len()
        ));
        self.assets = report.metas;
        Ok(())
    }

    /// Copies an external file into the asset root, rescans, and runs the built-in importer.
    pub fn import_file(&mut self, source: impl AsRef<Path>) -> EngineResult<()> {
        let source = source.as_ref();
        let file_name = source
            .file_name()
            .ok_or_else(|| EngineError::config("import path must point at a file"))?;
        let asset_root = self.root.join(&self.manifest.asset_root);
        fs::create_dir_all(&asset_root).map_err(|source| EngineError::Filesystem {
            path: asset_root.clone(),
            source,
        })?;
        let destination = asset_root.join(file_name);
        fs::copy(source, &destination).map_err(|source| EngineError::Filesystem {
            path: destination.clone(),
            source,
        })?;
        self.rescan_assets()?;
        let relative = destination
            .strip_prefix(&asset_root)
            .unwrap_or(&destination)
            .to_path_buf();
        let meta = self
            .assets
            .iter()
            .find(|asset| asset.source_path == relative)
            .cloned()
            .ok_or_else(|| EngineError::config("imported file has no matching importer"))?;
        let outcome = import_builtin_asset(
            &asset_root,
            &mut self.registry,
            ImportTask {
                guid: meta.guid,
                source_path: meta.source_path.clone(),
                kind: meta.kind,
                importer: meta.importer.clone(),
            },
        )?;
        for diagnostic in outcome.diagnostics {
            self.asset_imports
                .push(format!("import warning: {}", diagnostic.message));
        }
        self.asset_imports.push(format!(
            "imported {} as {}",
            relative.display(),
            resource_kind_label(meta.kind, &Translations::load(Default::default()))
        ));
        Ok(())
    }
}

/// Formats asset kind for compact UI labels.
pub fn resource_kind_label(kind: ResourceKind, tr: &Translations) -> &str {
    match kind {
        ResourceKind::Texture => tr.tr("resource_texture"),
        ResourceKind::Material => tr.tr("resource_material"),
        ResourceKind::Shader => tr.tr("resource_shader"),
        ResourceKind::Audio => tr.tr("resource_audio"),
        ResourceKind::Model => tr.tr("resource_model"),
        ResourceKind::SkinnedModel => tr.tr("resource_skinned_model"),
        ResourceKind::Animation => tr.tr("resource_animation"),
    }
}

/// Formats an asset GUID as lowercase hex.
pub fn asset_guid_label(guid: AssetGuid) -> String {
    format!("{guid}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_editor::ThemePreference;

    #[test]
    fn hub_starts_on_projects_page_and_filters_cards() {
        let mut hub = HubState::new(EditorPreferences::default());
        hub.upsert_project(ProjectMetadata::new("Demo", "/tmp/demo", "today", "0.1.0"));
        hub.upsert_project(ProjectMetadata::new(
            "Tools",
            "/tmp/tools",
            "today",
            "0.1.0",
        ));

        assert_eq!(hub.page(), HubPage::Projects);

        hub.set_search("demo");

        let projects = hub.filtered_projects();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "Demo");
    }

    #[test]
    fn theme_switch_changes_tokens_without_restarting_state() {
        let mut hub = HubState::new(EditorPreferences::default());
        let dark = hub.design_tokens();

        hub.set_theme(ThemePreference::Light);

        assert_ne!(hub.design_tokens(), dark);
        assert_eq!(hub.preferences().theme, ThemePreference::Light);
    }

    #[test]
    fn new_project_validation_clears_error_after_success() {
        let mut hub = HubState::new(EditorPreferences::default());
        let missing = NewProjectRequest::default();

        assert!(hub.create_project_plan(&missing).is_err());
        assert!(hub.new_project_error().is_some());

        let complete = NewProjectRequest {
            name: "Demo".to_owned(),
            location: Some(PathBuf::from("/tmp")),
            template_id: Some("empty".to_owned()),
            toolchain_version: Some("0.1.0".to_owned()),
        };

        assert!(hub.create_project_plan(&complete).is_ok());
        assert_eq!(hub.new_project_error(), None);
        assert_eq!(
            hub.preferences().last_project_location,
            Some(PathBuf::from("/tmp"))
        );
    }

    #[test]
    fn new_project_files_can_be_opened_by_editor_shell() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("aster-new-project-test-{unique}"));
        let mut hub = HubState::new(EditorPreferences::default());
        let request = NewProjectRequest {
            name: "Demo".to_owned(),
            location: Some(temp_root.clone()),
            template_id: Some("three_d".to_owned()),
            toolchain_version: Some("0.1.0".to_owned()),
        };

        let plan = hub.create_project_plan(&request).unwrap();
        hub.create_project_files(&plan).unwrap();

        let mut shell = EditorShell::with_core_services(EditorPreferences::default());
        shell.open_project(&plan.path).unwrap();

        assert!(plan.path.join("aster.project.toml").is_file());
        assert!(plan.path.join("scenes/main.aster_scene.json").is_file());

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn launch_reports_missing_toolchain_and_accepts_installed_version() {
        let mut hub = HubState::new(EditorPreferences::default());
        let project = ProjectMetadata::new("Demo", "/tmp/demo", "today", "0.1.0");

        let error = hub.launch_editor_action(&project).unwrap_err().to_string();
        assert!(error.contains("not installed"));

        hub.add_install(ToolchainInstall::new("0.1.0", "/opt/aster"));

        assert_eq!(
            hub.launch_editor_action(&project).unwrap(),
            HubAction::LaunchEditor {
                project_path: PathBuf::from("/tmp/demo"),
                toolchain_version: "0.1.0".to_owned(),
            }
        );
    }

    #[test]
    fn deletion_requires_confirmation_and_refuses_open_projects() {
        let mut hub = HubState::new(EditorPreferences::default());
        let path = Path::new("/tmp/demo");
        hub.upsert_project(ProjectMetadata::new("Demo", path, "today", "0.1.0"));

        assert_eq!(
            hub.request_project_deletion(path, ProjectDeletionMode::RemoveRecent, false),
            ProjectDeletionDecision::NeedsConfirmation {
                path: path.to_path_buf(),
                mode: ProjectDeletionMode::RemoveRecent,
            }
        );

        hub.mark_project_open(path);

        assert_eq!(
            hub.request_project_deletion(path, ProjectDeletionMode::RemoveRecent, true),
            ProjectDeletionDecision::RefusedOpenProject {
                path: path.to_path_buf(),
            }
        );
    }

    #[test]
    fn editor_shell_opens_with_required_core_panels() {
        let shell = EditorShell::with_core_services(EditorPreferences::default());

        for id in [
            "hierarchy",
            "inspector",
            "project",
            "console",
            "scene_view",
            "game_view",
        ] {
            assert!(shell.panels().get(id).is_some(), "missing panel {id}");
        }
        for id in [
            "play.toggle",
            "play.pause",
            "play.stop",
            "edit.undo",
            "edit.redo",
            "assets.reload",
            "scene.save",
            "project.build",
        ] {
            assert!(shell.commands().get(id).is_some(), "missing command {id}");
        }
    }

    #[test]
    fn editor_shell_records_undo_commands() {
        let mut shell = EditorShell::with_core_services(EditorPreferences::default());
        shell.push_undo(UndoCommand::new("Rename", "entity:1", "A", "B"));

        assert!(shell.undo_stack().can_undo());
        assert_eq!(shell.pop_undo().unwrap().after, "B");
        assert!(shell.undo_stack().can_redo());
    }
}
