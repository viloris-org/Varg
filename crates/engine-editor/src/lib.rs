#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Editor-facing services shared by the Hub, native editor shell, CLI, and agent tools.

use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
};

use engine_core::{EngineError, EngineResult};
use engine_i18n::Locale;
use serde::{Deserialize, Serialize};

#[cfg(feature = "agent-tools")]
pub mod agent;

pub mod native;
pub mod render;
pub use native::{
    GizmoOperation, GizmoService, GizmoSpace, OutlineEntry, OutlineService, PickRequest,
    PickResult, PickingService, PreviewKind, PreviewRequest, PreviewService, PreviewState,
};

/// Theme preference used by Hub and editor shells.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follow the host operating system theme.
    #[default]
    System,
    /// Force the dark tool palette.
    Dark,
    /// Force the light tool palette.
    Light,
}

/// Durable editor preferences.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct EditorPreferences {
    /// Selected theme.
    #[serde(default)]
    pub theme: ThemePreference,
    /// UI locale.
    #[serde(default)]
    pub locale: Locale,
    /// Reopen the last project on editor startup when possible.
    #[serde(default = "default_reopen_last_project")]
    pub reopen_last_project: bool,
    /// Last directory used by the new-project dialog.
    #[serde(default)]
    pub last_project_location: Option<PathBuf>,
    /// Serialized layout identifier for the current editor shell.
    #[serde(default = "default_layout")]
    pub layout: String,
}

impl Default for EditorPreferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            locale: Locale::default(),
            reopen_last_project: true,
            last_project_location: None,
            layout: default_layout(),
        }
    }
}

fn default_reopen_last_project() -> bool {
    true
}

fn default_layout() -> String {
    "default".to_owned()
}

/// Reads preferences from TOML.
pub fn read_preferences_toml(input: &str) -> EngineResult<EditorPreferences> {
    toml::from_str(input).map_err(|error| EngineError::config(error.to_string()))
}

/// Writes preferences as TOML.
pub fn write_preferences_toml(preferences: &EditorPreferences) -> EngineResult<String> {
    toml::to_string_pretty(preferences).map_err(|error| EngineError::other(error.to_string()))
}

/// Engine or toolchain version available to the Hub.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolchainInstall {
    /// Stable version label.
    pub version: String,
    /// Local install location.
    pub path: PathBuf,
    /// Whether this install can launch the editor.
    pub editor_available: bool,
    /// Whether this install can launch runtime builds.
    pub runtime_available: bool,
}

impl ToolchainInstall {
    /// Creates an install record.
    pub fn new(version: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            version: version.into(),
            path: path.into(),
            editor_available: true,
            runtime_available: true,
        }
    }
}

/// Structured progress for install, import, build, package, and launch tasks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundProgress {
    /// Stable task name.
    pub task: String,
    /// Completed work units.
    pub completed: u64,
    /// Total work units when known.
    pub total: Option<u64>,
    /// Current status text.
    pub message: String,
    /// Diagnostic lines captured from the operation.
    pub diagnostics: Vec<String>,
}

impl BackgroundProgress {
    /// Creates an indeterminate progress event.
    pub fn indeterminate(task: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            completed: 0,
            total: None,
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }
}

/// Project template available from the Hub.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectTemplate {
    /// Template identifier.
    pub id: String,
    /// Human-readable template name.
    pub name: String,
}

/// Recent project metadata shared by CLI and Hub.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProjectMetadata {
    /// Project display name.
    pub name: String,
    /// Project root path.
    pub path: PathBuf,
    /// Last opened or created date, formatted by the producer.
    pub last_touched: String,
    /// Toolchain version selected by the project.
    pub toolchain_version: String,
}

impl ProjectMetadata {
    /// Creates project metadata.
    pub fn new(
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        last_touched: impl Into<String>,
        toolchain_version: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            last_touched: last_touched.into(),
            toolchain_version: toolchain_version.into(),
        }
    }
}

/// Service boundary for recent projects.
pub trait ProjectStore {
    /// Lists known recent projects.
    fn projects(&self) -> &[ProjectMetadata];
    /// Adds or updates a recent project.
    fn upsert_project(&mut self, project: ProjectMetadata);
    /// Removes a project from the recent list without deleting files.
    fn remove_recent(&mut self, path: &Path) -> bool;
}

/// In-memory project store useful for tests, CLI commands, and the first UI shell.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MemoryProjectStore {
    projects: Vec<ProjectMetadata>,
}

