//! egui rendering for [`EditorShell`].
//!
//! Call [`draw_shell`] once per frame inside an egui context.

#![allow(deprecated)] // egui 0.34 keeps Panel::show(ctx) available.

use egui::{
    Align2, Color32, CornerRadius, DragValue, FontId, Frame, Margin, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Vec2,
};

use crate::{asset_guid_label, resource_kind_label, EditorShell};
use engine_assets::{AssetGuid, ResourceKind, ResourceMetaFormat};
use engine_core::{
    math::{Quat, Transform, Vec3 as EngineVec3},
    EngineResult, EntityId,
};
use engine_ecs::{
    AudioSourceComponentData, CameraComponentData, ColliderComponentData, ComponentData,
    ComponentFieldSchema, ComponentSchema, ComponentSchemaRegistry, LightComponentData,
    MaterialRef, MeshRendererComponentData, RigidbodyComponentData,
};
use engine_editor::{CommandAvailability, ConsoleLevel, EditorCommand, UndoCommand};
use engine_i18n::Translations;
use engine_render::{
    RenderCamera, RenderLight, RenderObject, RenderTargetDesc, RenderWorld, ViewKind,
};
use std::{fs, path::PathBuf};

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Copy)]
struct InfernuxPalette {
    text: Color32,
    text_dim: Color32,
    text_disabled: Color32,
    window_bg: Color32,
    panel_bg: Color32,
    menu_bar: Color32,
    status_bar: Color32,
    viewport_bg: Color32,
    frame_bg: Color32,
    frame_hover: Color32,
    header: Color32,
    header_hover: Color32,
    border: Color32,
    row_alt: Color32,
    selection: Color32,
    accent: Color32,
    play: Color32,
    pause: Color32,
    warning: Color32,
    error: Color32,
}

impl InfernuxPalette {
    const fn dark() -> Self {
        Self {
            text: Color32::from_rgb(214, 214, 214),
            text_dim: Color32::from_rgb(140, 140, 140),
            text_disabled: Color32::from_rgb(102, 102, 102),
            window_bg: Color32::from_rgb(56, 56, 56),
            panel_bg: Color32::from_rgb(54, 54, 54),
            menu_bar: Color32::from_rgb(41, 41, 41),
            status_bar: Color32::from_rgb(33, 33, 33),
            viewport_bg: Color32::from_rgb(31, 31, 31),
            frame_bg: Color32::from_rgb(42, 42, 42),
            frame_hover: Color32::from_rgb(51, 45, 45),
            header: Color32::from_rgb(60, 60, 60),
            header_hover: Color32::from_rgb(71, 61, 61),
            border: Color32::from_rgb(26, 26, 26),
            row_alt: Color32::from_rgba_premultiplied(0, 0, 0, 22),
            selection: Color32::from_rgb(44, 93, 135),
            accent: Color32::from_rgb(235, 87, 87),
            play: Color32::from_rgb(51, 115, 77),
            pause: Color32::from_rgb(128, 102, 38),
            warning: Color32::from_rgb(227, 181, 77),
            error: Color32::from_rgb(235, 87, 87),
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
    /// Last Project panel import or rescan status.
    pub project_import_status: Option<String>,
    /// Scene object IDs selected in Hierarchy.
    pub hierarchy_selection: Vec<EntityId>,
    /// Dragged hierarchy object, if any.
    pub hierarchy_dragging: Option<EntityId>,
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
            dragged_asset: None,
            scene_view_target: None,
            game_view_target: None,
            runtime_game_world: None,
            play_mode_request: None,
            command_palette_open: false,
            command_filter: String::new(),
            command_status: None,
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

/// UI-side render target request produced by Scene View and Game View panels.
#[derive(Clone, Debug)]
pub struct ViewportTargetState {
    /// Target descriptor to allocate in the renderer backend.
    pub desc: RenderTargetDesc,
    /// Render-world data extracted for this view.
    pub world: RenderWorld,
}

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
    handle_command_shortcuts(ctx, shell, ui_state);

    let close = false;

    egui::TopBottomPanel::top("infernux_menu_bar")
        .exact_size(26.0)
        .frame(
            Frame::NONE
                .fill(pal.menu_bar)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ctx, |ui| draw_menu_bar(ui, shell, ui_state, &pal, &tr));

    egui::TopBottomPanel::top("infernux_toolbar")
        .exact_size(36.0)
        .frame(
            Frame::NONE
                .fill(pal.panel_bg)
                .inner_margin(Margin::symmetric(6, 3)),
        )
        .show(ctx, |ui| draw_toolbar(ui, shell, ui_state, &pal, &tr));

    egui::TopBottomPanel::bottom("infernux_status_bar")
        .exact_size(24.0)
        .frame(
            Frame::NONE
                .fill(pal.status_bar)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ctx, |ui| draw_status_bar(ui, shell, ui_state, &pal, &tr));

    if ui_state.show_hierarchy {
        egui::SidePanel::left("infernux_hierarchy")
            .default_size(260.0)
            .min_width(180.0)
            .frame(panel_frame(&pal))
            .show(ctx, |ui| {
                panel_title(ui, tr.tr("panel_hierarchy"), &pal);
                draw_hierarchy(ui, shell, ui_state, &pal, &tr);
            });
    }

    if ui_state.show_inspector {
        egui::SidePanel::right("infernux_inspector")
            .default_size(330.0)
            .min_width(240.0)
            .frame(panel_frame(&pal))
            .show(ctx, |ui| {
                panel_title(ui, tr.tr("panel_inspector"), &pal);
                draw_inspector(ui, shell, ui_state, &pal, &tr);
            });
    }

    if ui_state.show_project || ui_state.show_console {
        egui::TopBottomPanel::bottom("infernux_bottom_dock")
            .default_height(230.0)
            .min_height(120.0)
            .frame(panel_frame(&pal))
            .show(ctx, |ui| draw_bottom_dock(ui, shell, ui_state, &pal, &tr));
    }

    egui::CentralPanel::default()
        .frame(Frame::NONE.fill(pal.window_bg))
        .show(ctx, |ui| draw_center_dock(ui, shell, ui_state, &pal, &tr));

    if ui_state.command_palette_open {
        draw_command_palette(ctx, shell, ui_state, &pal);
    }

    close
}

fn draw_menu_bar(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    ui.horizontal_centered(|ui| {
        command_menu(ui, shell, ui_state, "File", tr.tr("menu_file"), 54.0, pal);
        command_menu(ui, shell, ui_state, "Edit", tr.tr("menu_edit"), 54.0, pal);
        command_menu(
            ui,
            shell,
            ui_state,
            "Assets",
            tr.tr("menu_assets"),
            54.0,
            pal,
        );
        ghost_button(ui, tr.tr("menu_gameobject"), 86.0, pal);
        ghost_button(ui, tr.tr("menu_component"), 86.0, pal);
        command_menu(
            ui,
            shell,
            ui_state,
            "Window",
            tr.tr("menu_window"),
            64.0,
            pal,
        );
        ghost_button(ui, tr.tr("menu_help"), 54.0, pal);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(tr.tr("editor_brand"))
                    .size(12.0)
                    .color(pal.text_dim),
            );
            let title = shell
                .project()
                .map(|project| project.name().to_owned())
                .unwrap_or_else(|| tr.tr("editor_untitled").to_owned());
            ui.label(RichText::new(title).size(12.0).color(pal.text));
            if ui_state.playing {
                ui.label(RichText::new("PLAY").size(11.0).strong().color(pal.play));
            }
        });
    });
}

fn draw_toolbar(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    ui.horizontal_centered(|ui| {
        tool_button(ui, "Q", tr.tr("tool_view"), false, pal);
        tool_button(ui, "W", tr.tr("tool_move"), true, pal);
        tool_button(ui, "E", tr.tr("tool_rotate"), false, pal);
        tool_button(ui, "R", tr.tr("tool_scale"), false, pal);
        ui.add_space(10.0);
        dropdown_pill(ui, tr.tr("tool_global"), 76.0, pal);
        dropdown_pill(ui, tr.tr("tool_pivot"), 68.0, pal);

        ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                if transport_command_button(ui, shell, ui_state, "play.toggle", "▶", pal.play, pal)
                    .clicked()
                {
                    execute_shell_command(shell, ui_state, "play.toggle");
                }
                if transport_command_button(ui, shell, ui_state, "play.pause", "⏸", pal.pause, pal)
                    .clicked()
                {
                    execute_shell_command(shell, ui_state, "play.pause");
                }
                if transport_command_button(ui, shell, ui_state, "play.stop", "■", pal.accent, pal)
                    .clicked()
                {
                    execute_shell_command(shell, ui_state, "play.stop");
                }
            },
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            command_text_button(ui, shell, ui_state, "scene.save", tr.tr("tool_save"), pal);
            command_text_button(ui, shell, ui_state, "edit.redo", "Redo", pal);
            command_text_button(ui, shell, ui_state, "edit.undo", "Undo", pal);
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
                tr.tr("panel_hierarchy"),
                &mut ui_state.show_hierarchy,
                pal,
            );
        });
    });
}

fn command_menu(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    category: &str,
    label: &str,
    width: f32,
    pal: &InfernuxPalette,
) {
    let commands = shell
        .commands()
        .commands()
        .filter(|command| command.category == category)
        .cloned()
        .collect::<Vec<_>>();
    if commands.is_empty() {
        ghost_button(ui, label, width, pal);
        return;
    }

    ui.menu_button(RichText::new(label).size(12.0).color(pal.text), |ui| {
        for command in &commands {
            command_menu_item(ui, shell, ui_state, command);
        }
    });
}

