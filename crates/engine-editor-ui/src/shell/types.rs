//! Shared types for the editor shell UI.

use egui::Color32;
use engine_assets::AssetGuid;
use engine_core::{
    math::{Transform, Vec3 as EngineVec3},
    EntityId,
};
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
            row_alt: Color32::from_rgba_premultiplied(8, 8, 8, 8),
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

/// Projection mode used by the editor Scene View camera.
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
        axis: EngineVec3,
    },
    /// Translate within the plane spanned by two world-space gizmo axes.
    MovePlane {
        /// First world-space axis used by the active plane handle.
        axis_a: EngineVec3,
        /// Second world-space axis used by the active plane handle.
        axis_b: EngineVec3,
    },
    /// Transform from raw screen-space movement.
    Screen,
    /// Rotate around a world-space axis.
    RotateAxis {
        /// World-space rotation axis.
        axis: EngineVec3,
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
    pub start_hit: Option<EngineVec3>,
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
    /// Script file name typed by the user for Project panel script creation.
    pub project_new_script_name: String,
    /// Script backend selected for newly-created script assets.
    pub project_new_script_backend: ScriptTemplateBackend,
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
    /// Last requested selected-camera preview render target.
    pub camera_preview_target: Option<ViewportTargetState>,
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
    /// Rendered selected-camera preview texture set by the native host.
    pub camera_preview_texture: Option<ViewportTexture>,
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
    /// Current Scene View projection mode.
    pub editor_scene_view_projection: EditorSceneViewProjection,
    /// Whether axis presets automatically switch Scene View to orthographic mode.
    pub editor_scene_view_auto_orthographic: bool,
    /// Current named Scene View orientation.
    pub editor_scene_view_orientation: EditorSceneViewOrientation,
    /// Current transform tool coordinate space.
    pub editor_transform_space: EditorTransformSpace,
    /// Current transform tool mode.
    pub editor_transform_tool: EditorTransformTool,
    /// Inspector component type IDs that are currently collapsed.
    pub inspector_collapsed: Vec<String>,
    /// Scene snapshot captured before a transform drag began (batches undo to drag session).
    pub inspector_drag_before: Option<String>,
    /// Scene snapshot captured before a Scene View guide drag began.
    pub scene_guide_drag_before: Option<(EntityId, String)>,
    /// Scene snapshot captured before a viewport transform drag began.
    pub viewport_transform_drag_before: Option<(EntityId, String)>,
    /// Full viewport transform drag state used for stable cumulative edits.
    pub viewport_transform_drag: Option<ViewportTransformDragState>,
    /// Filter text for the Add Component searchable dropdown.
    pub add_component_filter: String,
    /// Component type ID awaiting removal confirmation (two-click delete).
    pub remove_confirm: Option<String>,
    /// Editor snap settings for transform gizmos.
    pub editor_snap_settings: EditorSnapSettings,
    /// Whether snapping is temporarily toggled via Ctrl key.
    pub snap_toggle: bool,
    /// Whether the current drag has produced any accumulated movement.
    pub drag_dirty: bool,
    /// Text label shown near the gizmo during drag (e.g. "d 1.23, 0.00, -0.45").
    pub drag_delta_label: Option<String>,
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
    /// Current in-editor script editing session.
    pub script_editor: Option<ScriptEditorState>,
    /// Copilot panel transient state.
    pub copilot: CopilotPanelState,
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
            project_new_script_name: "player_controller".to_owned(),
            project_new_script_backend: ScriptTemplateBackend::Python,
            project_import_status: None,
            hierarchy_selection: Vec::new(),
            hierarchy_dragging: None,
            hierarchy_rename: None,
            dragged_asset: None,
            scene_view_target: None,
            game_view_target: None,
            camera_preview_target: None,
            runtime_game_world: None,
            play_mode_request: None,
            command_palette_open: false,
            command_filter: String::new(),
            command_status: None,
            scene_view_texture: None,
            game_view_texture: None,
            camera_preview_texture: None,
            editor_camera_yaw: 0.0,
            editor_camera_pitch: 0.3,
            editor_camera_distance: 6.0,
            editor_camera_target_distance: 6.0,
            editor_camera_target: [0.0, 1.0, 0.0],
            editor_scene_view_projection: EditorSceneViewProjection::Perspective,
            editor_scene_view_auto_orthographic: true,
            editor_scene_view_orientation: EditorSceneViewOrientation::Free,
            editor_transform_space: EditorTransformSpace::Global,
            editor_transform_tool: EditorTransformTool::Move,
            inspector_collapsed: Vec::new(),
            inspector_drag_before: None,
            scene_guide_drag_before: None,
            viewport_transform_drag_before: None,
            viewport_transform_drag: None,
            add_component_filter: String::new(),
            remove_confirm: None,
            editor_snap_settings: EditorSnapSettings::default(),
            snap_toggle: false,
            drag_dirty: false,
            drag_delta_label: None,
            status_toast: None,
            status_toast_frames: 0,
            pending_action: None,
            pending_action_after_close: None,
            show_close_dialog: false,
            close_dialog_exit_app: false,
            expanded_folders: std::collections::BTreeSet::new(),
            asset_rename: None,
            asset_delete_confirm: None,
            script_editor: None,
            copilot: CopilotPanelState {
                visible: true,
                ..CopilotPanelState::default()
            },
        }
    }
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

