#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Editor UI state types shared by host frontends.
//!
//! These types describe the Hub (project picker) and Editor Shell (active editor)
//! state. They contain no rendering code — just data structures and methods
//! for managing projects, scenes, selections, undo history, and asset browsing.

use std::{
    fs,
    path::{Path, PathBuf},
};

use engine_assets::{
    import_builtin_asset, scan_project_assets, AssetDatabase, AssetRegistry, ImportTask,
    ResourceKind, ResourceMetaFormat,
};
use engine_core::{
    math::{Transform, Vec3},
    EngineError, EngineResult, EntityId,
};
use engine_ecs::{
    CameraComponentData, CameraRole, ComponentData, LightComponentData, MeshRendererComponentData,
    ProjectManifest, Scene,
};
use engine_i18n::Translations;
use engine_render::{RenderTargetDesc, RenderWorld};

use crate::register_core_commands;
use crate::register_core_panels;
use crate::{
    CommandRegistry, ConsoleEntry, ConsoleLevel, ConsoleService, ConsoleSource, DurableEditorState,
    EditorPreferences, MemoryProjectStore, NewProjectRequest, PanelRegistry, ProjectCreationPlan,
    ProjectMetadata, ProjectStore, Selection, SelectionService, ThemePreference, ToolchainInstall,
    UndoCommand, UndoRedoStack,
};

// ─── Design Tokens ───────────────────────────────────────────────────────────

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

// ─── Hub Types ───────────────────────────────────────────────────────────────

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

// ─── Hub State ───────────────────────────────────────────────────────────────