fn command_menu_item(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    command: &EditorCommand,
) {
    let enabled = command_enabled(shell, ui_state, command);
    let text = match command.shortcut.as_deref() {
        Some(shortcut) => format!("{}\t{}", command.label, shortcut),
        None => command.label.clone(),
    };
    if ui
        .add_enabled(enabled, egui::Button::new(text).frame(false))
        .clicked()
    {
        execute_shell_command(shell, ui_state, &command.id);
        ui.close();
    }
}

fn command_text_button(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    command_id: &str,
    fallback_label: &str,
    pal: &InfernuxPalette,
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
    if ui
        .add_enabled(enabled, small_text_button_widget(label, pal))
        .clicked()
    {
        execute_shell_command(shell, ui_state, command_id);
    }
}

fn transport_command_button(
    ui: &mut egui::Ui,
    shell: &EditorShell,
    ui_state: &ShellUiState,
    command_id: &str,
    label: &str,
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
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(14.0).color(pal.text))
            .fill(if active { active_color } else { pal.frame_bg })
            .min_size(Vec2::new(30.0, 24.0)),
    )
}

fn command_enabled(shell: &EditorShell, ui_state: &ShellUiState, command: &EditorCommand) -> bool {
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

fn execute_shell_command(shell: &mut EditorShell, ui_state: &mut ShellUiState, command_id: &str) {
    let Some(command) = shell.commands().get(command_id).cloned() else {
        return;
    };
    if !command_enabled(shell, ui_state, &command) {
        ui_state.command_status = Some(format!("{} is not available", command.label));
        return;
    }

    match command_id {
        "project.open" => {
            ui_state.command_status = Some("Open Project is handled by the host shell".to_owned());
        }
        "scene.save" => save_scene(shell),
        "project.build" => {
            push_info(
                shell,
                "Build requested; host packaging hook is not connected yet",
            );
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
            Some(Ok(())) => push_info(shell, "Assets reloaded"),
            Some(Err(error)) => push_error(shell, error.to_string()),
            None => {}
        },
        "layout.reset" => {
            *ui_state = ShellUiState::all_open();
        }
        "command.palette" => {
            ui_state.command_palette_open = true;
        }
        _ => {}
    }
    ui_state.command_status = Some(format!("Ran {}", command.label));
}

fn draw_status_bar(
    ui: &mut egui::Ui,
    shell: &EditorShell,
    ui_state: &ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    ui.horizontal_centered(|ui| {
        let status = if ui_state.playing {
            tr.tr("status_play_mode")
        } else if shell.project().is_some() {
            tr.tr("status_ready")
        } else {
            tr.tr("status_no_project")
        };
        ui.label(RichText::new(status).size(11.0).color(pal.text_dim));
        ui.separator();
        ui.label(
            RichText::new(tr.tr_fmt(
                "status_console_count",
                &[&shell.console().entries().len().to_string()],
            ))
            .size(11.0)
            .color(pal.text_dim),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let rect = ui
                .allocate_exact_size(Vec2::new(180.0, 5.0), Sense::hover())
                .0;
            ui.painter()
                .rect_filled(rect, CornerRadius::same(0), pal.frame_bg);
            ui.painter().rect_filled(
                Rect::from_min_size(rect.min, Vec2::new(rect.width() * 0.35, rect.height())),
                CornerRadius::same(0),
                pal.accent,
            );
            ui.label(
                RichText::new(tr.tr("status_asset_indexing"))
                    .size(11.0)
                    .color(pal.text_dim),
            );
        });
    });
}

fn draw_center_dock(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    if ui_state.show_scene_view && ui_state.show_game_view {
        ui.columns(2, |columns| {
            viewport_panel(
                &mut columns[0],
                shell,
                tr.tr("viewport_scene"),
                true,
                ui_state,
                pal,
                tr,
            );
            viewport_panel(
                &mut columns[1],
                shell,
                tr.tr("viewport_game"),
                false,
                ui_state,
                pal,
                tr,
            );
        });
    } else if ui_state.show_scene_view {
        viewport_panel(ui, shell, tr.tr("viewport_scene"), true, ui_state, pal, tr);
    } else if ui_state.show_game_view {
        viewport_panel(ui, shell, tr.tr("viewport_game"), false, ui_state, pal, tr);
    } else {
        empty_view(ui, tr.tr("viewport_no_viewport"), pal);
    }
}

fn draw_bottom_dock(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    ui.horizontal(|ui| {
        if ui_state.show_project {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() * if ui_state.show_console { 0.5 } else { 1.0 });
                panel_title(ui, tr.tr("panel_project"), pal);
                draw_project_panel(ui, shell, ui_state, pal, tr);
            });
        }
        if ui_state.show_console {
            ui.vertical(|ui| {
                panel_title(ui, tr.tr("panel_console"), pal);
                draw_console(ui, shell, ui_state, pal, tr);
            });
        }
    });
}

fn draw_command_palette(
    ctx: &egui::Context,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
) {
    let mut open = ui_state.command_palette_open;
    egui::Window::new("Command Palette")
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.add_sized(
                Vec2::new(ui.available_width(), 24.0),
                egui::TextEdit::singleline(&mut ui_state.command_filter)
                    .hint_text("Search commands")
                    .font(FontId::proportional(13.0)),
            );
            ui.add_space(6.0);
            let query = ui_state.command_filter.trim().to_lowercase();
            let commands = shell
                .commands()
                .commands()
                .filter(|command| {
                    query.is_empty()
                        || command.label.to_lowercase().contains(&query)
                        || command.id.to_lowercase().contains(&query)
                        || command.category.to_lowercase().contains(&query)
                })
                .cloned()
                .collect::<Vec<_>>();
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for command in &commands {
                        let enabled = command_enabled(shell, ui_state, command);
                        let shortcut = command.shortcut.as_deref().unwrap_or("");
                        let text = if shortcut.is_empty() {
                            format!("{}  /  {}", command.label, command.category)
                        } else {
                            format!(
                                "{}  /  {}  /  {}",
                                command.label, command.category, shortcut
                            )
                        };
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(RichText::new(text).size(12.0))
                                    .fill(pal.frame_bg)
                                    .min_size(Vec2::new(ui.available_width(), 24.0)),
                            )
                            .clicked()
                        {
                            execute_shell_command(shell, ui_state, &command.id);
                            ui_state.command_palette_open = false;
                        }
                    }
                });
            if let Some(status) = &ui_state.command_status {
                ui.label(RichText::new(status).size(11.0).color(pal.text_dim));
            }
        });
    ui_state.command_palette_open = open && ui_state.command_palette_open;
}

fn handle_command_shortcuts(
    ctx: &egui::Context,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
) {
    let matched = ctx.input(|input| {
        if input.key_pressed(egui::Key::K) && input.modifiers.command && input.modifiers.shift {
            Some("command.palette")
        } else if input.key_pressed(egui::Key::S) && input.modifiers.command {
            Some("scene.save")
        } else if input.key_pressed(egui::Key::O) && input.modifiers.command {
            Some("project.open")
        } else if input.key_pressed(egui::Key::B) && input.modifiers.command {
            Some("project.build")
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
        execute_shell_command(shell, ui_state, command_id);
    }
}

fn viewport_panel(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    label: &str,
    scene_tools: bool,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), pal.viewport_bg);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(1.0, pal.border),
        StrokeKind::Inside,
    );

    let tab = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 26.0));
    ui.painter()
        .rect_filled(tab, CornerRadius::same(0), pal.header);
    paint_text_in_rect(
        ui,
        tab.shrink2(Vec2::new(10.0, 0.0)),
        label,
        FontId::proportional(13.0),
        pal.text,
        Align2::LEFT_CENTER,
    );

    let content_rect = rect.shrink2(Vec2::new(0.0, 26.0));
    draw_render_viewport(ui, shell, ui_state, content_rect, scene_tools, pal, tr);

    if scene_tools {
        draw_scene_overlay(ui, rect, pal);
        draw_orientation_gizmo(ui, rect, pal);
    }

    if ui_state.playing || ui_state.paused {
        let color = if ui_state.paused { pal.pause } else { pal.play };
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            CornerRadius::same(0),
            Stroke::new(2.0, color),
            StrokeKind::Inside,
        );
    }

    if response.clicked() {
        select_first_scene_object(shell);
    }
}

fn draw_render_viewport(
    ui: &mut egui::Ui,
    shell: &EditorShell,
    ui_state: &mut ShellUiState,
    rect: Rect,
    scene_tools: bool,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    if shell.project().is_none() {
        draw_viewport_hint(ui, rect, tr.tr("viewport_hint_open"), pal);
        return;
    }

    let world = if scene_tools {
        extract_render_world(shell, true)
    } else {
        ui_state
            .runtime_game_world
            .clone()
            .unwrap_or_else(|| extract_render_world(shell, false))
    };
    let width = rect.width().round().max(1.0) as u32;
    let height = rect.height().round().max(1.0) as u32;
    let desc = RenderTargetDesc::view(
        width,
        height,
        if scene_tools {
            ViewKind::SceneView
        } else {
            ViewKind::GameView
        },
    );
    let state = ViewportTargetState {
        desc: desc.clone(),
        world,
    };
    if scene_tools {
        ui_state.scene_view_target = Some(state.clone());
    } else {
        ui_state.game_view_target = Some(state.clone());
    }

    if !state.world.is_visible() {
        draw_viewport_hint(ui, rect, tr.tr("viewport_hint_empty"), pal);
    } else {
        paint_render_target_placeholder(ui, rect, &state, scene_tools, pal);
    }

    let stats = format!(
        "{} target: {}x{} | camera: {} | draws: {} | lights: {}",
        if scene_tools { "Scene" } else { "Game" },
        desc.width,
        desc.height,
        state
            .world
            .camera
            .as_ref()
            .map(|camera| format!("{:032x}", camera.object.as_u128()))
            .unwrap_or_else(|| "none".to_owned()),
        state.world.objects.len(),
        state.world.lights.len()
    );
    paint_text_in_rect(
        ui,
        Rect::from_min_max(
            rect.left_bottom() + Vec2::new(10.0, -24.0),
            rect.right_bottom() + Vec2::new(-10.0, -2.0),
        ),
        &stats,
        FontId::proportional(11.0),
        pal.text_dim,
        Align2::LEFT_CENTER,
    );
}

