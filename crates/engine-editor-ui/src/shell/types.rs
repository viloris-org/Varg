//! Shared types for the editor shell UI.

use egui::Color32;
use engine_assets::AssetGuid;
use engine_core::EntityId;
use engine_render::{RenderTargetDesc, RenderWorld};
use std::path::PathBuf;

/// RGB color helper.
pub fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Dark theme color palette for the Infernux editor.
#[derive(Clone, Copy)]
pub struct InfernuxPalette {
    /// Primary text color.
    pub text: Color32,
    /// Dimmed text color.
    pub text_dim: Color32,
    /// Disabled text color.
    pub text_disabled: Color32,
    /// Window background color.
    pub window_bg: Color32,
    /// Panel background color.
    pub panel_bg: Color32,
    /// Menu bar background color.
    pub menu_bar: Color32,
    /// Status bar background color.
    pub status_bar: Color32,
    /// Viewport background color.
    pub viewport_bg: Color32,
    /// Frame background color.
    pub frame_bg: Color32,
    /// Input field background color.
    pub input_bg: Color32,
    /// Frame hover color.
    pub frame_hover: Color32,
    /// Frame active/pressed color.
    pub frame_active: Color32,
    /// Header background color.
    pub header: Color32,
    /// Header hover color.
    pub header_hover: Color32,
    /// Header active color.
    pub header_active: Color32,
    /// Border color.
    pub border: Color32,
    /// Border highlight color (for focused elements).
    pub border_highlight: Color32,
    /// Subtle separator color.
    pub separator: Color32,
    /// Alternate row color.
    pub row_alt: Color32,
    /// Selection color.
    pub selection: Color32,
    /// Selection hover color.
    pub selection_hover: Color32,
    /// Accent color.
    pub accent: Color32,
    /// Accent hover color.
    pub accent_hover: Color32,
    /// Play button color.
    pub play: Color32,
    /// Play button hover color.
    pub play_hover: Color32,
    /// Pause button color.
    pub pause: Color32,
    /// Pause button hover color.
    pub pause_hover: Color32,
    /// Warning color.
    pub warning: Color32,
    /// Error color.
    pub error: Color32,
    /// Success/info color.
    pub success: Color32,
    /// Overlay background (for modals/dialogs).
    pub overlay_bg: Color32,
}

impl InfernuxPalette {
    /// Creates a dark theme palette.
    pub const fn dark() -> Self {
        Self {
            text: Color32::from_rgb(220, 220, 220),
            text_dim: Color32::from_rgb(150, 150, 150),
            text_disabled: Color32::from_rgb(100, 100, 100),
            window_bg: Color32::from_rgb(32, 32, 32),
            panel_bg: Color32::from_rgb(40, 40, 40),
            menu_bar: Color32::from_rgb(35, 35, 35),
            status_bar: Color32::from_rgb(30, 30, 30),
            viewport_bg: Color32::from_rgb(28, 28, 28),
            frame_bg: Color32::from_rgb(50, 50, 50),
            input_bg: Color32::from_rgb(38, 38, 38),
            frame_hover: Color32::from_rgb(60, 60, 60),
            frame_active: Color32::from_rgb(55, 55, 55),
            header: Color32::from_rgb(48, 48, 48),
            header_hover: Color32::from_rgb(58, 58, 58),
            header_active: Color32::from_rgb(52, 52, 52),
            border: Color32::from_rgb(60, 60, 60),
            border_highlight: Color32::from_rgb(80, 120, 160),
            separator: Color32::from_rgb(55, 55, 55),
            row_alt: Color32::from_rgba_premultiplied(255, 255, 255, 8),
            selection: Color32::from_rgb(50, 100, 150),
            selection_hover: Color32::from_rgb(60, 110, 160),
            accent: Color32::from_rgb(220, 80, 80),
            accent_hover: Color32::from_rgb(235, 95, 95),
            play: Color32::from_rgb(60, 130, 90),
            play_hover: Color32::from_rgb(70, 145, 105),
            pause: Color32::from_rgb(140, 115, 50),
            pause_hover: Color32::from_rgb(155, 130, 65),
            warning: Color32::from_rgb(220, 170, 70),
            error: Color32::from_rgb(220, 80, 80),
            success: Color32::from_rgb(80, 180, 120),
            overlay_bg: Color32::from_rgba_premultiplied(0, 0, 0, 180),
        }
    }
}