/// Transient Copilot panel state.
#[derive(Clone, Debug)]
pub struct CopilotPanelState {
    /// Whether the panel is visible.
    pub visible: bool,
    /// User input text.
    pub input: String,
    /// Chat history.
    pub messages: Vec<CopilotChatMessage>,
    /// Current operation status.
    pub status: CopilotStatus,
    /// Whether auto-accept is enabled for low/medium risk operations.
    pub auto_accept: bool,
    /// Whether the trace section is expanded.
    pub trace_expanded: bool,
    /// Cached plan preview lines (one per planned operation).
    pub plan_preview: Vec<PlanPreviewItem>,
    /// Cached trace entries from the last execution.
    pub trace_entries: Vec<String>, // serialized TraceEntry debug lines
    /// Console entry count from the last execution.
    pub console_entry_count: usize,
    /// Error count from the last execution.
    pub console_error_count: usize,
    /// Status message to show (e.g. "Applied 4 operations").
    pub status_message: Option<String>,
    /// Error message to display.
    pub error_message: Option<String>,
}

impl Default for CopilotPanelState {
    fn default() -> Self {
        Self {
            visible: true,
            input: String::new(),
            messages: Vec::new(),
            status: CopilotStatus::Idle,
            auto_accept: false,
            trace_expanded: false,
            plan_preview: Vec::new(),
            trace_entries: Vec::new(),
            console_entry_count: 0,
            console_error_count: 0,
            status_message: None,
            error_message: None,
        }
    }
}

/// Status of the current Copilot operation.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CopilotStatus {
    /// Idle — waiting for user input.
    #[default]
    Idle,
    /// Agent is planning (sending to model, parsing response).
    Planning,
    /// Plan is ready for user review.
    ReadyForReview,
    /// Agent is executing approved operations.
    Executing,
    /// Execution complete.
    Complete,
    /// An error occurred.
    Error(String),
}

/// A single message in the Copilot chat history.
#[derive(Clone, Debug)]
pub struct CopilotChatMessage {
    /// Message role ("user" or "assistant").
    pub role: String,
    /// Message content.
    pub content: String,
}

/// A single planned operation shown in the review UI.
#[derive(Clone, Debug)]
pub struct PlanPreviewItem {
    /// Index in the plan.
    pub index: usize,
    /// Human-readable preview text.
    pub preview: String,
    /// Whether the operation is write-capable (requires approval).
    pub requires_write: bool,
    /// User has approved this operation.
    pub approved: bool,
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
