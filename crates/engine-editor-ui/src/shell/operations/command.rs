//! Command execution and availability checking for the editor shell.

use crate::EditorShell;
use egui::{CornerRadius, Stroke};
use engine_editor::{ConsoleEntry, ConsoleLevel, ConsoleSource};
use engine_i18n::Translations;

use super::super::types::{
    EditorAction, EditorTransformTool, InfernuxPalette, PlayModeRequest, ShellUiState,
};
use super::asset_ops::{create_default_material, create_script_asset};
use super::scene_ops::{
    add_component_to_selected, create_empty_object, create_root_object_with_component,
};
use engine_ecs::{
    AudioSourceComponentData, CameraComponentData, ColliderComponentData, ComponentData,
    LightComponentData, MeshRendererComponentData, ParticleEmitterComponentData,
    RigidbodyComponentData, ScriptComponentProxy,
};

/// Check if a command is currently enabled based on availability rules.
pub fn command_enabled(
    shell: &EditorShell,
    ui_state: &ShellUiState,
    command: &engine_editor::EditorCommand,
) -> bool {
    use engine_editor::CommandAvailability;
    match command.availability {
        CommandAvailability::Always => true,
        CommandAvailability::ProjectOpen => shell.project().is_some(),
        CommandAvailability::DirtyScene => shell
            .project()
            .map(|project| project.scene_dirty)
            .unwrap_or(false),
        CommandAvailability::CanUndo => shell.undo_stack().can_undo(),
        CommandAvailability::CanRedo => shell.undo_stack().can_redo(),
        CommandAvailability::Playing => ui_state.playing,
        CommandAvailability::NotPlaying => !ui_state.playing,
    }
}

/// Execute a shell command by ID, handling all command routing and state updates.
pub fn execute_shell_command(
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    command_id: &str,
    tr: &Translations,
) {
    let Some(command) = shell.commands().get(command_id).cloned() else {
        return;
    };
    if !command_enabled(shell, ui_state, &command) {
        ui_state.command_status =
            Some(tr.tr_fmt("command_status_not_available", &[&command.label]));
        return;
    }

    match command_id {
        "project.open" => {
            ui_state.command_status = Some(tr.tr("command_status_open_project").to_owned());
        }
        "scene.open" => {
            if shell.is_scene_dirty() {
                ui_state.show_close_dialog = true;
                ui_state.close_dialog_exit_app = false;
                ui_state.pending_action_after_close = Some(EditorAction::OpenScene);
            } else {
                ui_state.pending_action = Some(EditorAction::OpenScene);
            }
        }
        "scene.save" => match shell.save_scene() {
            Ok(display_path) => {
                ui_state.status_toast = Some(tr.tr_fmt("status_scene_saved", &[&display_path]));
                ui_state.status_toast_frames = 180;
            }
            Err(error) => push_error(shell, error.to_string()),
        },
        "scene.save_as" => {
            ui_state.pending_action = Some(EditorAction::SaveAs);
        }
        "project.close" => {
            if shell.is_scene_dirty() {
                ui_state.show_close_dialog = true;
                ui_state.close_dialog_exit_app = false;
            } else {
                shell.close_project();
                ui_state.pending_action = Some(EditorAction::ReturnToHub);
            }
        }
        "project.build" => {
            push_info(shell, tr.tr("command_status_build"));
        }
        "edit.undo" => apply_undo(shell),
        "edit.redo" => apply_redo(shell),
        "play.toggle" => {
            if ui_state.playing {
                ui_state.play_mode_request = Some(PlayModeRequest::Stop);
                ui_state.playing = false;
                ui_state.paused = false;
                ui_state.runtime_game_world = None;
            } else {
                ui_state.play_mode_request = Some(PlayModeRequest::Enter);
                ui_state.playing = true;
                ui_state.paused = false;
            }
        }
        "play.pause" => {
            ui_state.paused = !ui_state.paused;
            ui_state.play_mode_request = Some(PlayModeRequest::Pause(ui_state.paused));
        }
        "play.step" => {
            ui_state.paused = true;
            ui_state.play_mode_request = Some(PlayModeRequest::Step);
        }
        "play.stop" => {
            ui_state.playing = false;
            ui_state.paused = false;
            ui_state.runtime_game_world = None;
            ui_state.play_mode_request = Some(PlayModeRequest::Stop);
        }
        "assets.reload" => match shell.project_mut().map(|project| project.rescan_assets()) {
            Some(Ok(())) => push_info(shell, tr.tr("command_status_assets_reloaded")),
            Some(Err(error)) => push_error(shell, error.to_string()),
            None => {}
        },
        "assets.create_material" => create_default_material(shell),
        "assets.create_script" => create_script_asset(shell, ui_state, tr),
        "gameobject.create_empty" => create_empty_object(shell),
        "gameobject.create_camera" => create_root_object_with_component(
            shell,
            "Camera",
            ComponentData::Camera(CameraComponentData::default()),
        ),
        "gameobject.create_light" => create_root_object_with_component(
            shell,
            "Light",
            ComponentData::Light(LightComponentData::default()),
        ),
        "component.add_camera" => add_component_to_selected(
            shell,
            "Camera",
            ComponentData::Camera(CameraComponentData::default()),
        ),
        "component.add_mesh_renderer" => add_component_to_selected(
            shell,
            "Mesh Renderer",
            ComponentData::MeshRenderer(MeshRendererComponentData::default()),
        ),
        "component.add_light" => add_component_to_selected(
            shell,
            "Light",
            ComponentData::Light(LightComponentData::default()),
        ),
        "component.add_rigidbody" => add_component_to_selected(
            shell,
            "Rigidbody",
            ComponentData::Rigidbody(RigidbodyComponentData::default()),
        ),
        "component.add_collider" => add_component_to_selected(
            shell,
            "Collider",
            ComponentData::Collider(ColliderComponentData::default()),
        ),
        "component.add_audio_source" => add_component_to_selected(
            shell,
            "Audio Source",
            ComponentData::AudioSource(AudioSourceComponentData::default()),
        ),
        "component.add_particle_emitter" => add_component_to_selected(
            shell,
            "Particle Emitter",
            ComponentData::ParticleEmitter(ParticleEmitterComponentData::default()),
        ),
        "component.add_script" => add_component_to_selected(
            shell,
            "Script",
            ComponentData::Script(ScriptComponentProxy {
                backend: "rhai".to_owned(),
                script: String::new(),
                state_json: None,
                pending_recovery: false,
            }),
        ),
        "layout.reset" => {
            *ui_state = ShellUiState::all_open();
        }
        "command.palette" => {
            ui_state.command_palette_open = true;
        }
        "help.about" => {
            push_info(
                shell,
                "Aster editor shell: scene, asset, and play-mode tools are connected.",
            );
        }
        _ => {}
    }
    ui_state.command_status = Some(tr.tr_fmt("command_status_ran", &[&command.label]));
}

