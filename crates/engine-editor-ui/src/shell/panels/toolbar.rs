//! Toolbar panel for the editor shell.

use egui::{Color32, RichText, Vec2};

use super::super::operations::command::{command_enabled, execute_shell_command};
use super::super::types::{
    EditorTransformSpace, EditorTransformTool, InfernuxPalette, ShellUiState,
};
use super::super::widgets::buttons::{
    dropdown_pill, panel_toggle, small_text_button_widget, tool_button,
};
use super::super::widgets::icons::{actions, tools, transport};
use crate::EditorShell;
use engine_i18n::Translations;
/// Renders the toolbar with transport controls.

pub fn draw_toolbar(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    ui.horizontal_centered(|ui| {
        transform_tool_button(
            ui,
            ui_state,
            EditorTransformTool::View,
            tr.tr("tool_view"),
            tools::VIEW,
            "Q",
            pal,
        );
        transform_tool_button(
            ui,
            ui_state,
            EditorTransformTool::Move,
            tr.tr("tool_move"),
            tools::MOVE,
            "W",
            pal,
        );
        transform_tool_button(
            ui,
            ui_state,
            EditorTransformTool::Rotate,
            tr.tr("tool_rotate"),
            tools::ROTATE,
            "E",
            pal,
        );
        transform_tool_button(
            ui,
            ui_state,
            EditorTransformTool::Scale,
            tr.tr("tool_scale"),
            tools::SCALE,
            "R",
            pal,
        );
        ui.separator();
        transform_space_dropdown(ui, ui_state, tr);
        dropdown_pill(ui, tr.tr("tool_pivot"), 68.0, pal);
        snap_dropdown(ui, ui_state, tr, pal);
        ui.add_space(12.0);
        if transport_command_button(
            ui,
            shell,
            ui_state,
            "play.toggle",
            transport::PLAY,
            pal.play,
            pal,
        )
        .clicked()
        {
            execute_shell_command(shell, ui_state, "play.toggle", tr);
        }
        if transport_command_button(
            ui,
            shell,
            ui_state,
            "play.pause",
            transport::PAUSE,
            pal.pause,
            pal,
        )
        .clicked()
        {
            execute_shell_command(shell, ui_state, "play.pause", tr);
        }
        if transport_command_button(
            ui,
            shell,
            ui_state,
            "play.stop",
            transport::STOP,
            pal.accent,
            pal,
        )
        .clicked()
        {
            execute_shell_command(shell, ui_state, "play.stop", tr);
        }
        ui.separator();

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            command_text_button(
                ui,
                shell,
                ui_state,
                "scene.save",
                tr.tr("tool_save"),
                Some(actions::SAVE),
                pal,
                tr,
            );
            command_text_button(
                ui,
                shell,
                ui_state,
                "edit.redo",
                tr.tr("command_redo"),
                Some(actions::REDO),
                pal,
                tr,
            );
            command_text_button(
                ui,
                shell,
                ui_state,
                "edit.undo",
                tr.tr("command_undo"),
                Some(actions::UNDO),
                pal,
                tr,
            );
            panel_toggle(
                ui,
                tr.tr("panel_game_view"),
                &mut ui_state.show_game_view,
                pal,
            );
            panel_toggle(
                ui,
                tr.tr("panel_scene_view"),
                &mut ui_state.show_scene_view,
                pal,
            );
            panel_toggle(ui, tr.tr("panel_console"), &mut ui_state.show_console, pal);
            panel_toggle(ui, tr.tr("panel_project"), &mut ui_state.show_project, pal);
            panel_toggle(
                ui,
                tr.tr("panel_inspector"),
                &mut ui_state.show_inspector,
                pal,
            );
            panel_toggle(
                ui,
                tr.tr("panel_copilot"),
                &mut ui_state.copilot.visible,
                pal,
            );
            panel_toggle(
                ui,
                tr.tr("panel_hierarchy"),
                &mut ui_state.show_hierarchy,
                pal,
            );
        });
    });
}

fn transform_tool_button(
    ui: &mut egui::Ui,
    ui_state: &mut ShellUiState,
    tool: EditorTransformTool,
    tooltip: &str,
    icon: &str,
    shortcut: &str,
    pal: &InfernuxPalette,
) {
    let selected = ui_state.editor_transform_tool == tool;
    let response = tool_button(ui, icon, shortcut, tooltip, selected, pal);
    if response.clicked() {
        ui_state.editor_transform_tool = tool;
    }
}

fn transform_space_dropdown(ui: &mut egui::Ui, ui_state: &mut ShellUiState, tr: &Translations) {
    egui::ComboBox::from_id_salt("toolbar_transform_space")
        .width(76.0)
        .selected_text(transform_space_label(ui_state.editor_transform_space, tr))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut ui_state.editor_transform_space,
                EditorTransformSpace::Global,
                tr.tr("tool_global"),
            )
            .on_hover_text(tr.tr("tool_global_hint"));
            ui.selectable_value(
                &mut ui_state.editor_transform_space,
                EditorTransformSpace::Local,
                tr.tr("tool_local"),
            )
            .on_hover_text(tr.tr("tool_local_hint"));
        })
        .response
        .on_hover_text(tr.tr("tool_transform_space_hint"));
}