fn paint_render_target_placeholder(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &ViewportTargetState,
    scene_tools: bool,
    pal: &InfernuxPalette,
) {
    let top = rgb(26, 28, 30);
    let bottom = if scene_tools {
        rgb(35, 39, 42)
    } else {
        rgb(22, 25, 31)
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), bottom);
    ui.painter().rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.center().y)),
        CornerRadius::same(0),
        top,
    );
    let horizon = rect.center().y + rect.height() * 0.12;
    ui.painter().line_segment(
        [
            Pos2::new(rect.left(), horizon),
            Pos2::new(rect.right(), horizon),
        ],
        Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 32)),
    );
    for (idx, object) in state.world.objects.iter().enumerate() {
        let x = rect.left() + rect.width() * (0.2 + (idx as f32 * 0.19) % 0.62);
        let y = horizon - object.transform.translation.y * 7.0;
        let h = (26.0_f32 + object.transform.scale.y.abs() * 24.0_f32).clamp(18.0, 80.0);
        let w = (24.0_f32 + object.transform.scale.x.abs() * 20.0_f32).clamp(18.0, 70.0);
        let mesh_rect = Rect::from_center_size(Pos2::new(x, y - h * 0.5), Vec2::new(w, h));
        ui.painter()
            .rect_filled(mesh_rect, CornerRadius::same(2), pal.accent);
        ui.painter().rect_stroke(
            mesh_rect,
            CornerRadius::same(2),
            Stroke::new(1.0, pal.border),
            StrokeKind::Inside,
        );
    }
    for light in &state.world.lights {
        let x = rect.center().x + light.transform.translation.x * 12.0;
        let y = rect.top() + 52.0 + light.transform.translation.y.abs() * 4.0;
        ui.painter()
            .circle_filled(Pos2::new(x, y), 7.0, pal.warning);
    }
}

fn draw_viewport_hint(ui: &mut egui::Ui, rect: Rect, hint: &str, pal: &InfernuxPalette) {
    paint_wrapped_text_in_rect(
        ui,
        rect.shrink(16.0),
        hint,
        FontId::proportional(14.0),
        pal.text_disabled,
        Align2::CENTER_CENTER,
    );
}

fn draw_scene_overlay(ui: &mut egui::Ui, rect: Rect, pal: &InfernuxPalette) {
    let mut cursor = rect.min + Vec2::new(8.0, 34.0);
    for (label, active) in [("Q", false), ("W", true), ("E", false), ("R", false)] {
        let button = Rect::from_min_size(cursor, Vec2::splat(22.0));
        let fill = if active {
            pal.selection
        } else {
            Color32::from_rgba_premultiplied(20, 20, 20, 210)
        };
        ui.painter()
            .rect_filled(button, CornerRadius::same(0), fill);
        ui.painter().rect_stroke(
            button,
            CornerRadius::same(0),
            Stroke::new(1.0, pal.border),
            StrokeKind::Inside,
        );
        paint_text_in_rect(
            ui,
            button,
            label,
            FontId::proportional(12.0),
            pal.text,
            Align2::CENTER_CENTER,
        );
        cursor.x += 23.0;
    }

    let pill = Rect::from_min_size(cursor + Vec2::new(8.0, 0.0), Vec2::new(86.0, 22.0));
    ui.painter().rect_filled(
        pill,
        CornerRadius::same(4),
        Color32::from_rgba_premultiplied(35, 35, 35, 220),
    );
    paint_text_in_rect(
        ui,
        pill.shrink2(Vec2::new(6.0, 0.0)),
        "Global",
        FontId::proportional(12.0),
        pal.text,
        Align2::CENTER_CENTER,
    );
}

fn draw_orientation_gizmo(ui: &mut egui::Ui, rect: Rect, pal: &InfernuxPalette) {
    let center = rect.right_top() + Vec2::new(-54.0, 62.0);
    ui.painter().circle_filled(
        center,
        40.0,
        Color32::from_rgba_premultiplied(20, 20, 20, 150),
    );
    for (offset, color, label) in [
        (Vec2::new(26.0, 8.0), rgb(220, 70, 70), "X"),
        (Vec2::new(-8.0, -26.0), rgb(95, 190, 95), "Y"),
        (Vec2::new(-20.0, 18.0), rgb(80, 130, 220), "Z"),
    ] {
        ui.painter()
            .line_segment([center, center + offset], Stroke::new(2.0, color));
        ui.painter().circle_filled(center + offset, 7.0, color);
        paint_text_in_rect(
            ui,
            Rect::from_center_size(center + offset, Vec2::splat(14.0)),
            label,
            FontId::proportional(10.0),
            Color32::WHITE,
            Align2::CENTER_CENTER,
        );
    }
    ui.painter()
        .circle_stroke(center, 40.0, Stroke::new(1.0, pal.border));
}

fn draw_hierarchy(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    let mut create_error = None;
    toolbar_row(ui, pal, |ui| {
        if small_chip(ui, "+", 24.0, pal)
            .on_hover_text(tr.tr("hierarchy_create_hint"))
            .clicked()
        {
            if let Some(project) = shell.project_mut() {
                let name = format!("GameObject {}", project.scene.objects().len() + 1);
                match project.scene.create_object(name) {
                    Ok(entity) => {
                        project.scene_dirty = true;
                        if let Some(id) = project.scene.object(entity).map(|object| object.id) {
                            shell.select_entity_id(id);
                        }
                    }
                    Err(error) => create_error = Some(error.to_string()),
                }
            }
        }
        ui.add_space(4.0);
        search_field(
            ui,
            tr.tr("hierarchy_search"),
            &mut ui_state.hierarchy_filter,
            pal,
        );
    });
    if let Some(error) = create_error {
        shell.console_mut().push(engine_editor::ConsoleEntry {
            timestamp: "now".to_string(),
            level: engine_editor::ConsoleLevel::Error,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".to_string(),
                file: None,
                line: None,
            },
            message: error,
        });
    }

    let Some(project) = shell.project() else {
        empty_view(ui, tr.tr("hierarchy_no_scene"), pal);
        return;
    };

    let query = ui_state.hierarchy_filter.trim().to_lowercase();
    let roots = project.scene.transforms().roots().to_vec();
    let selected = shell.selected_entity_id();

    egui::ScrollArea::vertical()
        .id_salt("infernux_hierarchy_scroll")
        .show(ui, |ui| {
            row_label(ui, tr.tr("hierarchy_sample_scene"), false, 0, pal, || {});
            let mut row_index = 0usize;
            for entity in roots {
                draw_hierarchy_entity(
                    ui,
                    shell,
                    ui_state,
                    entity,
                    0,
                    &query,
                    selected,
                    &mut row_index,
                    pal,
                );
            }
        });
}

fn draw_hierarchy_entity(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    entity: engine_ecs::Entity,
    depth: usize,
    query: &str,
    selected: Option<EntityId>,
    row_index: &mut usize,
    pal: &InfernuxPalette,
) {
    let Some(project) = shell.project() else {
        return;
    };
    let Some(object) = project.scene.object(entity) else {
        return;
    };
    let id = object.id;
    let name = object.name.clone();
    let active = object.active;
    let children = project.scene.transforms().children(entity);
    let matches_query = query.is_empty() || name.to_lowercase().contains(query);

    if matches_query {
        let row_text = format!(
            "{}{} {}",
            "  ".repeat(depth),
            if children.is_empty() { " " } else { "▾" },
            if active {
                name.clone()
            } else {
                format!("{name} (inactive)")
            }
        );
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 22.0),
            Sense::click_and_drag(),
        );
        if selected == Some(id) || ui_state.hierarchy_selection.contains(&id) {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(0), pal.selection);
        } else if *row_index % 2 == 0 {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(0), pal.row_alt);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(0), pal.header_hover);
        }
        paint_text_in_rect(
            ui,
            rect.shrink2(Vec2::new(8.0, 0.0)),
            &row_text,
            FontId::proportional(12.0),
            pal.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            let additive = ui.input(|input| input.modifiers.command || input.modifiers.shift);
            if additive {
                if ui_state.hierarchy_selection.contains(&id) {
                    ui_state
                        .hierarchy_selection
                        .retain(|candidate| *candidate != id);
                } else {
                    ui_state.hierarchy_selection.push(id);
                }
            } else {
                ui_state.hierarchy_selection.clear();
                ui_state.hierarchy_selection.push(id);
            }
            shell.select_entity_id(id);
        }
        if response.drag_started() {
            ui_state.hierarchy_dragging = Some(id);
        }
        if response.hovered()
            && ui.input(|input| input.pointer.any_released())
            && ui_state.hierarchy_dragging.is_some()
            && ui_state.hierarchy_dragging != Some(id)
        {
            if let Some(child_id) = ui_state.hierarchy_dragging.take() {
                reparent_object(shell, child_id, Some(id));
            }
        }
        response.context_menu(|ui| {
            if ui.button("Duplicate").clicked() {
                duplicate_object(shell, id);
                ui.close();
            }
            if ui.button("Delete").clicked() {
                delete_object(shell, id);
                ui.close();
            }
            if ui.button("Clear Parent").clicked() {
                reparent_object(shell, id, None);
                ui.close();
            }
        });
        *row_index += 1;
    }

    for child in children {
        draw_hierarchy_entity(
            ui,
            shell,
            ui_state,
            child,
            depth + 1,
            query,
            selected,
            row_index,
            pal,
        );
    }
}