impl MemoryProjectStore {
    /// Creates an empty project store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectStore for MemoryProjectStore {
    fn projects(&self) -> &[ProjectMetadata] {
        &self.projects
    }

    fn upsert_project(&mut self, project: ProjectMetadata) {
        self.remove_recent(&project.path);
        self.projects.insert(0, project);
    }

    fn remove_recent(&mut self, path: &Path) -> bool {
        let before = self.projects.len();
        self.projects.retain(|project| project.path != path);
        before != self.projects.len()
    }
}

/// Durable editor state stored by native hosts between editor launches.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct DurableEditorState {
    /// Durable editor preferences.
    #[serde(default)]
    pub preferences: EditorPreferences,
    /// Recent projects shown by the Hub.
    #[serde(default)]
    pub recent_projects: Vec<ProjectMetadata>,
    /// Last successfully opened project.
    #[serde(default)]
    pub last_open_project: Option<PathBuf>,
    /// Serialized editor layout identifier.
    #[serde(default)]
    pub layout: String,
}

impl DurableEditorState {
    /// Creates state from preferences and a project store.
    pub fn from_parts(
        preferences: EditorPreferences,
        project_store: &impl ProjectStore,
        last_open_project: Option<PathBuf>,
    ) -> Self {
        Self {
            layout: preferences.layout.clone(),
            preferences,
            recent_projects: project_store.projects().to_vec(),
            last_open_project,
        }
    }
}

/// File-backed store for editor preferences, recent projects, and layout metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileEditorStore {
    path: PathBuf,
}

impl FileEditorStore {
    /// Creates a store bound to a specific TOML file path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the backing file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads durable state. Missing files produce default state.
    pub fn load(&self) -> EngineResult<DurableEditorState> {
        if !self.path.exists() {
            return Ok(DurableEditorState::default());
        }
        let input = fs::read_to_string(&self.path).map_err(|source| EngineError::Filesystem {
            path: self.path.clone(),
            source,
        })?;
        toml::from_str(&input).map_err(|error| EngineError::config(error.to_string()))
    }

    /// Saves durable state, creating the parent directory when needed.
    pub fn save(&self, state: &DurableEditorState) -> EngineResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| EngineError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let output =
            toml::to_string_pretty(state).map_err(|error| EngineError::other(error.to_string()))?;
        fs::write(&self.path, output).map_err(|source| EngineError::Filesystem {
            path: self.path.clone(),
            source,
        })
    }
}

/// Input gathered by the new-project dialog.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NewProjectRequest {
    /// Project name.
    pub name: String,
    /// Parent directory.
    pub location: Option<PathBuf>,
    /// Selected template id.
    pub template_id: Option<String>,
    /// Selected toolchain version.
    pub toolchain_version: Option<String>,
}

/// Validated project creation plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectCreationPlan {
    /// Project name.
    pub name: String,
    /// Project root path.
    pub path: PathBuf,
    /// Template id.
    pub template_id: String,
    /// Toolchain version.
    pub toolchain_version: String,
}

/// Validates Hub project creation input.
pub fn validate_new_project(request: &NewProjectRequest) -> EngineResult<ProjectCreationPlan> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err(EngineError::config("project name is required"));
    }
    if name.contains(['/', '\\']) {
        return Err(EngineError::config(
            "project name cannot contain path separators",
        ));
    }
    let location = request
        .location
        .clone()
        .ok_or_else(|| EngineError::config("project location is required"))?;
    let template_id = request
        .template_id
        .clone()
        .ok_or_else(|| EngineError::config("project template is required"))?;
    let toolchain_version = request
        .toolchain_version
        .clone()
        .ok_or_else(|| EngineError::config("engine/toolchain version is required"))?;

    Ok(ProjectCreationPlan {
        name: name.to_owned(),
        path: location.join(name),
        template_id,
        toolchain_version,
    })
}

/// Global selection shared by editor panels.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionService {
    selected: Option<Selection>,
}

impl SelectionService {
    /// Selects a target.
    pub fn select(&mut self, selection: Selection) {
        self.selected = Some(selection);
    }

    /// Clears the current selection.
    pub fn clear(&mut self) {
        self.selected = None;
    }

    /// Returns the current selection.
    pub fn selected(&self) -> Option<&Selection> {
        self.selected.as_ref()
    }
}

/// Target selected in the editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    /// Scene entity path or stable id.
    Entity(String),
    /// Asset path.
    Asset(PathBuf),
    /// Editor panel id.
    Panel(String),
}