fn transform_space_label(space: EditorTransformSpace, tr: &Translations) -> String {
    match space {
        EditorTransformSpace::Global => tr.tr("tool_global"),
        EditorTransformSpace::Local => tr.tr("tool_local"),
    }
    .to_owned()
}

fn snap_dropdown(
    ui: &mut egui::Ui,
    ui_state: &mut ShellUiState,
    tr: &Translations,
    pal: &InfernuxPalette,
) {
    let snap_label = snap_summary(&ui_state.editor_snap_settings);
    egui::ComboBox::from_id_salt("toolbar_snap")
        .width(58.0)
        .selected_text(snap_label)
        .show_ui(ui, |ui| {
            let snap = &mut ui_state.editor_snap_settings;
            ui.label(RichText::new(tr.tr("tool_snap_move")).color(pal.text_dim));
            for &step in &SNAP_PRESETS_MOVE {
                if ui
                    .selectable_label(snap.move_snap == Some(step), format!("{step:.2}"))
                    .clicked()
                {
                    snap.move_snap = if snap.move_snap == Some(step) {
                        None
                    } else {
                        Some(step)
                    };
                }
            }
            ui.separator();
            ui.label(RichText::new(tr.tr("tool_snap_angle")).color(pal.text_dim));
            for &step in &SNAP_PRESETS_ANGLE {
                if ui
                    .selectable_label(snap.angle_snap == Some(step), format!("{step:.0}\u{00b0}"))
                    .clicked()
                {
                    snap.angle_snap = if snap.angle_snap == Some(step) {
                        None
                    } else {
                        Some(step)
                    };
                }
            }
            ui.separator();
            ui.label(RichText::new(tr.tr("tool_snap_scale")).color(pal.text_dim));
            for &step in &SNAP_PRESETS_SCALE {
                if ui
                    .selectable_label(snap.scale_snap == Some(step), format!("{step:.2}"))
                    .clicked()
                {
                    snap.scale_snap = if snap.scale_snap == Some(step) {
                        None
                    } else {
                        Some(step)
                    };
                }
            }
        })
        .response
        .on_hover_text(tr.tr("tool_snap_hint"));
}

fn snap_summary(settings: &super::super::types::EditorSnapSettings) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = settings.move_snap {
        parts.push(format!("G:{m:.2}"));
    }
    if let Some(a) = settings.angle_snap {
        parts.push(format!("A:{a:.0}"));
    }
    if let Some(s) = settings.scale_snap {
        parts.push(format!("S:{s:.2}"));
    }
    if parts.is_empty() {
        "Snap".to_owned()
    } else {
        parts.join(" ")
    }
}

const SNAP_PRESETS_MOVE: [f32; 3] = [0.25, 0.5, 1.0];
const SNAP_PRESETS_ANGLE: [f32; 3] = [5.0, 15.0, 45.0];
const SNAP_PRESETS_SCALE: [f32; 3] = [0.1, 0.25, 0.5];

/// Renders a transport control button (play/pause/stop).

pub fn transport_command_button(
    ui: &mut egui::Ui,
    shell: &EditorShell,
    ui_state: &ShellUiState,
    command_id: &str,
    icon: &str,
    active_color: Color32,
    pal: &InfernuxPalette,
) -> egui::Response {
    let active = match command_id {
        "play.toggle" => ui_state.playing,
        "play.pause" => ui_state.paused,
        _ => false,
    };
    let enabled = shell
        .commands()
        .get(command_id)
        .map(|command| command_enabled(shell, ui_state, command))
        .unwrap_or(false);

    let icon_text = RichText::new(icon).size(16.0).color(pal.text);

    ui.add_enabled(
        enabled,
        egui::Button::new(icon_text)
            .fill(if active { active_color } else { pal.frame_bg })
            .min_size(Vec2::new(32.0, 28.0)),
    )
}
/// Renders a text-based command button.

pub fn command_text_button(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    command_id: &str,
    fallback_label: &str,
    icon: Option<&str>,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    let command = shell.commands().get(command_id).cloned();
    let enabled = command
        .as_ref()
        .map(|command| command_enabled(shell, ui_state, command))
        .unwrap_or(false);
    let label = command
        .as_ref()
        .map(|command| command.label.as_str())
        .unwrap_or(fallback_label);
    let button_label = icon.unwrap_or(label);
    if ui
        .add_enabled(enabled, small_text_button_widget(button_label, pal))
        .on_hover_text(label)
        .clicked()
    {
        execute_shell_command(shell, ui_state, command_id, tr);
    }
}