fn draw_inspector(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    let Some(selected_id) = shell.selected_entity_id() else {
        empty_view(ui, tr.tr("inspector_select_hint"), pal);
        return;
    };
    let before = scene_snapshot(shell);
    let Some(project) = shell.project_mut() else {
        empty_view(ui, tr.tr("inspector_no_project"), pal);
        return;
    };
    let Some(entity) = project.scene.find_by_id(selected_id) else {
        empty_view(ui, tr.tr("inspector_gone"), pal);
        return;
    };

    let mut errors = Vec::new();
    let mut undo_to_push = None;
    egui::ScrollArea::vertical()
        .id_salt("infernux_inspector_scroll")
        .show(ui, |ui| {
            let mut dirty = false;
            if let Some(object) = project.scene.object_mut(entity) {
                ui.horizontal(|ui| {
                    dirty |= ui
                        .add(egui::Checkbox::without_text(&mut object.active))
                        .changed();
                    dirty |= ui
                        .add_sized(
                            Vec2::new(ui.available_width(), 22.0),
                            egui::TextEdit::singleline(&mut object.name),
                        )
                        .changed();
                });
                ui.add_space(6.0);
                dirty |= string_property_row(ui, tr.tr("inspector_tag"), &mut object.tag, pal);
                ui.horizontal(|ui| {
                    ui.add_sized(
                        Vec2::new(86.0, 20.0),
                        egui::Label::new(
                            RichText::new(tr.tr("inspector_layer"))
                                .size(12.0)
                                .color(pal.text_dim),
                        ),
                    );
                    dirty |= ui
                        .add_sized(Vec2::new(80.0, 20.0), DragValue::new(&mut object.layer))
                        .changed();
                });
                ui.add_space(8.0);
            }

            if let Some(mut transform) = project.scene.transforms().local(entity) {
                component_header(ui, tr.tr("inspector_transform"), true, pal);
                let transform_before = transform;
                dirty |= vec3_editor(
                    ui,
                    tr.tr("inspector_position"),
                    &mut transform.translation,
                    pal,
                );
                let mut rotation = EngineVec3::new(
                    transform.rotation.x,
                    transform.rotation.y,
                    transform.rotation.z,
                );
                dirty |= vec3_editor(ui, tr.tr("inspector_rotation"), &mut rotation, pal);
                let mut rotation_w = transform.rotation.w;
                ui.horizontal(|ui| {
                    ui.add_sized(
                        Vec2::new(82.0, 20.0),
                        egui::Label::new(
                            RichText::new("Rotation W").size(12.0).color(pal.text_dim),
                        ),
                    );
                    dirty |= axis_drag(ui, "W", &mut rotation_w, rgb(180, 180, 180));
                });
                transform.rotation = Quat {
                    x: rotation.x,
                    y: rotation.y,
                    z: rotation.z,
                    w: rotation_w,
                };
                dirty |= vec3_editor(ui, tr.tr("inspector_scale"), &mut transform.scale, pal);
                if transform != transform_before {
                    project.scene.transforms_mut().set_local(entity, transform);
                }
            }

            let components = project.scene.components(entity).unwrap_or(&[]).to_vec();
            let assets = project.assets.clone();
            let mut remove_component = None;
            for mut component in components {
                let changed = draw_component_editor(ui, &mut component, &assets, pal, tr);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    if small_chip(ui, "Remove", 68.0, pal).clicked() {
                        remove_component = Some(component.type_id().to_owned());
                    }
                });
                if changed {
                    if let Err(error) = project.scene.upsert_component(entity, component) {
                        errors.push(error.to_string());
                    } else {
                        dirty = true;
                    }
                }
            }
            if let Some(component_type) = remove_component {
                if let Err(error) = project.scene.remove_component(entity, &component_type) {
                    errors.push(error.to_string());
                } else {
                    dirty = true;
                }
            }

            ui.add_space(8.0);
            egui::ComboBox::from_id_salt("inspector_add_component")
                .selected_text(tr.tr("inspector_add_component"))
                .show_ui(ui, |ui| {
                    for (label, component) in default_components() {
                        if ui.selectable_label(false, label).clicked() {
                            if let Err(error) = project.scene.upsert_component(entity, component) {
                                errors.push(error.to_string());
                            } else {
                                dirty = true;
                            }
                            ui.close();
                        }
                    }
                });

            if let Some(asset_guid) = ui_state.dragged_asset.take() {
                if assign_asset_to_object(project, entity, asset_guid) {
                    dirty = true;
                }
            }

            if dirty {
                project.scene_dirty = true;
                if let (Some(before), Ok(after)) =
                    (before.as_ref(), project.scene.to_json("Inspector"))
                {
                    undo_to_push = Some(UndoCommand::new(
                        "Inspector Edit",
                        format!("{:032x}", selected_id.as_u128()),
                        before.clone(),
                        after,
                    ));
                }
            }
        });
    for error in errors {
        push_error(shell, error);
    }
    if let Some(command) = undo_to_push {
        shell.push_undo(command);
    }
}

fn draw_component_editor(
    ui: &mut egui::Ui,
    component: &mut ComponentData,
    assets: &[ResourceMetaFormat],
    pal: &InfernuxPalette,
    tr: &Translations,
) -> bool {
    let registry = ComponentSchemaRegistry::builtin();
    let schema = registry.get(component.type_id());
    let title = schema
        .map(|schema| schema.display_name.as_str())
        .unwrap_or_else(|| component.type_id());
    component_header(ui, title, true, pal);
    if let Some(schema) = schema {
        return draw_component_schema_fields(ui, component, schema, assets, pal, tr);
    }

    let mut dirty = false;
    match component {
        ComponentData::Camera(camera) => {
            dirty |= f32_property_row(
                ui,
                tr.tr("property_fov"),
                &mut camera.vertical_fov_degrees,
                pal,
            );
            dirty |= f32_property_row(ui, tr.tr("property_near"), &mut camera.near, pal);
            dirty |= f32_property_row(ui, "Far", &mut camera.far, pal);
            dirty |= bool_property_row(ui, "Primary", &mut camera.primary, pal);
        }
        ComponentData::MeshRenderer(renderer) => {
            dirty |= asset_ref_row(
                ui,
                tr.tr("property_mesh"),
                &mut renderer.mesh,
                &mut renderer.builtin_mesh,
                assets,
                &[ResourceKind::Model, ResourceKind::SkinnedModel],
                pal,
            );
            dirty |= material_ref_row(
                ui,
                tr.tr("property_material"),
                &mut renderer.material,
                assets,
                pal,
            );
            dirty |= bool_property_row(ui, "Casts Shadows", &mut renderer.casts_shadows, pal);
        }
        ComponentData::Light(light) => {
            dirty |= enum_property_row(
                ui,
                tr.tr("property_type"),
                &mut light.kind,
                &["directional", "point", "spot"],
                pal,
            );
            dirty |= vec3_editor(ui, "Color", &mut light.color, pal);
            dirty |= f32_property_row(ui, tr.tr("property_intensity"), &mut light.intensity, pal);
        }
        ComponentData::Rigidbody(body) => {
            dirty |= enum_property_row(
                ui,
                tr.tr("property_body_type"),
                &mut body.body_type,
                &["dynamic", "kinematic", "static"],
                pal,
            );
            dirty |= f32_property_row(ui, tr.tr("property_mass"), &mut body.mass, pal);
            dirty |= bool_property_row(ui, "Use Gravity", &mut body.use_gravity, pal);
        }
        ComponentData::Collider(collider) => {
            dirty |= enum_property_row(
                ui,
                tr.tr("property_shape"),
                &mut collider.shape,
                &["box", "sphere", "capsule"],
                pal,
            );
            dirty |= vec3_editor(ui, "Size", &mut collider.size, pal);
            dirty |= bool_property_row(ui, "Is Trigger", &mut collider.is_trigger, pal);
            dirty |= u32_property_row(ui, "Mask", &mut collider.mask, pal);
        }
        ComponentData::AudioSource(source) => {
            let mut builtin = None;
            dirty |= asset_ref_row(
                ui,
                "Clip",
                &mut source.clip,
                &mut builtin,
                assets,
                &[ResourceKind::Audio],
                pal,
            );
            dirty |= f32_property_row(ui, tr.tr("property_volume"), &mut source.volume, pal);
            dirty |= bool_property_row(ui, "Looping", &mut source.looping, pal);
            dirty |= bool_property_row(ui, "Play On Start", &mut source.play_on_start, pal);
        }
        ComponentData::Script(script) => {
            dirty |= string_property_row(ui, tr.tr("property_backend"), &mut script.backend, pal);
            dirty |=
                string_property_row(ui, tr.tr("property_script_name"), &mut script.script, pal);
            dirty |= bool_property_row(ui, "Pending Recovery", &mut script.pending_recovery, pal);
        }
    }
    dirty
}