/// Editor command metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorCommand {
    /// Stable command id.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Top-level menu or palette category.
    pub category: String,
    /// Optional keyboard shortcut display text.
    pub shortcut: Option<String>,
    /// Rule used by shells to decide whether the command is currently usable.
    pub availability: CommandAvailability,
}

/// Runtime availability rule for an editor command.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommandAvailability {
    /// Command is always available.
    #[default]
    Always,
    /// Command requires an open project.
    ProjectOpen,
    /// Command requires an open project with unsaved scene changes.
    DirtyScene,
    /// Command requires undo history.
    CanUndo,
    /// Command requires redo history.
    CanRedo,
    /// Command requires play mode.
    Playing,
    /// Command requires play mode to be stopped.
    NotPlaying,
}

/// Reversible editor operation captured by the command stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndoCommand {
    /// Display label shown in history menus.
    pub label: String,
    /// Stable target identifier, such as an entity id or asset path.
    pub target: String,
    /// Serialized state before the command.
    pub before: String,
    /// Serialized state after the command.
    pub after: String,
}

impl UndoCommand {
    /// Creates a reversible command record.
    pub fn new(
        label: impl Into<String>,
        target: impl Into<String>,
        before: impl Into<String>,
        after: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            target: target.into(),
            before: before.into(),
            after: after.into(),
        }
    }
}

/// Linear undo/redo stack used by editor tools and panels.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UndoRedoStack {
    undo: Vec<UndoCommand>,
    redo: Vec<UndoCommand>,
}

impl UndoRedoStack {
    /// Pushes a completed command and clears redo history.
    pub fn push(&mut self, command: UndoCommand) {
        self.undo.push(command);
        self.redo.clear();
    }

    /// Pops the most recent undo command and moves it to redo history.
    pub fn undo(&mut self) -> Option<UndoCommand> {
        let command = self.undo.pop()?;
        self.redo.push(command.clone());
        Some(command)
    }

    /// Pops the most recent redo command and moves it back to undo history.
    pub fn redo(&mut self) -> Option<UndoCommand> {
        let command = self.redo.pop()?;
        self.undo.push(command.clone());
        Some(command)
    }

    /// Returns whether an undo command is available.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Returns whether a redo command is available.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// Registry for menu, toolbar, and command-palette commands.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandRegistry {
    commands: BTreeMap<String, EditorCommand>,
}

impl CommandRegistry {
    /// Registers or replaces a command.
    pub fn register(&mut self, command: EditorCommand) {
        self.commands.insert(command.id.clone(), command);
    }

    /// Looks up a command.
    pub fn get(&self, id: &str) -> Option<&EditorCommand> {
        self.commands.get(id)
    }

    /// Lists commands in stable id order.
    pub fn commands(&self) -> impl Iterator<Item = &EditorCommand> {
        self.commands.values()
    }
}

/// Editor panel metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorPanel {
    /// Stable panel id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Default dock region.
    pub default_region: DockRegion,
}

/// Default dock region for a panel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DockRegion {
    /// Left dock.
    Left,
    /// Right dock.
    Right,
    /// Bottom dock.
    Bottom,
    /// Center document area.
    Center,
}

/// Registry that allows editor modules to expose panels without owning the shell.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PanelRegistry {
    panels: BTreeMap<String, EditorPanel>,
}

impl PanelRegistry {
    /// Registers or replaces a panel.
    pub fn register(&mut self, panel: EditorPanel) {
        self.panels.insert(panel.id.clone(), panel);
    }

    /// Looks up a panel by id.
    pub fn get(&self, id: &str) -> Option<&EditorPanel> {
        self.panels.get(id)
    }

    /// Lists registered panels in stable id order.
    pub fn panels(&self) -> impl Iterator<Item = &EditorPanel> {
        self.panels.values()
    }
}

/// Console message severity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConsoleLevel {
    /// Trace detail.
    Trace,
    /// Debug detail.
    Debug,
    /// Informational message.
    Info,
    /// Warning.
    Warn,
    /// Error.
    Error,
}

/// Console message source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleSource {
    /// Source subsystem.
    pub subsystem: String,
    /// Optional file path.
    pub file: Option<PathBuf>,
    /// Optional line number.
    pub line: Option<u32>,
}

/// Console entry captured for editor display.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleEntry {
    /// Time text supplied by the logger or shell.
    pub timestamp: String,
    /// Message severity.
    pub level: ConsoleLevel,
    /// Message source.
    pub source: ConsoleSource,
    /// Message body.
    pub message: String,
}

