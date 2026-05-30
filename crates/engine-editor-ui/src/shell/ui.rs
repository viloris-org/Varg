//! egui rendering for [`EditorShell`].
//!
//! Call [`draw_shell`] once per frame inside an egui context.

#![allow(deprecated)] // egui 0.34 keeps Panel::show(ctx) available.

use egui::{Frame, Margin};

use super::operations::command::{
    apply_visuals, handle_command_shortcuts, handle_transform_tool_shortcuts,
};
use super::panels::{
    draw_bottom_dock, draw_center_dock, draw_close_project_dialog, draw_command_palette,
    draw_copilot, draw_hierarchy, draw_inspector, draw_menu_bar, draw_project_panel,
    draw_script_editor, draw_status_bar, draw_toolbar,
};
use super::types::{InfernuxPalette, ShellUiState};
use super::widgets::layout::{panel_frame, panel_title};
use crate::EditorShell;
use engine_i18n::Translations;

/// Draw the full editor shell into `ctx`.
///
/// Returns `true` when the user requests the window to close.
pub fn draw_shell(
    ctx: &egui::Context,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
) -> bool {
    let pal = InfernuxPalette::dark();
    let tr = Translations::load(shell.preferences().locale);
    apply_visuals(ctx, &pal);
    handle_command_shortcuts(ctx, shell, ui_state, &tr);
    handle_transform_tool_shortcuts(ctx, ui_state);

    let close = false;

    // Top menu bar
    egui::TopBottomPanel::top("infernux_menu_bar")
        .exact_size(26.0)
        .frame(
            Frame::NONE
                .fill(pal.menu_bar)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ctx, |ui| draw_menu_bar(ui, shell, ui_state, &pal, &tr));

    // Toolbar
    egui::TopBottomPanel::top("infernux_toolbar")
        .exact_size(36.0)
        .frame(
            Frame::NONE
                .fill(pal.panel_bg)
                .inner_margin(Margin::symmetric(6, 3)),
        )
        .show(ctx, |ui| draw_toolbar(ui, shell, ui_state, &pal, &tr));

    // Status bar
    egui::TopBottomPanel::bottom("infernux_status_bar")
        .exact_size(24.0)
        .frame(
            Frame::NONE
                .fill(pal.status_bar)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ctx, |ui| draw_status_bar(ui, shell, ui_state, &pal, &tr));

    // Left panel (hierarchy + project)
    if ui_state.show_hierarchy || ui_state.show_project {
        egui::SidePanel::left("infernux_hierarchy")
            .default_size(220.0)
            .min_width(150.0)
            .frame(panel_frame(&pal))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    if ui_state.show_hierarchy {
                        if ui_state.show_project {
                            ui.set_max_height(ui.available_height() * 0.75);
                        }
                        draw_hierarchy(ui, shell, ui_state, &pal, &tr);
                    }
                    if ui_state.show_hierarchy && ui_state.show_project {
                        ui.separator();
                    }
                    if ui_state.show_project {
                        panel_title(ui, tr.tr("panel_project"), &pal);
                        draw_project_panel(ui, shell, ui_state, &pal, &tr);
                    }
                });
            });
    }

    // Right inspector panel
    if ui_state.show_inspector {
        egui::SidePanel::right("infernux_inspector")
            .default_size(330.0)
            .min_width(240.0)
            .frame(panel_frame(&pal))
            .show(ctx, |ui| {
                draw_inspector(ui, shell, ui_state, &pal, &tr);
            });
    }

    // Copilot panel (right side, next to inspector)
    if ui_state.copilot.visible {
        let _res = egui::SidePanel::right("infernux_copilot")
            .default_width(340.0)
            .min_width(260.0)
            .max_width(500.0)
            .resizable(true)
            .frame(panel_frame(&pal))
            .show(ctx, |ui| {
                draw_copilot(ui, shell, ui_state, &pal, &tr);
            });
    }

    // Bottom dock (console)
    if ui_state.show_console {
        egui::TopBottomPanel::bottom("infernux_bottom_dock")
            .default_height(230.0)
            .min_height(120.0)
            .frame(panel_frame(&pal))
            .show(ctx, |ui| draw_bottom_dock(ui, shell, ui_state, &pal, &tr));
    }

    // Center viewport
    egui::CentralPanel::default()
        .frame(Frame::NONE.fill(pal.window_bg))
        .show(ctx, |ui| draw_center_dock(ui, shell, ui_state, &pal, &tr));

    // Dialogs
    if ui_state.command_palette_open {
        draw_command_palette(ctx, shell, ui_state, &pal, &tr);
    }

    if ui_state.show_close_dialog {
        draw_close_project_dialog(ctx, shell, ui_state, &pal, &tr);
    }

    if ui_state.script_editor.is_some() {
        draw_script_editor(ctx, shell, ui_state, &pal, &tr);
    }

    close
}

// Re-export build_editor_render_world (used by other modules)
pub use super::panels::viewport::{build_camera_preview_render_world, build_editor_render_world};