fn draw_component_schema_fields(
    ui: &mut egui::Ui,
    component: &mut ComponentData,
    schema: &ComponentSchema,
    assets: &[ResourceMetaFormat],
    pal: &InfernuxPalette,
    tr: &Translations,
) -> bool {
    let mut dirty = false;
    for field in &schema.fields {
        dirty |= draw_component_schema_field(ui, component, field, assets, pal, tr);
    }
    dirty
}

fn draw_component_schema_field(
    ui: &mut egui::Ui,
    component: &mut ComponentData,
    field: &ComponentFieldSchema,
    assets: &[ResourceMetaFormat],
    pal: &InfernuxPalette,
    tr: &Translations,
) -> bool {
    let label = component_field_label(field, tr);
    match component {
        ComponentData::Camera(camera) => match field.name.as_str() {
            "vertical_fov_degrees" => {
                f32_property_row(ui, &label, &mut camera.vertical_fov_degrees, pal)
            }
            "near" => f32_property_row(ui, &label, &mut camera.near, pal),
            "far" => f32_property_row(ui, &label, &mut camera.far, pal),
            "primary" => bool_property_row(ui, &label, &mut camera.primary, pal),
            _ => false,
        },
        ComponentData::MeshRenderer(renderer) => match field.name.as_str() {
            "mesh" => asset_ref_row(
                ui,
                &label,
                &mut renderer.mesh,
                &mut renderer.builtin_mesh,
                assets,
                &[ResourceKind::Model, ResourceKind::SkinnedModel],
                pal,
            ),
            "material" => material_ref_row(ui, &label, &mut renderer.material, assets, pal),
            "casts_shadows" => bool_property_row(ui, &label, &mut renderer.casts_shadows, pal),
            _ => false,
        },
        ComponentData::Light(light) => match field.name.as_str() {
            "kind" => enum_property_row(
                ui,
                &label,
                &mut light.kind,
                &["directional", "point", "spot"],
                pal,
            ),
            "color" => vec3_editor(ui, &label, &mut light.color, pal),
            "intensity" => f32_property_row(ui, &label, &mut light.intensity, pal),
            _ => false,
        },
        ComponentData::Rigidbody(body) => match field.name.as_str() {
            "body_type" => enum_property_row(
                ui,
                &label,
                &mut body.body_type,
                &["dynamic", "kinematic", "static"],
                pal,
            ),
            "mass" => f32_property_row(ui, &label, &mut body.mass, pal),
            "use_gravity" => bool_property_row(ui, &label, &mut body.use_gravity, pal),
            _ => false,
        },
        ComponentData::Collider(collider) => match field.name.as_str() {
            "shape" => enum_property_row(
                ui,
                &label,
                &mut collider.shape,
                &["box", "sphere", "capsule"],
                pal,
            ),
            "size" => vec3_editor(ui, &label, &mut collider.size, pal),
            "is_trigger" => bool_property_row(ui, &label, &mut collider.is_trigger, pal),
            "mask" => u32_property_row(ui, &label, &mut collider.mask, pal),
            _ => false,
        },
        ComponentData::AudioSource(source) => match field.name.as_str() {
            "clip" => {
                let mut builtin = None;
                asset_ref_row(
                    ui,
                    &label,
                    &mut source.clip,
                    &mut builtin,
                    assets,
                    &[ResourceKind::Audio],
                    pal,
                )
            }
            "volume" => f32_property_row(ui, &label, &mut source.volume, pal),
            "looping" => bool_property_row(ui, &label, &mut source.looping, pal),
            "play_on_start" => bool_property_row(ui, &label, &mut source.play_on_start, pal),
            _ => false,
        },
        ComponentData::Script(script) => match field.name.as_str() {
            "backend" => string_property_row(ui, &label, &mut script.backend, pal),
            "script" => string_property_row(ui, &label, &mut script.script, pal),
            "pending_recovery" => bool_property_row(ui, &label, &mut script.pending_recovery, pal),
            _ => false,
        },
    }
}

fn component_field_label(field: &ComponentFieldSchema, tr: &Translations) -> String {
    match field.name.as_str() {
        "vertical_fov_degrees" => tr.tr("property_fov").to_owned(),
        "near" => tr.tr("property_near").to_owned(),
        "mesh" => tr.tr("property_mesh").to_owned(),
        "material" => tr.tr("property_material").to_owned(),
        "kind" => tr.tr("property_type").to_owned(),
        "intensity" => tr.tr("property_intensity").to_owned(),
        "body_type" => tr.tr("property_body_type").to_owned(),
        "mass" => tr.tr("property_mass").to_owned(),
        "shape" => tr.tr("property_shape").to_owned(),
        "volume" => tr.tr("property_volume").to_owned(),
        "backend" => tr.tr("property_backend").to_owned(),
        "script" => tr.tr("property_script_name").to_owned(),
        other => title_case_field(other),
    }
}

fn title_case_field(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn draw_project_panel(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    toolbar_row(ui, pal, |ui| {
        if small_chip(ui, tr.tr("project_create"), 64.0, pal).clicked() {
            create_default_material(shell);
        }
        if small_chip(ui, "Rescan", 56.0, pal).clicked() {
            match shell.project_mut().map(|project| {
                project.rescan_assets()?;
                Ok::<_, engine_core::EngineError>(format!("scan: {} assets", project.assets.len()))
            }) {
                Some(Ok(status)) => ui_state.project_import_status = Some(status),
                Some(Err(error)) => push_error(shell, error.to_string()),
                None => {}
            }
        }
        if small_chip(ui, tr.tr("project_import"), 64.0, pal).clicked() {
            let path = PathBuf::from(ui_state.project_import_path.trim());
            if path.as_os_str().is_empty() {
                ui_state.project_import_status = Some("enter a file path to import".to_owned());
            } else {
                match shell
                    .project_mut()
                    .map(|project| project.import_file(&path))
                {
                    Some(Ok(())) => {
                        ui_state.project_import_status =
                            Some(format!("imported {}", path.display()));
                    }
                    Some(Err(error)) => push_error(shell, error.to_string()),
                    None => {}
                }
            }
        }
        ui.add_space(6.0);
        ui.add_sized(
            Vec2::new(170.0, 20.0),
            egui::TextEdit::singleline(&mut ui_state.project_import_path)
                .hint_text("file path")
                .font(FontId::proportional(11.0)),
        );
        search_field(
            ui,
            tr.tr("project_search"),
            &mut ui_state.project_filter,
            pal,
        );
    });

    for dropped in ui.input(|input| input.raw.dropped_files.clone()) {
        if let Some(path) = dropped.path {
            match shell
                .project_mut()
                .map(|project| project.import_file(&path))
            {
                Some(Ok(())) => {
                    ui_state.project_import_status = Some(format!("imported {}", path.display()))
                }
                Some(Err(error)) => push_error(shell, error.to_string()),
                None => {}
            }
        }
    }
    let Some(project) = shell.project() else {
        empty_view(ui, tr.tr("project_no_project"), pal);
        return;
    };
    let project_root = project.root.clone();
    let all_assets = project.assets.clone();
    let recent_imports = project
        .asset_imports
        .iter()
        .rev()
        .take(2)
        .cloned()
        .collect::<Vec<_>>();

    ui.add(
        egui::Label::new(
            RichText::new(project_root.display().to_string())
                .size(11.0)
                .color(pal.text_dim),
        )
        .truncate(),
    );
    ui.add_space(6.0);
    if let Some(status) = &ui_state.project_import_status {
        ui.label(RichText::new(status).size(11.0).color(pal.text_dim));
    }
    for status in &recent_imports {
        ui.label(RichText::new(status).size(11.0).color(pal.text_disabled));
    }
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("infernux_project_assets_scroll")
        .show(ui, |ui| {
            let query = ui_state.project_filter.trim().to_lowercase();
            let mut assets = all_assets.iter().collect::<Vec<_>>();
            assets.sort_by(|left, right| left.source_path.cmp(&right.source_path));
            let assets = assets
                .into_iter()
                .filter(|asset| {
                    query.is_empty()
                        || asset
                            .source_path
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&query)
                        || resource_kind_label(asset.kind, tr)
                            .to_lowercase()
                            .contains(&query)
                        || asset_guid_label(asset.guid).contains(&query)
                })
                .collect::<Vec<_>>();

            if all_assets.is_empty() {
                empty_view(ui, tr.tr("project_empty"), pal);
                return;
            }
            if assets.is_empty() {
                empty_view(ui, tr.tr("project_no_match"), pal);
                return;
            }

            let tile_size = Vec2::new(92.0, 74.0);
            ui.horizontal_wrapped(|ui| {
                for asset in assets {
                    let (rect, response) =
                        ui.allocate_exact_size(tile_size, Sense::click_and_drag());
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(0), pal.frame_bg);
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(0),
                        Stroke::new(1.0, pal.border),
                        StrokeKind::Inside,
                    );
                    paint_text_in_rect(
                        ui,
                        Rect::from_min_max(
                            rect.min + Vec2::new(6.0, 8.0),
                            rect.max - Vec2::new(6.0, 42.0),
                        ),
                        resource_kind_label(asset.kind, tr),
                        FontId::proportional(11.0),
                        pal.text_dim,
                        Align2::CENTER_CENTER,
                    );
                    let name = asset
                        .source_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("asset");
                    paint_text_in_rect(
                        ui,
                        Rect::from_min_max(
                            rect.min + Vec2::new(6.0, 40.0),
                            rect.max - Vec2::new(6.0, 18.0),
                        ),
                        name,
                        FontId::proportional(11.0),
                        pal.text,
                        Align2::CENTER_CENTER,
                    );
                    paint_text_in_rect(
                        ui,
                        Rect::from_min_max(
                            rect.min + Vec2::new(6.0, 58.0),
                            rect.max - Vec2::new(6.0, 4.0),
                        ),
                        &asset_guid_label(asset.guid),
                        FontId::proportional(9.0),
                        pal.text_disabled,
                        Align2::CENTER_CENTER,
                    );
                    let icon_rect = Rect::from_center_size(
                        rect.center_top() + Vec2::new(0.0, 20.0),
                        Vec2::new(30.0, 24.0),
                    );
                    ui.painter().rect_filled(
                        icon_rect,
                        CornerRadius::same(3),
                        thumbnail_color(asset.kind),
                    );
                    if response.clicked() {
                        shell
                            .selection_mut()
                            .select(engine_editor::Selection::Asset(asset.source_path.clone()));
                    }
                    if response.drag_started() {
                        ui_state.dragged_asset = Some(asset.guid);
                    }
                }
            });
        });
}