/// Filter for console display.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConsoleFilter {
    /// Minimum level to show.
    pub min_level: Option<ConsoleLevel>,
    /// Source subsystem substring.
    pub source_contains: Option<String>,
    /// Message substring.
    pub message_contains: Option<String>,
}

/// Console service with filtering, copy, clear, and source location metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConsoleService {
    entries: VecDeque<ConsoleEntry>,
}

impl ConsoleService {
    /// Adds an entry.
    pub fn push(&mut self, entry: ConsoleEntry) {
        self.entries.push_back(entry);
    }

    /// Clears all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns all entries.
    pub fn entries(&self) -> &VecDeque<ConsoleEntry> {
        &self.entries
    }

    /// Returns entries matching a filter.
    pub fn filtered(&self, filter: &ConsoleFilter) -> Vec<&ConsoleEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                filter
                    .min_level
                    .map(|level| entry.level >= level)
                    .unwrap_or(true)
            })
            .filter(|entry| {
                filter
                    .source_contains
                    .as_ref()
                    .map(|text| entry.source.subsystem.contains(text))
                    .unwrap_or(true)
            })
            .filter(|entry| {
                filter
                    .message_contains
                    .as_ref()
                    .map(|text| entry.message.contains(text))
                    .unwrap_or(true)
            })
            .collect()
    }
}

/// Registers the required first editor panels.
pub fn register_core_panels(registry: &mut PanelRegistry) {
    for panel in [
        EditorPanel {
            id: "hierarchy".to_owned(),
            title: "Hierarchy".to_owned(),
            default_region: DockRegion::Left,
        },
        EditorPanel {
            id: "inspector".to_owned(),
            title: "Inspector".to_owned(),
            default_region: DockRegion::Right,
        },
        EditorPanel {
            id: "project".to_owned(),
            title: "Project".to_owned(),
            default_region: DockRegion::Bottom,
        },
        EditorPanel {
            id: "console".to_owned(),
            title: "Console".to_owned(),
            default_region: DockRegion::Bottom,
        },
        EditorPanel {
            id: "scene_view".to_owned(),
            title: "Scene View".to_owned(),
            default_region: DockRegion::Center,
        },
        EditorPanel {
            id: "game_view".to_owned(),
            title: "Game View".to_owned(),
            default_region: DockRegion::Center,
        },
        EditorPanel {
            id: "assets".to_owned(),
            title: "Assets".to_owned(),
            default_region: DockRegion::Bottom,
        },
        EditorPanel {
            id: "performance".to_owned(),
            title: "Performance".to_owned(),
            default_region: DockRegion::Bottom,
        },
    ] {
        registry.register(panel);
    }
}

/// Registers the first editor toolbar and menu commands.
pub fn register_core_commands(registry: &mut CommandRegistry) {
    for command in [
        command(
            "project.open",
            "Open Project",
            "File",
            Some("Ctrl+O"),
            CommandAvailability::Always,
        ),
        command(
            "scene.save",
            "Save",
            "File",
            Some("Ctrl+S"),
            CommandAvailability::DirtyScene,
        ),
        command(
            "project.build",
            "Build",
            "File",
            Some("Ctrl+B"),
            CommandAvailability::ProjectOpen,
        ),
        command(
            "edit.undo",
            "Undo",
            "Edit",
            Some("Ctrl+Z"),
            CommandAvailability::CanUndo,
        ),
        command(
            "edit.redo",
            "Redo",
            "Edit",
            Some("Ctrl+Y"),
            CommandAvailability::CanRedo,
        ),
        command(
            "play.toggle",
            "Play",
            "Play",
            Some("Ctrl+P"),
            CommandAvailability::Always,
        ),
        command(
            "play.pause",
            "Pause",
            "Play",
            Some("Ctrl+Shift+P"),
            CommandAvailability::Playing,
        ),
        command(
            "play.step",
            "Step",
            "Play",
            Some("F10"),
            CommandAvailability::Playing,
        ),
        command(
            "play.stop",
            "Stop",
            "Play",
            Some("Shift+F5"),
            CommandAvailability::Playing,
        ),
        command(
            "assets.reload",
            "Reload Assets",
            "Assets",
            None,
            CommandAvailability::ProjectOpen,
        ),
        command(
            "layout.reset",
            "Reset Layout",
            "Window",
            None,
            CommandAvailability::Always,
        ),
        command(
            "command.palette",
            "Command Palette",
            "Window",
            Some("Ctrl+Shift+K"),
            CommandAvailability::Always,
        ),
    ] {
        registry.register(command);
    }
}