/// First Hub state model.
#[derive(Clone, Debug)]
pub struct HubState {
    page: HubPage,
    /// Current search/filter text for the Projects page.
    pub search: String,
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
        match crate::validate_new_project(request) {
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

// ─── Project Context ─────────────────────────────────────────────────────────

/// Open editor project data bound to shell panels and agent tools.
///
/// Combines the parsed manifest, editable scene graph, asset database,
/// resource registry, and scan results into a single context object
/// shared by the editor shell, AI agent tools, and CLI commands.
#[derive(Debug)]
pub struct ProjectContext {
    /// Project root path.
    pub root: PathBuf,
    /// Parsed project manifest from `aster.project.toml`.
    pub manifest: ProjectManifest,
    /// Editable scene graph loaded from the default scene file.
    pub scene: Scene,
    /// Asset database for GUID/path resolution and dependency tracking.
    pub database: AssetDatabase,
    /// CPU/GPU resource registry used by import and preview workflows.
    pub registry: AssetRegistry,
    /// Last asset scan results shown by the Project panel.
    pub assets: Vec<ResourceMetaFormat>,
    /// Recent import/rescan status messages.
    pub asset_imports: Vec<String>,
    /// Whether the scene has unsaved edits.
    pub scene_dirty: bool,
    /// Absolute path to the loaded scene file.
    pub scene_path: PathBuf,
}

impl ProjectContext {
    /// Opens a project from `project_root` by loading its manifest, default scene, and asset root.
    pub fn open(project_root: impl Into<PathBuf>) -> EngineResult<Self> {
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
        database.scan(&project_root.join(&manifest.asset_root))?;
        Ok(Self {
            root: project_root,
            manifest,
            scene,
            database,
            registry: AssetRegistry::default(),
            assets: scan.metas,
            asset_imports: Vec::new(),
            scene_dirty: false,
            scene_path,
        })
    }

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
        self.database
            .scan(&self.root.join(&self.manifest.asset_root))?;
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

    /// Serializes the project state into an AI-consumable context.
    ///
    /// Produces a compact JSON representation of the scene graph, components,
    /// transforms, and asset list suitable as LLM prompt context.
    pub fn to_ai_context(&self) -> serde_json::Value {
        serde_json::json!({
            "project": {
                "name": self.manifest.name,
                "default_scene": self.manifest.default_scene,
            },
            "scene": self.scene_to_ai_context(),
            "assets": self.assets_to_ai_context(),
        })
    }

    fn scene_to_ai_context(&self) -> serde_json::Value {
        let objects: Vec<serde_json::Value> = self
            .scene
            .objects()
            .iter()
            .map(|(entity, obj)| {
                let transform = self.scene.transforms().local(*entity);
                let parent = self.scene.transforms().parent(*entity);
                serde_json::json!({
                    "id": format!("{}:{}",
                        entity.handle().slot(),
                        entity.handle().generation().get()
                    ),
                    "name": obj.name,
                    "tag": obj.tag,
                    "layer": obj.layer,
                    "active": obj.active,
                    "parent": parent.map(|p| format!("{}:{}",
                        p.handle().slot(),
                        p.handle().generation().get()
                    )),
                    "transform": transform.map(|t| serde_json::json!({
                        "position": [t.translation.x, t.translation.y, t.translation.z],
                        "rotation": [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
                        "scale": [t.scale.x, t.scale.y, t.scale.z],
                    })),
                    "components": obj.components.iter()
                        .map(|c| Self::component_to_ai_context(c))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();

        serde_json::json!({ "objects": objects })
    }

    fn component_to_ai_context(component: &engine_ecs::ComponentData) -> serde_json::Value {
        use engine_ecs::ComponentData;
        match component {
            ComponentData::Camera(c) => serde_json::json!({
                "type": "Camera",
                "fov": c.vertical_fov_degrees,
                "near": c.near,
                "far": c.far,
                "primary": c.primary,
            }),
            ComponentData::MeshRenderer(m) => serde_json::json!({
                "type": "MeshRenderer",
                "mesh": m.mesh.map(|id| id.as_u128().to_string()),
                "builtin_mesh": m.builtin_mesh,
                "casts_shadows": m.casts_shadows,
                "receive_shadows": m.receive_shadows,
            }),
            ComponentData::Light(l) => serde_json::json!({
                "type": "Light",
                "kind": l.kind,
                "color": [l.color.x, l.color.y, l.color.z],
                "intensity": l.intensity,
                "range": l.range,
                "spot_angle": l.spot_angle,
            }),
            ComponentData::Rigidbody(r) => serde_json::json!({
                "type": "Rigidbody",
                "body_type": r.body_type,
                "mass": r.mass,
                "use_gravity": r.use_gravity,
                "linear_damping": r.linear_damping,
                "angular_damping": r.angular_damping,
            }),
            ComponentData::Collider(c) => serde_json::json!({
                "type": "Collider",
                "shape": c.shape,
                "size": [c.size.x, c.size.y, c.size.z],
                "is_trigger": c.is_trigger,
            }),
            ComponentData::Script(s) => serde_json::json!({
                "type": "Script",
                "backend": s.backend,
                "script": s.script,
            }),
            ComponentData::AudioSource(a) => serde_json::json!({
                "type": "AudioSource",
                "clip": a.clip.map(|id| id.as_u128().to_string()),
                "volume": a.volume,
                "looping": a.looping,
                "play_on_start": a.play_on_start,
            }),
            ComponentData::ParticleEmitter(p) => serde_json::json!({
                "type": "ParticleEmitter",
                "max_particles": p.max_particles,
                "emission_rate": p.emission_rate,
            }),
            ComponentData::Sprite2D(s) => serde_json::json!({
                "type": "Sprite2D",
                "texture": s.texture.map(|id| id.as_u128().to_string()),
            }),
            ComponentData::Camera2D(c) => serde_json::json!({
                "type": "Camera2D",
                "zoom": c.zoom,
            }),
            ComponentData::AnimationPlayer(_a) => serde_json::json!({
                "type": "AnimationPlayer",
            }),
            _ => serde_json::json!({ "type": component.type_id() }),
        }
    }

    fn assets_to_ai_context(&self) -> serde_json::Value {
        let items: Vec<serde_json::Value> = self
            .database
            .iter_entries()
            .map(|entry| {
                serde_json::json!({
                    "guid": entry.guid.to_string(),
                    "path": entry.path.to_string_lossy(),
                    "kind": format!("{:?}", entry.kind),
                })
            })
            .collect();
        serde_json::json!({
            "count": items.len(),
            "items": items,
        })
    }
}

// ─── Editor Shell Types ──────────────────────────────────────────────────────

/// Viewport projection mode used by the editor Scene View camera.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditorSceneViewProjection {
    /// Perspective 3D view.
    #[default]
    Perspective,
    /// Orthographic editor view.
    Orthographic,
}

/// Named editor Scene View orientation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditorSceneViewOrientation {
    /// Free orbit view.
    #[default]
    Free,
    /// Top view looking down the Y axis.
    Top,
    /// Bottom view looking up the Y axis.
    Bottom,
    /// Left view.
    Left,
    /// Right view.
    Right,
    /// Front view.
    Front,
    /// Rear view.
    Rear,
}

/// Transform gizmo coordinate space used by editor tools.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditorTransformSpace {
    /// Align transform handles to world axes.
    #[default]
    Global,
    /// Align transform handles to the selected object's local axes.
    Local,
}

/// Active transform tool in the editor viewport.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditorTransformTool {
    /// Inspect/orbit the view without showing a transform gizmo.
    View,
    /// Translate the selected object.
    #[default]
    Move,
    /// Rotate the selected object.
    Rotate,
    /// Scale the selected object.
    Scale,
}

/// Active viewport transform drag operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewportTransformDragMode {
    /// Translate along one world-space gizmo axis.
    MoveAxis {
        /// World-space axis used by the active handle.
        axis: Vec3,
    },
    /// Translate within the plane spanned by two world-space gizmo axes.
    MovePlane {
        /// First world-space axis used by the active plane handle.
        axis_a: Vec3,
        /// Second world-space axis used by the active plane handle.
        axis_b: Vec3,
    },
    /// Transform from raw screen-space movement.
    Screen,
    /// Rotate around a world-space axis.
    RotateAxis {
        /// World-space rotation axis.
        axis: Vec3,
    },
}

/// State captured at the beginning of a viewport transform drag.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewportTransformDragState {
    /// Entity being transformed.
    pub selected_id: EntityId,
    /// Undo snapshot captured before the drag began.
    pub before_scene: String,
    /// Transform captured before the drag began.
    pub start_transform: Transform,
    /// Pointer position captured before the drag began.
    pub start_pointer: [f32; 2],
    /// World-space hit point captured before the drag began.
    pub start_hit: Option<Vec3>,
    /// Drag operation captured before the drag began.
    pub mode: ViewportTransformDragMode,
    /// Previous angle (radians) for rotation drag delta computation.
    pub rotate_prev_angle: f32,
}

/// Snap settings for editor transform gizmos.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorSnapSettings {
    /// Grid snap increment for move tool (None = disabled).
    pub move_snap: Option<f32>,
    /// Angle snap increment in degrees for rotate tool (None = disabled).
    pub angle_snap: Option<f32>,
    /// Scale snap increment for scale tool (None = disabled).
    pub scale_snap: Option<f32>,
}