fn draw_console(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
) {
    toolbar_row(ui, pal, |ui| {
        if small_chip(ui, tr.tr("console_clear"), 54.0, pal).clicked() {
            shell.console_mut().clear();
        }
        if small_chip(
            ui,
            if ui_state.console_collapse {
                tr.tr("console_expanded")
            } else {
                tr.tr("console_collapse")
            },
            74.0,
            pal,
        )
        .clicked()
        {
            ui_state.console_collapse = !ui_state.console_collapse;
        }
        ui.add_space(6.0);
        search_field(
            ui,
            tr.tr("console_filter"),
            &mut ui_state.console_filter,
            pal,
        );
    });

    let query = ui_state.console_filter.trim().to_lowercase();
    let mut last_message = String::new();
    egui::ScrollArea::vertical()
        .id_salt("infernux_console_scroll")
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for (idx, entry) in shell.console().entries().iter().enumerate() {
                let row_text = format!("[{:?}] {}", entry.level, entry.message);
                if !query.is_empty() && !row_text.to_lowercase().contains(&query) {
                    continue;
                }
                if ui_state.console_collapse && row_text == last_message {
                    continue;
                }
                last_message = row_text.clone();
                let rect = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), 23.0), Sense::click())
                    .0;
                if idx % 2 == 0 {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(0), pal.row_alt);
                }
                let color = match entry.level {
                    ConsoleLevel::Trace | ConsoleLevel::Debug => pal.text_dim,
                    ConsoleLevel::Info => pal.text,
                    ConsoleLevel::Warn => pal.warning,
                    ConsoleLevel::Error => pal.error,
                };
                paint_text_in_rect(
                    ui,
                    rect.shrink2(Vec2::new(8.0, 0.0)),
                    &row_text,
                    FontId::proportional(12.0),
                    color,
                    Align2::LEFT_CENTER,
                );
            }
        });
}

fn vec3_editor(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut engine_core::math::Vec3,
    pal: &InfernuxPalette,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(82.0, 20.0),
            egui::Label::new(RichText::new(label).size(12.0).color(pal.text_dim)),
        );
        changed |= axis_drag(ui, "X", &mut value.x, rgb(190, 75, 75));
        changed |= axis_drag(ui, "Y", &mut value.y, rgb(90, 170, 90));
        changed |= axis_drag(ui, "Z", &mut value.z, rgb(80, 120, 190));
    });
    changed
}

fn axis_drag(ui: &mut egui::Ui, label: &str, value: &mut f32, color: Color32) -> bool {
    ui.label(RichText::new(label).size(11.0).strong().color(color));
    ui.add_sized(Vec2::new(58.0, 20.0), DragValue::new(value).speed(0.05))
        .changed()
}

fn string_property_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    pal: &InfernuxPalette,
) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(86.0, 20.0),
            egui::Label::new(RichText::new(label).size(12.0).color(pal.text_dim)).truncate(),
        );
        ui.add_sized(
            Vec2::new((ui.available_width() - 2.0).max(80.0), 20.0),
            egui::TextEdit::singleline(value).font(FontId::proportional(12.0)),
        )
        .changed()
    })
    .inner
}

fn f32_property_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    pal: &InfernuxPalette,
) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(86.0, 20.0),
            egui::Label::new(RichText::new(label).size(12.0).color(pal.text_dim)).truncate(),
        );
        ui.add_sized(Vec2::new(96.0, 20.0), DragValue::new(value).speed(0.05))
            .changed()
    })
    .inner
}

fn u32_property_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    pal: &InfernuxPalette,
) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(86.0, 20.0),
            egui::Label::new(RichText::new(label).size(12.0).color(pal.text_dim)).truncate(),
        );
        ui.add_sized(Vec2::new(96.0, 20.0), DragValue::new(value).speed(1.0))
            .changed()
    })
    .inner
}

fn bool_property_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut bool,
    pal: &InfernuxPalette,
) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(86.0, 20.0),
            egui::Label::new(RichText::new(label).size(12.0).color(pal.text_dim)).truncate(),
        );
        ui.checkbox(value, "").changed()
    })
    .inner
}

fn enum_property_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    options: &[&str],
    pal: &InfernuxPalette,
) -> bool {
    let before = value.clone();
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(86.0, 20.0),
            egui::Label::new(RichText::new(label).size(12.0).color(pal.text_dim)).truncate(),
        );
        egui::ComboBox::from_id_salt(format!("enum_{label}_{before}"))
            .selected_text(value.as_str())
            .show_ui(ui, |ui| {
                for option in options {
                    ui.selectable_value(value, (*option).to_owned(), *option);
                }
            });
    });
    *value != before
}

fn asset_ref_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<engine_core::AssetId>,
    builtin: &mut Option<String>,
    assets: &[ResourceMetaFormat],
    accepted: &[ResourceKind],
    pal: &InfernuxPalette,
) -> bool {
    let before = *value;
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(86.0, 20.0),
            egui::Label::new(RichText::new(label).size(12.0).color(pal.text_dim)).truncate(),
        );
        let selected = value
            .map(|id| format!("{:032x}", id.as_u128()))
            .or_else(|| builtin.clone())
            .unwrap_or_else(|| "None".to_owned());
        egui::ComboBox::from_id_salt(format!("asset_{label}_{selected}"))
            .selected_text(selected)
            .show_ui(ui, |ui| {
                if ui.selectable_label(value.is_none(), "None").clicked() {
                    *value = None;
                    *builtin = None;
                }
                for asset in assets.iter().filter(|asset| accepted.contains(&asset.kind)) {
                    let name = asset.source_path.to_string_lossy();
                    if ui
                        .selectable_label(*value == Some(asset.guid.as_asset_id()), name.as_ref())
                        .clicked()
                    {
                        *value = Some(asset.guid.as_asset_id());
                        *builtin = None;
                    }
                }
            });
    });
    *value != before
}

fn material_ref_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut MaterialRef,
    assets: &[ResourceMetaFormat],
    pal: &InfernuxPalette,
) -> bool {
    let before = value.clone();
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(86.0, 20.0),
            egui::Label::new(RichText::new(label).size(12.0).color(pal.text_dim)).truncate(),
        );
        let selected = value
            .asset
            .map(|id| format!("{:032x}", id.as_u128()))
            .or_else(|| value.builtin.clone())
            .unwrap_or_else(|| "None".to_owned());
        egui::ComboBox::from_id_salt(format!("material_{selected}"))
            .selected_text(selected)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(value.asset.is_none() && value.builtin.is_none(), "None")
                    .clicked()
                {
                    value.asset = None;
                    value.builtin = None;
                }
                if ui
                    .selectable_label(
                        value.builtin.as_deref() == Some("debug/default"),
                        "debug/default",
                    )
                    .clicked()
                {
                    value.asset = None;
                    value.builtin = Some("debug/default".to_owned());
                }
                for asset in assets
                    .iter()
                    .filter(|asset| asset.kind == ResourceKind::Material)
                {
                    let name = asset.source_path.to_string_lossy();
                    if ui
                        .selectable_label(
                            value.asset == Some(asset.guid.as_asset_id()),
                            name.as_ref(),
                        )
                        .clicked()
                    {
                        value.asset = Some(asset.guid.as_asset_id());
                        value.builtin = None;
                    }
                }
            });
    });
    *value != before
}

fn component_header(ui: &mut egui::Ui, title: &str, enabled: bool, pal: &InfernuxPalette) {
    ui.add_space(6.0);
    let rect = ui
        .allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click())
        .0;
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), pal.header);
    paint_text_in_rect(
        ui,
        Rect::from_min_max(
            rect.min + Vec2::new(8.0, 0.0),
            rect.min + Vec2::new(22.0, 24.0),
        ),
        "v",
        FontId::proportional(11.0),
        pal.text_dim,
        Align2::LEFT_CENTER,
    );
    paint_text_in_rect(
        ui,
        Rect::from_min_max(
            rect.min + Vec2::new(26.0, 0.0),
            rect.right_top() + Vec2::new(-54.0, 24.0),
        ),
        title,
        FontId::proportional(12.0),
        pal.text,
        Align2::LEFT_CENTER,
    );
    let check = Rect::from_min_size(rect.right_top() + Vec2::new(-48.0, 5.0), Vec2::splat(14.0));
    ui.painter().rect_stroke(
        check,
        CornerRadius::same(0),
        Stroke::new(1.0, pal.border),
        StrokeKind::Inside,
    );
    if enabled {
        ui.painter().line_segment(
            [check.left_center(), check.center_bottom()],
            Stroke::new(1.5, pal.accent),
        );
        ui.painter().line_segment(
            [check.center_bottom(), check.right_top()],
            Stroke::new(1.5, pal.accent),
        );
    }
    paint_text_in_rect(
        ui,
        Rect::from_center_size(
            rect.right_center() - Vec2::new(14.0, 0.0),
            Vec2::new(28.0, 20.0),
        ),
        "...",
        FontId::proportional(12.0),
        pal.text_dim,
        Align2::CENTER_CENTER,
    );
}

