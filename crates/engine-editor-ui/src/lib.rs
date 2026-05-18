#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Native Hub and editor shell state for the first Aster UI surface.
//!
//! This crate intentionally keeps the first shell free of concrete UI framework dependencies.
//! A future egui or native backend can render this state without entering `runtime-min`.

use std::path::{Path, PathBuf};

use engine_core::{EngineError, EngineResult};
use engine_editor::{
    register_core_commands, register_core_panels, CommandRegistry, ConsoleService,
    EditorPreferences, MemoryProjectStore, NewProjectRequest, PanelRegistry, ProjectCreationPlan,
    ProjectMetadata, ProjectStore, SelectionService, ThemePreference, ToolchainInstall,
};

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
    /// Launch the editor with a project and toolchain version.
    LaunchEditor {
        /// Project root path.
        project_path: PathBuf,
        /// Toolchain version to launch.
        toolchain_version: String,
    },
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
        }
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

/// First native editor shell state.
#[derive(Clone, Debug, Default)]
pub struct EditorShell {
    panels: PanelRegistry,
    commands: CommandRegistry,
    selection: SelectionService,
    console: ConsoleService,
    preferences: EditorPreferences,
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
        for id in ["play", "pause", "stop", "reload", "save", "build"] {
            assert!(shell.commands().get(id).is_some(), "missing command {id}");
        }
    }
}