impl Default for EditorSnapSettings {
    fn default() -> Self {
        Self {
            move_snap: None,
            angle_snap: None,
            scale_snap: None,
        }
    }
}

/// Info about a wgpu-rendered texture ready for display in a viewport.
#[derive(Clone, Debug)]
pub struct ViewportTexture {
    /// Texture ID.
    pub id: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Script template backend used when creating new script assets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScriptTemplateBackend {
    /// Python script using the runtime-min subprocess context API.
    #[default]
    Python,
    /// Rhai script using the engine-script-rhai lifecycle API.
    Rhai,
}

impl ScriptTemplateBackend {
    /// Returns the file extension for this backend.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::Rhai => "rhai",
        }
    }

    /// Returns the Script component backend identifier.
    pub const fn component_backend(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Rhai => "rhai",
        }
    }
}

/// Transient state for the in-editor script source editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptEditorState {
    /// Project-relative asset path under the asset root.
    pub relative_path: PathBuf,
    /// Editable script source text.
    pub source: String,
    /// Whether the script source differs from the last loaded or saved version.
    pub dirty: bool,
    /// Last save/load status for the editor window.
    pub status: Option<String>,
}

/// Play Mode command requested by editor UI and executed by the native host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayModeRequest {
    /// Clone the edit scene and start ticking runtime services.
    Enter,
    /// Update runtime pause state.
    Pause(bool),
    /// Tick one frame while paused.
    Step,
    /// Stop ticking runtime services and restore the edit scene.
    Stop,
}

/// Action the editor shell requests from the native host (file dialogs, navigation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorAction {
    /// Open a native file dialog to load a scene.
    OpenScene,
    /// Open a native Save As file dialog.
    SaveAs,
    /// Navigate back to the Hub screen.
    ReturnToHub,
    /// Close the application window (user confirmed discard/save on window close).
    CloseWindow,
}

/// UI-side render target request produced by Scene View and Game View panels.
#[derive(Clone, Debug)]
pub struct ViewportTargetState {
    /// Target descriptor to allocate in the renderer backend.
    pub desc: RenderTargetDesc,
    /// Render-world data extracted for this view.
    pub world: RenderWorld,
}

// ─── Editor Shell ────────────────────────────────────────────────────────────

