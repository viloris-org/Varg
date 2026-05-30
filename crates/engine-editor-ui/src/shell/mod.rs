//! Editor shell UI modules.

pub mod copilot_engine;
pub mod operations;
pub mod panels;
pub mod types;
pub mod ui;
pub mod widgets;

pub use types::{
    EditorAction, PlayModeRequest, ScriptEditorState, ScriptTemplateBackend, ShellUiState,
    ViewportTexture,
};
pub use ui::{build_camera_preview_render_world, build_editor_render_world, draw_shell};