/// Info about a wgpu-rendered texture ready for display in a viewport via `egui::Image`.
#[derive(Clone, Debug)]
pub struct ViewportTexture {
    /// Texture ID (wraps into `egui::TextureId`).
    pub id: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Transient UI state for the editor shell.
#[derive(Debug, Default)]
pub struct ShellUiState {
    /// Whether the Hierarchy panel is visible.
    pub show_hierarchy: bool,
    /// Whether the Inspector panel is visible.
    pub show_inspector: bool,
    /// Whether the Project panel is visible.
    pub show_project: bool,
    /// Whether the Console panel is visible.
    pub show_console: bool,
    /// Whether the Scene View panel is visible.
    pub show_scene_view: bool,
    /// Whether the Game View panel is visible.
    pub show_game_view: bool,
    /// Whether the engine is in play mode.
    pub playing: bool,
    /// Whether the engine is paused.
    pub paused: bool,
    /// Hierarchy object-name filter.
    pub hierarchy_filter: String,
    /// Project asset-name filter.
    pub project_filter: String,
    /// Console message filter.
    pub console_filter: String,
    /// Whether repeated console rows are collapsed by message.
    pub console_collapse: bool,
    /// Path typed by the user for Project panel import.
    pub project_import_path: String,
    /// Last Project panel import or rescan status.
    pub project_import_status: Option<String>,
    /// Scene object IDs selected in Hierarchy.
    pub hierarchy_selection: Vec<EntityId>,
    /// Dragged hierarchy object, if any.
    pub hierarchy_dragging: Option<EntityId>,
    /// Entity currently being renamed in hierarchy: (EntityId, edit text).
    pub hierarchy_rename: Option<(EntityId, String)>,
    /// Asset dragged from Project panel.
    pub dragged_asset: Option<AssetGuid>,
    /// Last requested Scene View render target.
    pub scene_view_target: Option<ViewportTargetState>,
    /// Last requested Game View render target.
    pub game_view_target: Option<ViewportTargetState>,
    /// Latest Game View render-world produced by Play Mode runtime ticking.
    pub runtime_game_world: Option<RenderWorld>,
    /// Pending Play Mode request for the native editor host to execute.
    pub play_mode_request: Option<PlayModeRequest>,
    /// Whether the command palette popup is open.
    pub command_palette_open: bool,
    /// Command palette text filter.
    pub command_filter: String,
    /// Last command dispatch status shown in the command palette.
    pub command_status: Option<String>,
    /// Rendered scene view texture set by the native host before each egui frame.
    pub scene_view_texture: Option<ViewportTexture>,
    /// Rendered game view texture set by the native host before each egui frame.
    pub game_view_texture: Option<ViewportTexture>,
    /// Editor camera orbit state: yaw angle in radians.
    pub editor_camera_yaw: f32,
    /// Editor camera orbit state: pitch angle in radians.
    pub editor_camera_pitch: f32,
    /// Editor camera orbit distance from target.
    pub editor_camera_distance: f32,
    /// Smoothed editor camera orbit distance target.
    pub editor_camera_target_distance: f32,
    /// Editor camera look-at target in world space.
    pub editor_camera_target: [f32; 3],
    /// Inspector component type IDs that are currently collapsed.
    pub inspector_collapsed: Vec<String>,
    /// Scene snapshot captured before a transform drag began (batches undo to drag session).
    pub inspector_drag_before: Option<String>,
    /// Filter text for the Add Component searchable dropdown.
    pub add_component_filter: String,
    /// Component type ID awaiting removal confirmation (two-click delete).
    pub remove_confirm: Option<String>,
    /// Temporary status message shown in the status bar.
    pub status_toast: Option<String>,
    /// Frames remaining before the status toast is cleared.
    pub status_toast_frames: u32,
    /// Pending action for the native host to execute.
    pub pending_action: Option<EditorAction>,
    /// Action to execute after the unsaved-changes dialog is resolved (Save or Discard).
    pub pending_action_after_close: Option<EditorAction>,
    /// Whether the unsaved-changes close dialog is visible.
    pub show_close_dialog: bool,
    /// Whether closing the dialog should exit the app (true) or return to hub (false).
    pub close_dialog_exit_app: bool,
    /// Folder paths (relative) that are expanded in the Project panel tree.
    pub expanded_folders: std::collections::BTreeSet<String>,
    /// Asset path currently being renamed in the Project panel: (relative_path, edit_text).
    pub asset_rename: Option<(PathBuf, String)>,
    /// Asset path awaiting delete confirmation (two-click delete).
    pub asset_delete_confirm: Option<PathBuf>,
}

impl ShellUiState {
    /// Creates a default state with the Infernux editor panels open.
    pub fn all_open() -> Self {
        Self {
            show_hierarchy: true,
            show_inspector: true,
            show_project: true,
            show_console: true,
            show_scene_view: true,
            show_game_view: true,
            playing: false,
            paused: false,
            hierarchy_filter: String::new(),
            project_filter: String::new(),
            console_filter: String::new(),
            console_collapse: false,
            project_import_path: String::new(),
            project_import_status: None,
            hierarchy_selection: Vec::new(),
            hierarchy_dragging: None,
            hierarchy_rename: None,
            dragged_asset: None,
            scene_view_target: None,
            game_view_target: None,
            runtime_game_world: None,
            play_mode_request: None,
            command_palette_open: false,
            command_filter: String::new(),
            command_status: None,
            scene_view_texture: None,
            game_view_texture: None,
            editor_camera_yaw: 0.0,
            editor_camera_pitch: 0.3,
            editor_camera_distance: 6.0,
            editor_camera_target_distance: 6.0,
            editor_camera_target: [0.0, 1.0, 0.0],
            inspector_collapsed: Vec::new(),
            inspector_drag_before: None,
            add_component_filter: String::new(),
            remove_confirm: None,
            status_toast: None,
            status_toast_frames: 0,
            pending_action: None,
            pending_action_after_close: None,
            show_close_dialog: false,
            close_dialog_exit_app: false,
            expanded_folders: std::collections::BTreeSet::new(),
            asset_rename: None,
            asset_delete_confirm: None,
        }
    }
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