fn panel_title(ui: &mut egui::Ui, title: &str, pal: &InfernuxPalette) {
    let rect = ui
        .allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click())
        .0;
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), pal.header);
    paint_text_in_rect(
        ui,
        rect.shrink2(Vec2::new(8.0, 0.0)),
        title,
        FontId::proportional(13.0),
        pal.text,
        Align2::LEFT_CENTER,
    );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, pal.border),
    );
}

fn row_label(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    index: usize,
    pal: &InfernuxPalette,
    on_click: impl FnOnce(),
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(0), pal.selection);
    } else if index % 2 == 0 {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(0), pal.row_alt);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(0), pal.header_hover);
    }
    paint_text_in_rect(
        ui,
        rect.shrink2(Vec2::new(8.0, 0.0)),
        label,
        FontId::proportional(12.0),
        pal.text,
        Align2::LEFT_CENTER,
    );
    if response.clicked() {
        on_click();
    }
}

fn empty_view(ui: &mut egui::Ui, hint: &str, pal: &InfernuxPalette) {
    let rect = ui.available_rect_before_wrap().shrink(18.0);
    let w = rect.width().clamp(220.0, 460.0);
    let h = rect.height().clamp(120.0, 220.0);
    let box_rect = Rect::from_center_size(rect.center(), Vec2::new(w, h));
    ui.painter().rect_stroke(
        box_rect,
        CornerRadius::same(8),
        Stroke::new(1.0, pal.text_disabled),
        StrokeKind::Inside,
    );
    paint_wrapped_text_in_rect(
        ui,
        box_rect.shrink(16.0),
        hint,
        FontId::proportional(13.0),
        pal.text_dim,
        Align2::CENTER_CENTER,
    );
    ui.allocate_rect(ui.available_rect_before_wrap(), Sense::hover());
}

fn toolbar_row(ui: &mut egui::Ui, pal: &InfernuxPalette, add: impl FnOnce(&mut egui::Ui)) {
    let rect = ui
        .allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover())
        .0;
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), pal.menu_bar);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 3.0))),
        |ui| {
            ui.horizontal_centered(add);
        },
    );
}

fn panel_toggle(ui: &mut egui::Ui, label: &str, state: &mut bool, pal: &InfernuxPalette) {
    let fill = if *state { pal.header } else { pal.frame_bg };
    let response = ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(if *state {
            pal.text
        } else {
            pal.text_dim
        }))
        .fill(fill)
        .min_size(Vec2::new(58.0, 24.0)),
    );
    if response.clicked() {
        *state = !*state;
    }
}

fn tool_button(ui: &mut egui::Ui, label: &str, tooltip: &str, active: bool, pal: &InfernuxPalette) {
    ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(pal.text))
            .fill(if active { pal.selection } else { pal.frame_bg })
            .min_size(Vec2::new(26.0, 24.0)),
    )
    .on_hover_text(tooltip);
}

fn dropdown_pill(ui: &mut egui::Ui, label: &str, width: f32, pal: &InfernuxPalette) {
    ui.add(
        egui::Button::new(
            RichText::new(format!("{label} ▾"))
                .size(12.0)
                .color(pal.text),
        )
        .fill(pal.frame_bg)
        .min_size(Vec2::new(width, 24.0)),
    );
}

fn ghost_button(ui: &mut egui::Ui, label: &str, width: f32, pal: &InfernuxPalette) {
    ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(pal.text))
            .fill(Color32::TRANSPARENT)
            .min_size(Vec2::new(width, 22.0)),
    );
}

fn small_text_button_widget(label: &str, pal: &InfernuxPalette) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).size(12.0).color(pal.text))
        .fill(pal.frame_bg)
        .min_size(Vec2::new(56.0, 24.0))
}

fn small_chip(ui: &mut egui::Ui, label: &str, width: f32, pal: &InfernuxPalette) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(pal.text))
            .fill(pal.frame_bg)
            .min_size(Vec2::new(width, 20.0)),
    )
}

fn search_field(ui: &mut egui::Ui, hint: &str, value: &mut String, pal: &InfernuxPalette) {
    ui.add_sized(
        Vec2::new((ui.available_width() - 4.0).max(80.0), 20.0),
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .font(FontId::proportional(11.0))
            .text_color(pal.text),
    );
}

fn panel_frame(pal: &InfernuxPalette) -> Frame {
    Frame::NONE
        .fill(pal.panel_bg)
        .stroke(Stroke::new(1.0, pal.border))
        .inner_margin(Margin::same(0))
}

fn default_components() -> Vec<(&'static str, ComponentData)> {
    vec![
        (
            "Camera",
            ComponentData::Camera(CameraComponentData::default()),
        ),
        (
            "Mesh Renderer",
            ComponentData::MeshRenderer(MeshRendererComponentData::default()),
        ),
        ("Light", ComponentData::Light(LightComponentData::default())),
        (
            "Rigidbody",
            ComponentData::Rigidbody(RigidbodyComponentData::default()),
        ),
        (
            "Collider",
            ComponentData::Collider(ColliderComponentData::default()),
        ),
        (
            "Audio Source",
            ComponentData::AudioSource(AudioSourceComponentData::default()),
        ),
    ]
}