fn command(
    id: &str,
    label: &str,
    category: &str,
    shortcut: Option<&str>,
    availability: CommandAvailability,
) -> EditorCommand {
    EditorCommand {
        id: id.to_owned(),
        label: label.to_owned(),
        category: category.to_owned(),
        shortcut: shortcut.map(str::to_owned),
        availability,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_project_creation_and_remembers_root_path() {
        let request = NewProjectRequest {
            name: "Space Demo".to_owned(),
            location: Some(PathBuf::from("/tmp/aster")),
            template_id: Some("empty".to_owned()),
            toolchain_version: Some("0.1.0".to_owned()),
        };

        let plan = validate_new_project(&request).unwrap();

        assert_eq!(plan.path, PathBuf::from("/tmp/aster/Space Demo"));
    }

    #[test]
    fn reports_missing_toolchain_as_actionable_configuration_error() {
        let request = NewProjectRequest {
            name: "Demo".to_owned(),
            location: Some(PathBuf::from("/tmp")),
            template_id: Some("empty".to_owned()),
            toolchain_version: None,
        };

        let error = validate_new_project(&request).unwrap_err().to_string();

        assert!(error.contains("engine/toolchain version is required"));
    }

    #[test]
    fn project_store_upserts_and_removes_recent_entries() {
        let mut store = MemoryProjectStore::new();
        store.upsert_project(ProjectMetadata::new("Old", "/tmp/demo", "today", "0.1.0"));
        store.upsert_project(ProjectMetadata::new("New", "/tmp/demo", "later", "0.1.0"));

        assert_eq!(store.projects().len(), 1);
        assert_eq!(store.projects()[0].name, "New");
        assert!(store.remove_recent(Path::new("/tmp/demo")));
        assert!(store.projects().is_empty());
    }

    #[test]
    fn core_panels_and_commands_are_registered() {
        let mut panels = PanelRegistry::default();
        let mut commands = CommandRegistry::default();

        register_core_panels(&mut panels);
        register_core_commands(&mut commands);

        for id in [
            "hierarchy",
            "inspector",
            "project",
            "console",
            "scene_view",
            "game_view",
        ] {
            assert!(panels.get(id).is_some(), "missing panel {id}");
        }
        for id in [
            "play.toggle",
            "play.pause",
            "play.stop",
            "play.step",
            "edit.undo",
            "edit.redo",
            "assets.reload",
            "scene.save",
            "project.build",
            "command.palette",
        ] {
            assert!(commands.get(id).is_some(), "missing command {id}");
        }
        assert_eq!(
            commands.get("scene.save").unwrap().shortcut.as_deref(),
            Some("Ctrl+S")
        );
    }

    #[test]
    fn selection_and_console_filtering_are_shared_services() {
        let mut selection = SelectionService::default();
        selection.select(Selection::Entity("player".to_owned()));
        assert_eq!(
            selection.selected(),
            Some(&Selection::Entity("player".to_owned()))
        );

        let mut console = ConsoleService::default();
        console.push(ConsoleEntry {
            timestamp: "10:00".to_owned(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "importer".to_owned(),
                file: None,
                line: None,
            },
            message: "import complete".to_owned(),
        });
        console.push(ConsoleEntry {
            timestamp: "10:01".to_owned(),
            level: ConsoleLevel::Error,
            source: ConsoleSource {
                subsystem: "builder".to_owned(),
                file: Some(PathBuf::from("build.rs")),
                line: Some(7),
            },
            message: "build failed".to_owned(),
        });

        let filtered = console.filtered(&ConsoleFilter {
            min_level: Some(ConsoleLevel::Warn),
            source_contains: None,
            message_contains: Some("build".to_owned()),
        });

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].source.file, Some(PathBuf::from("build.rs")));
    }

    #[test]
    fn undo_redo_stack_moves_commands_between_histories() {
        let mut stack = UndoRedoStack::default();
        stack.push(UndoCommand::new(
            "Rename",
            "entity:player",
            "Player",
            "Hero",
        ));

        assert!(stack.can_undo());
        assert_eq!(stack.undo().unwrap().before, "Player");
        assert!(stack.can_redo());
        assert_eq!(stack.redo().unwrap().after, "Hero");
    }
}
