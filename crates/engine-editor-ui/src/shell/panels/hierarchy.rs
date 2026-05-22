//! Hierarchy panel for the editor shell.

use egui::{Align2, CornerRadius, FontId, Sense, Vec2};

use super::super::operations::command::push_error;
use super::super::operations::scene_ops::{
    create_empty_child, create_object_with_component, delete_object, duplicate_object,
    rename_object, reparent_object,
};
use super::super::types::{InfernuxPalette, ShellUiState};
use super::super::widgets::buttons::small_chip;
use super::super::widgets::icons::{components as component_icons, ui as icon};
use super::super::widgets::layout::{empty_view, search_field, toolbar_row};
use super::super::widgets::text::paint_text_in_rect;
use crate::EditorShell;
use engine_core::EntityId;
use engine_ecs::{CameraComponentData, ComponentData, LightComponentData};
use engine_i18n::Translations;
/// Renders the hierarchy panel with scene object tree.

pub fn draw_hierarchy(
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
        push_error(shell, error);
    }

    let Some(project) = shell.project() else {
        empty_view(ui, tr.tr("hierarchy_no_scene"), pal);
        return;
    };

    let query = ui_state.hierarchy_filter.trim().to_lowercase();
    let roots = project.scene.transforms().roots().to_vec();
    let selected = shell.selected_entity_id();

    if roots.is_empty() {
        empty_view(ui, tr.tr("hierarchy_no_objects"), pal);
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("infernux_hierarchy_scroll")
        .show(ui, |ui| {
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
                    tr,
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
    tr: &Translations,
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
    let mut children = project.scene.transforms().children(entity);
    {
        let transforms = project.scene.transforms();
        children.sort_by_key(|child| transforms.sibling_index(*child).unwrap_or(0));
    }
    let has_children = !children.is_empty();
    let matches_query = query.is_empty() || name.to_lowercase().contains(query);

    if matches_query {
        let icon = component_icon(object);
        let indent = "  ".repeat(depth);
        let chevron = if has_children {
            icon::CHEVRON_DOWN
        } else {
            " "
        };
        let is_renaming = ui_state
            .hierarchy_rename
            .as_ref()
            .map(|(rid, _)| *rid == id)
            .unwrap_or(false);

        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 22.0),
            Sense::click_and_drag(),
        );

        if is_renaming {
            let text_id = ui.make_persistent_id(("hierarchy_rename_edit", id));
            if ui.memory(|mem| !mem.has_focus(text_id)) {
                ui.memory_mut(|mem| mem.request_focus(text_id));
            }
            if selected == Some(id) || ui_state.hierarchy_selection.contains(&id) {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(0), pal.selection);
            } else if *row_index % 2 == 0 {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(0), pal.row_alt);
            }
            let prefix = format!("{indent}{chevron} {icon} ");
            let prefix_galley = ui.painter().layout_no_wrap(
                prefix.clone(),
                FontId::proportional(12.0),
                pal.text_dim,
            );
            let prefix_width = prefix_galley.size().x;
            ui.painter().galley(
                rect.left_center() + Vec2::new(8.0, 0.0),
                prefix_galley,
                pal.text_dim,
            );
            let edit_rect = egui::Rect::from_min_max(
                rect.min + Vec2::new(8.0 + prefix_width, 2.0),
                rect.max - Vec2::new(4.0, 2.0),
            );
            let mut edit_text = ui_state
                .hierarchy_rename
                .as_ref()
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            let text_edit = egui::TextEdit::singleline(&mut edit_text)
                .id(text_id)
                .desired_width(edit_rect.width())
                .font(FontId::proportional(12.0))
                .text_color(pal.text);
            let edit_response = ui.put(edit_rect, text_edit);
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
            ui_state.hierarchy_rename = Some((id, edit_text.clone()));

            if (edit_response.lost_focus() || enter_pressed) && !escape_pressed {
                let new_name = edit_text.trim().to_string();
                if !new_name.is_empty() && new_name != object.name {
                    rename_object(shell, entity, id, new_name);
                }
                ui_state.hierarchy_rename = None;
            } else if escape_pressed {
                ui_state.hierarchy_rename = None;
            }
        } else {
            let row_text = format!(
                "{indent}{chevron} {icon} {}",
                if active {
                    name.clone()
                } else {
                    format!("{name} (inactive)")
                }
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
            if response.double_clicked() {
                ui_state.hierarchy_rename = Some((id, name.clone()));
            } else if response.clicked() {
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
                if ui.button(tr.tr("hierarchy_duplicate")).clicked() {
                    duplicate_object(shell, id);
                    ui.close();
                }
                if ui.button(tr.tr("hierarchy_delete")).clicked() {
                    delete_object(shell, id);
                    ui.close();
                }
                ui.separator();
                if ui.button(tr.tr("hierarchy_create_empty_child")).clicked() {
                    create_empty_child(shell, entity, id);
                    ui.close();
                }
                if ui.button(tr.tr("hierarchy_create_camera")).clicked() {
                    create_object_with_component(
                        shell,
                        entity,
                        "Camera",
                        ComponentData::Camera(CameraComponentData::default()),
                    );
                    ui.close();
                }
                if ui.button(tr.tr("hierarchy_create_light")).clicked() {
                    create_object_with_component(
                        shell,
                        entity,
                        "Light",
                        ComponentData::Light(LightComponentData::default()),
                    );
                    ui.close();
                }
                ui.separator();
                if ui.button(tr.tr("hierarchy_clear_parent")).clicked() {
                    reparent_object(shell, id, None);
                    ui.close();
                }
            });
        }

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
            tr,
        );
    }
}

fn component_icon(object: &engine_ecs::GameObject) -> &'static str {
    for component in &object.components {
        match component {
            engine_ecs::ComponentData::Camera(_) => return component_icons::CAMERA,
            engine_ecs::ComponentData::Light(_) => return component_icons::LIGHTBULB,
            engine_ecs::ComponentData::MeshRenderer(_) => return component_icons::CUBE,
            _ => {}
        }
    }
    component_icons::GAME_OBJECT
}