fn assign_asset_to_object(
    project: &mut crate::ProjectContext,
    entity: engine_ecs::Entity,
    guid: AssetGuid,
) -> bool {
    let Some(asset) = project
        .assets
        .iter()
        .find(|asset| asset.guid == guid)
        .cloned()
    else {
        return false;
    };
    match asset.kind {
        ResourceKind::Model | ResourceKind::SkinnedModel => {
            let mut renderer = project
                .scene
                .components(entity)
                .unwrap_or(&[])
                .iter()
                .find_map(|component| match component {
                    ComponentData::MeshRenderer(renderer) => Some(renderer.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            renderer.mesh = Some(guid.as_asset_id());
            renderer.builtin_mesh = None;
            project
                .scene
                .upsert_component(entity, ComponentData::MeshRenderer(renderer))
                .is_ok()
        }
        ResourceKind::Material => {
            let mut renderer = project
                .scene
                .components(entity)
                .unwrap_or(&[])
                .iter()
                .find_map(|component| match component {
                    ComponentData::MeshRenderer(renderer) => Some(renderer.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            renderer.material.asset = Some(guid.as_asset_id());
            renderer.material.builtin = None;
            project
                .scene
                .upsert_component(entity, ComponentData::MeshRenderer(renderer))
                .is_ok()
        }
        ResourceKind::Audio => {
            let mut source = project
                .scene
                .components(entity)
                .unwrap_or(&[])
                .iter()
                .find_map(|component| match component {
                    ComponentData::AudioSource(source) => Some(source.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            source.clip = Some(guid.as_asset_id());
            project
                .scene
                .upsert_component(entity, ComponentData::AudioSource(source))
                .is_ok()
        }
        _ => false,
    }
}

fn scene_snapshot(shell: &EditorShell) -> Option<String> {
    shell
        .project()
        .and_then(|project| project.scene.to_json("Editor").ok())
}

fn push_scene_undo(shell: &mut EditorShell, label: &str, target: String, before: Option<String>) {
    let Some(before) = before else {
        return;
    };
    if let Some(after) = scene_snapshot(shell) {
        if before != after {
            shell.push_undo(UndoCommand::new(label, target, before, after));
        }
    }
}

fn push_error(shell: &mut EditorShell, message: String) {
    shell.console_mut().push(engine_editor::ConsoleEntry {
        timestamp: "now".to_string(),
        level: engine_editor::ConsoleLevel::Error,
        source: engine_editor::ConsoleSource {
            subsystem: "editor".to_string(),
            file: None,
            line: None,
        },
        message,
    });
}

fn push_info(shell: &mut EditorShell, message: impl Into<String>) {
    shell.console_mut().push(engine_editor::ConsoleEntry {
        timestamp: "now".to_string(),
        level: engine_editor::ConsoleLevel::Info,
        source: engine_editor::ConsoleSource {
            subsystem: "editor".to_string(),
            file: None,
            line: None,
        },
        message: message.into(),
    });
}

fn apply_undo(shell: &mut EditorShell) {
    if let Err(error) = shell.undo_scene_command() {
        push_error(shell, error.to_string());
    }
}

fn apply_redo(shell: &mut EditorShell) {
    if let Err(error) = shell.redo_scene_command() {
        push_error(shell, error.to_string());
    }
}

fn reparent_object(shell: &mut EditorShell, child_id: EntityId, parent_id: Option<EntityId>) {
    let before = scene_snapshot(shell);
    let result = shell.project_mut().and_then(|project| {
        let child = project.scene.find_by_id(child_id)?;
        let parent = parent_id.and_then(|id| project.scene.find_by_id(id));
        match project.scene.set_parent(child, parent) {
            Ok(()) => {
                project.scene_dirty = true;
                Some(Ok(()))
            }
            Err(error) => Some(Err(error)),
        }
    });
    match result {
        Some(Ok(())) => push_scene_undo(
            shell,
            "Reparent Object",
            format!("{:032x}", child_id.as_u128()),
            before,
        ),
        Some(Err(error)) => push_error(shell, error.to_string()),
        None => {}
    }
}

fn duplicate_object(shell: &mut EditorShell, id: EntityId) {
    let before = scene_snapshot(shell);
    let result = shell.project_mut().and_then(|project| {
        let entity = project.scene.find_by_id(id)?;
        match project.scene.clone_object(entity) {
            Ok(clone) => {
                project.scene_dirty = true;
                project.scene.object(clone).map(|object| Ok(object.id))
            }
            Err(error) => Some(Err(error)),
        }
    });
    match result {
        Some(Ok(cloned_id)) => {
            shell.select_entity_id(cloned_id);
            push_scene_undo(
                shell,
                "Duplicate Object",
                format!("{:032x}", id.as_u128()),
                before,
            );
        }
        Some(Err(error)) => push_error(shell, error.to_string()),
        None => {}
    }
}

fn delete_object(shell: &mut EditorShell, id: EntityId) {
    let before = scene_snapshot(shell);
    let result = shell.project_mut().and_then(|project| {
        let entity = project.scene.find_by_id(id)?;
        match project
            .scene
            .destroy_deferred(entity)
            .and_then(|()| project.scene.process_deferred_destroy())
        {
            Ok(()) => {
                project.scene_dirty = true;
                Some(Ok(()))
            }
            Err(error) => Some(Err(error)),
        }
    });
    match result {
        Some(Ok(())) => {
            shell.selection_mut().clear();
            push_scene_undo(
                shell,
                "Delete Object",
                format!("{:032x}", id.as_u128()),
                before,
            );
        }
        Some(Err(error)) => push_error(shell, error.to_string()),
        None => {}
    }
}

fn create_default_material(shell: &mut EditorShell) {
    let Some(project) = shell.project_mut() else {
        return;
    };
    let asset_root = project.root.join(&project.manifest.asset_root);
    let material_dir = asset_root.join("materials");
    let material_path = material_dir.join("new_material.material.json");
    let result: EngineResult<()> = (|| {
        fs::create_dir_all(&material_dir).map_err(|source| {
            engine_core::EngineError::Filesystem {
                path: material_dir.clone(),
                source,
            }
        })?;
        if !material_path.exists() {
            fs::write(
                &material_path,
                "{\n  \"version\": 1,\n  \"shader\": \"00000000000000000000000000000000\",\n  \"textures\": {},\n  \"parameters\": {}\n}\n",
            )
            .map_err(|source| engine_core::EngineError::Filesystem {
                path: material_path.clone(),
                source,
            })?;
        }
        project.rescan_assets()
    })();
    match result {
        Ok(()) => project
            .asset_imports
            .push(format!("created {}", material_path.display())),
        Err(error) => push_error(shell, error.to_string()),
    }
}

fn thumbnail_color(kind: ResourceKind) -> Color32 {
    match kind {
        ResourceKind::Texture => rgb(91, 157, 245),
        ResourceKind::Material => rgb(235, 87, 87),
        ResourceKind::Shader => rgb(160, 130, 220),
        ResourceKind::Audio => rgb(113, 183, 139),
        ResourceKind::Model | ResourceKind::SkinnedModel => rgb(220, 167, 80),
        ResourceKind::Animation => rgb(120, 180, 190),
    }
}

fn extract_render_world(shell: &EditorShell, scene_view: bool) -> RenderWorld {
    let Some(project) = shell.project() else {
        return RenderWorld::default();
    };
    let camera_entity = if scene_view {
        project.scene.main_camera().or_else(|| {
            shell
                .selected_entity_id()
                .and_then(|id| project.scene.find_by_id(id))
        })
    } else {
        project.scene.game_camera().or_else(|| {
            project.scene.objects().into_iter().find_map(|(entity, _)| {
                project.scene.components(entity).and_then(|components| {
                    components.iter().find_map(|component| match component {
                        ComponentData::Camera(camera) if camera.primary => Some(entity),
                        _ => None,
                    })
                })
            })
        })
    };
    let camera = camera_entity.and_then(|entity| {
        let object = project.scene.object(entity)?;
        let camera = project
            .scene
            .components(entity)?
            .iter()
            .find_map(|component| {
                if let ComponentData::Camera(camera) = component {
                    Some(camera)
                } else {
                    None
                }
            })?;
        Some(RenderCamera {
            object: object.id,
            transform: project
                .scene
                .transforms()
                .local(entity)
                .unwrap_or(Transform::IDENTITY),
            vertical_fov_degrees: camera.vertical_fov_degrees,
            near: camera.near,
            far: camera.far,
        })
    });
    let mut world = RenderWorld {
        camera,
        objects: Vec::new(),
        lights: Vec::new(),
    };
    for (entity, object) in project.scene.objects() {
        if !object.active {
            continue;
        }
        let transform = project
            .scene
            .transforms()
            .local(entity)
            .unwrap_or(Transform::IDENTITY);
        for component in project.scene.components(entity).unwrap_or(&[]) {
            match component {
                ComponentData::MeshRenderer(renderer) => world.objects.push(RenderObject {
                    object: object.id,
                    transform,
                    mesh: renderer
                        .builtin_mesh
                        .clone()
                        .or_else(|| renderer.mesh.map(|id| format!("{:032x}", id.as_u128())))
                        .unwrap_or_else(|| "missing-mesh".to_owned()),
                    material: renderer
                        .material
                        .builtin
                        .clone()
                        .or_else(|| {
                            renderer
                                .material
                                .asset
                                .map(|id| format!("{:032x}", id.as_u128()))
                        })
                        .unwrap_or_else(|| "missing-material".to_owned()),
                }),
                ComponentData::Light(light) => world.lights.push(RenderLight {
                    object: object.id,
                    transform,
                    kind: light.kind.clone(),
                    intensity: light.intensity,
                }),
                _ => {}
            }
        }
    }
    world
}

fn select_first_scene_object(shell: &mut EditorShell) {
    if let Some(id) = shell.project().and_then(|project| {
        project
            .scene
            .find_by_name("Player")
            .and_then(|entity| project.scene.object(entity).map(|object| object.id))
            .or_else(|| {
                project
                    .scene
                    .objects()
                    .into_iter()
                    .next()
                    .map(|(_, object)| object.id)
            })
    }) {
        shell.select_entity_id(id);
    }
}

fn save_scene(shell: &mut EditorShell) {
    if let Err(error) = shell.save_scene() {
        shell.console_mut().push(engine_editor::ConsoleEntry {
            timestamp: "now".to_string(),
            level: engine_editor::ConsoleLevel::Error,
            source: engine_editor::ConsoleSource {
                subsystem: "editor".to_string(),
                file: None,
                line: None,
            },
            message: error.to_string(),
        });
    }
}

fn paint_text_in_rect(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align2,
) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let text = elide_to_width(ui, text, font.clone(), color, rect.width());
    let galley = ui.painter().layout_no_wrap(text, font, color);
    let text_rect = align.align_size_within_rect(galley.size(), rect);
    ui.painter()
        .with_clip_rect(rect)
        .galley(text_rect.min, galley, color);
}

fn paint_wrapped_text_in_rect(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align2,
) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let galley = ui
        .painter()
        .layout(text.to_owned(), font, color, rect.width());
    let text_rect = align.align_size_within_rect(galley.size(), rect);
    ui.painter()
        .with_clip_rect(rect)
        .galley(text_rect.min, galley, color);
}

fn elide_to_width(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> String {
    if ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), color)
        .size()
        .x
        <= max_width
    {
        return text.to_owned();
    }

    let ellipsis = "...";
    if ui
        .painter()
        .layout_no_wrap(ellipsis.to_owned(), font.clone(), color)
        .size()
        .x
        > max_width
    {
        return String::new();
    }

    let chars = text.chars().collect::<Vec<_>>();
    let mut low = 0;
    let mut high = chars.len();
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate = chars
            .iter()
            .take(mid)
            .chain(ellipsis.chars().collect::<Vec<_>>().iter())
            .collect::<String>();
        let width = ui
            .painter()
            .layout_no_wrap(candidate, font.clone(), color)
            .size()
            .x;
        if width <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    chars
        .into_iter()
        .take(low)
        .chain(ellipsis.chars())
        .collect()
}

fn apply_visuals(ctx: &egui::Context, pal: &InfernuxPalette) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = pal.window_bg;
    visuals.window_fill = pal.panel_bg;
    visuals.extreme_bg_color = pal.frame_bg;
    visuals.faint_bg_color = pal.frame_bg;
    visuals.window_stroke = Stroke::new(1.0, pal.border);
    visuals.window_corner_radius = CornerRadius::same(0);
    visuals.widgets.noninteractive.bg_fill = pal.frame_bg;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
    visuals.widgets.inactive.bg_fill = pal.frame_bg;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text_dim);
    visuals.widgets.hovered.bg_fill = pal.frame_hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.text);
    visuals.widgets.active.bg_fill = pal.header_hover;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, pal.text);
    visuals.selection.bg_fill = pal.selection;
    visuals.selection.stroke = Stroke::new(1.0, pal.text);
    ctx.set_visuals(visuals);
}