/// First native editor shell state.
///
/// Owns the panel/command registries, selection, console, undo stack,
/// and optional project context. Created by the native host (Tauri/Electron)
/// and shared with the UI layer.
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
        let project_ctx = ProjectContext::open(&project_root).map_err(|error| {
            EngineError::config(format!(
                "failed to open project {}: {error}",
                project_root.display()
            ))
        })?;
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
        self.project = Some(project_ctx);
        Ok(())
    }

    /// Saves the active scene to the project's default scene path.
    ///
    /// Creates a `.bak` backup of the previous file, then writes atomically
    /// via a `.tmp` file and `fs::rename`.
    pub fn save_scene(&mut self) -> EngineResult<String> {
        let Some(project) = self.project.as_mut() else {
            return Err(EngineError::config("no project is open"));
        };
        let scene_name = project
            .scene_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Scene");
        let json = project.scene.to_json(scene_name)?;
        Self::write_scene_atomic(&project.scene_path.clone(), &json)?;
        project.scene_dirty = false;
        Ok(project.scene_path.display().to_string())
    }

    /// Saves the active scene to a new path.
    ///
    /// After Save As, `ProjectManifest.default_scene` is NOT automatically updated.
    /// The user must explicitly set the default scene via the project settings.
    pub fn save_scene_as(&mut self, path: &Path) -> EngineResult<String> {
        let Some(project) = self.project.as_mut() else {
            return Err(EngineError::config("no project is open"));
        };
        let scene_name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Scene");
        let json = project.scene.to_json(scene_name)?;
        Self::write_scene_atomic(path, &json)?;
        project.scene_path = path.to_path_buf();
        project.scene_dirty = false;
        Ok(path.display().to_string())
    }

    /// Writes scene JSON atomically: `.tmp` file write → rename, with `.bak` backup.
    fn write_scene_atomic(target: &Path, json: &str) -> EngineResult<()> {
        if target.exists() {
            let bak_path = target.with_extension("json.bak");
            let _ = fs::remove_file(&bak_path);
            let _ = fs::copy(target, &bak_path);
        }
        let tmp_path = target.with_extension("json.tmp");
        fs::write(&tmp_path, json).map_err(|source| EngineError::Filesystem {
            path: tmp_path.clone(),
            source,
        })?;
        fs::rename(&tmp_path, target).map_err(|source| EngineError::Filesystem {
            path: target.to_path_buf(),
            source,
        })
    }

    /// Closes the current project and clears editor state.
    pub fn close_project(&mut self) {
        self.project = None;
        self.selection.clear();
    }

    /// Loads a scene from the given path, replacing the current scene.
    ///
    /// The Hierarchy and Inspector will refresh on the next frame because they
    /// read from `project.scene` directly. Selection is cleared.
    pub fn load_scene(&mut self, path: &Path) -> EngineResult<String> {
        let Some(project) = self.project.as_mut() else {
            return Err(EngineError::config("no project is open"));
        };
        let scene_text = fs::read_to_string(path).map_err(|source| EngineError::Filesystem {
            path: path.to_path_buf(),
            source,
        })?;
        project.scene = Scene::from_json(&scene_text)?;
        project.scene_path = path.to_path_buf();
        project.scene_dirty = false;
        self.selection.clear();
        Ok(path.display().to_string())
    }

    /// Returns whether the current scene has unsaved changes.
    pub fn is_scene_dirty(&self) -> bool {
        self.project
            .as_ref()
            .map(|p| p.scene_dirty)
            .unwrap_or(false)
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
    pub fn select_entity_id(&mut self, id: EntityId) {
        self.selection
            .select(Selection::Entity(format!("{:032x}", id.as_u128())));
    }

    /// Returns the selected scene object ID, if the selection is an entity.
    pub fn selected_entity_id(&self) -> Option<EntityId> {
        let Selection::Entity(value) = self.selection.selected()? else {
            return None;
        };
        u128::from_str_radix(value, 16)
            .ok()
            .map(EntityId::from_u128)
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

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Formats asset kind for compact UI labels.
pub fn resource_kind_label(kind: ResourceKind, tr: &Translations) -> &str {
    match kind {
        ResourceKind::Texture => tr.tr("resource_texture"),
        ResourceKind::Material => tr.tr("resource_material"),
        ResourceKind::Shader => tr.tr("resource_shader"),
        ResourceKind::Audio => tr.tr("resource_audio_clip"),
        ResourceKind::Model => tr.tr("resource_model"),
        ResourceKind::SkinnedModel => tr.tr("resource_skinned_model"),
        ResourceKind::Scene => tr.tr("resource_scene"),
        ResourceKind::Script => tr.tr("resource_script"),
        ResourceKind::Animation => tr.tr("resource_animation"),
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