/// Apply undo operation and handle errors.
pub fn apply_undo(shell: &mut EditorShell) {
    if let Err(error) = shell.undo_scene_command() {
        push_error(shell, error.to_string());
    }
}

/// Apply redo operation and handle errors.
pub fn apply_redo(shell: &mut EditorShell) {
    if let Err(error) = shell.redo_scene_command() {
        push_error(shell, error.to_string());
    }
}

/// Push an error message to the console.
pub fn push_error(shell: &mut EditorShell, message: String) {
    shell.console_mut().push(ConsoleEntry {
        timestamp: "now".to_string(),
        level: ConsoleLevel::Error,
        source: ConsoleSource {
            subsystem: "editor".to_string(),
            file: None,
            line: None,
        },
        message,
    });
}

/// Push an info message to the console.
pub fn push_info(shell: &mut EditorShell, message: impl Into<String>) {
    shell.console_mut().push(ConsoleEntry {
        timestamp: "now".to_string(),
        level: ConsoleLevel::Info,
        source: ConsoleSource {
            subsystem: "editor".to_string(),
            file: None,
            line: None,
        },
        message: message.into(),
    });
}

/// Handle keyboard shortcuts for editor commands.
pub fn handle_command_shortcuts(
    ctx: &egui::Context,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    tr: &Translations,
) {
    let matched = ctx.input(|input| {
        if input.key_pressed(egui::Key::K) && input.modifiers.command && input.modifiers.shift {
            Some("command.palette")
        } else if input.key_pressed(egui::Key::S)
            && input.modifiers.command
            && input.modifiers.shift
        {
            Some("scene.save_as")
        } else if input.key_pressed(egui::Key::S) && input.modifiers.command {
            Some("scene.save")
        } else if input.key_pressed(egui::Key::O) && input.modifiers.command {
            Some("scene.open")
        } else if input.key_pressed(egui::Key::B) && input.modifiers.command {
            Some("project.build")
        } else if input.key_pressed(egui::Key::N)
            && input.modifiers.command
            && input.modifiers.shift
        {
            Some("gameobject.create_empty")
        } else if input.key_pressed(egui::Key::Z) && input.modifiers.command {
            Some("edit.undo")
        } else if input.key_pressed(egui::Key::Y) && input.modifiers.command {
            Some("edit.redo")
        } else if input.key_pressed(egui::Key::P)
            && input.modifiers.command
            && input.modifiers.shift
        {
            Some("play.pause")
        } else if input.key_pressed(egui::Key::P) && input.modifiers.command {
            Some("play.toggle")
        } else if input.key_pressed(egui::Key::F10) {
            Some("play.step")
        } else if input.key_pressed(egui::Key::F5) && input.modifiers.shift {
            Some("play.stop")
        } else {
            None
        }
    });
    if let Some(command_id) = matched {
        execute_shell_command(shell, ui_state, command_id, tr);
    }
}

