//! Panel modules for the editor shell.

pub mod console;
pub mod copilot;
pub mod dialogs;
pub mod hierarchy;
pub mod inspector;
pub mod menu;
pub mod project;
pub mod status_bar;
pub mod toolbar;
pub mod viewport;

// Re-export panel drawing functions
pub use console::draw_console;
pub use copilot::draw_copilot;
pub use dialogs::{draw_close_project_dialog, draw_command_palette, draw_script_editor};
pub use hierarchy::draw_hierarchy;
pub use inspector::draw_inspector;
pub use menu::draw_menu_bar;
pub use project::draw_project_panel;
pub use status_bar::draw_status_bar;
pub use toolbar::draw_toolbar;
pub use viewport::{
    build_camera_preview_render_world, build_editor_render_world, draw_bottom_dock,
    draw_center_dock,
};