/// Handle single-key editor transform tool shortcuts.
pub fn handle_transform_tool_shortcuts(ctx: &egui::Context, ui_state: &mut ShellUiState) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }

    let matched = ctx.input(|input| {
        if input.modifiers.any() {
            return None;
        }
        [egui::Key::Q, egui::Key::W, egui::Key::E, egui::Key::R]
            .into_iter()
            .find(|&key| input.key_pressed(key))
            .and_then(transform_tool_for_shortcut_key)
    });

    if let Some(tool) = matched {
        ui_state.editor_transform_tool = tool;
    }
}

pub(crate) fn transform_tool_for_shortcut_key(key: egui::Key) -> Option<EditorTransformTool> {
    match key {
        egui::Key::Q => Some(EditorTransformTool::View),
        egui::Key::W => Some(EditorTransformTool::Move),
        egui::Key::E => Some(EditorTransformTool::Rotate),
        egui::Key::R => Some(EditorTransformTool::Scale),
        _ => None,
    }
}

/// Apply egui visual theme based on the palette.
pub fn apply_visuals(ctx: &egui::Context, pal: &InfernuxPalette) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = pal.window_bg;
    visuals.window_fill = pal.panel_bg;
    visuals.extreme_bg_color = pal.input_bg;
    visuals.faint_bg_color = pal.frame_bg;
    visuals.text_edit_bg_color = Some(pal.input_bg);
    visuals.window_stroke = Stroke::new(1.0, pal.border);
    visuals.window_corner_radius = CornerRadius::same(4);

    visuals.widgets.noninteractive.bg_fill = pal.frame_bg;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(3);

    visuals.widgets.inactive.bg_fill = pal.frame_bg;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text_dim);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(3);

    visuals.widgets.hovered.bg_fill = pal.frame_hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.text);
    visuals.widgets.hovered.expansion = 1.0;
    visuals.widgets.hovered.corner_radius = CornerRadius::same(3);

    visuals.widgets.active.bg_fill = pal.frame_active;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, pal.text);
    visuals.widgets.active.corner_radius = CornerRadius::same(3);

    visuals.widgets.open.bg_fill = pal.header_active;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, pal.text);
    visuals.widgets.open.corner_radius = CornerRadius::same(3);

    visuals.selection.bg_fill = pal.selection;
    visuals.selection.stroke = Stroke::new(1.0, pal.border_highlight);

    visuals.hyperlink_color = pal.accent;
    visuals.warn_fg_color = pal.warning;
    visuals.error_fg_color = pal.error;

    ctx.set_visuals(visuals);
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn qwer_select_transform_tools() {
        assert_eq!(
            transform_tool_for_shortcut_key(egui::Key::Q),
            Some(EditorTransformTool::View)
        );
        assert_eq!(
            transform_tool_for_shortcut_key(egui::Key::W),
            Some(EditorTransformTool::Move)
        );
        assert_eq!(
            transform_tool_for_shortcut_key(egui::Key::E),
            Some(EditorTransformTool::Rotate)
        );
        assert_eq!(
            transform_tool_for_shortcut_key(egui::Key::R),
            Some(EditorTransformTool::Scale)
        );
    }
}
