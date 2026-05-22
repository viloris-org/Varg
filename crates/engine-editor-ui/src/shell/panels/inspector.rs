//! Inspector panel for the editor shell.

use egui::{DragValue, FontId, RichText, Vec2};

use super::super::operations::command::push_error;
use super::super::operations::scene_ops::{
    assign_asset_to_object, default_components, scene_snapshot,
};
use super::super::types::{InfernuxPalette, ShellUiState};
use super::super::widgets::buttons::small_chip;
use super::super::widgets::component_ui::component_header;
use super::super::widgets::icons::ui as icon;
use super::super::widgets::layout::empty_view;
use super::super::widgets::property_editors::*;
use crate::EditorShell;
use engine_assets::{ResourceKind, ResourceMetaFormat};
use engine_core::math::{Quat, Vec3 as EngineVec3};
use engine_ecs::{ComponentData, ComponentFieldSchema, ComponentSchema, ComponentSchemaRegistry};
use engine_editor::UndoCommand;
use engine_i18n::Translations;
/// Renders the inspector panel with component properties.

pub fn draw_inspector(
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
    let before = ui_state
        .inspector_drag_before
        .clone()
        .or_else(|| scene_snapshot(shell));
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
            let mut any_transform_dragging = false;
            if let Some(object) = project.scene.object_mut(entity) {
                ui.horizontal(|ui| {
                    dirty |= ui
                        .add(egui::Checkbox::without_text(&mut object.active))
                        .changed();
                    dirty |= ui
                        .add_sized(
                            Vec2::new(ui.available_width(), 22.0),
                            egui::TextEdit::singleline(&mut object.name)
                                .text_color(pal.text),
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
                let transform_key = "Transform".to_string();
                let transform_collapsed = ui_state.inspector_collapsed.contains(&transform_key);
                let header_resp = component_header(
                    ui,
                    tr.tr("inspector_transform"),
                    true,
                    transform_collapsed,
                    pal,
                );
                if header_resp.clicked() {
                    if transform_collapsed {
                        ui_state.inspector_collapsed.retain(|c| c != &transform_key);
                    } else {
                        ui_state.inspector_collapsed.push(transform_key);
                    }
                }
                if !transform_collapsed {
                    let transform_before = transform;
                    ui.horizontal(|ui| {
                        let (changed, dragging) = vec3_editor_with_step(
                            ui,
                            tr.tr("inspector_position"),
                            &mut transform.translation,
                            0.1,
                            pal,
                        );
                        dirty |= changed;
                        any_transform_dragging |= dragging;
                        if small_reset_button(ui, tr.tr("inspector_reset"), 36.0, pal).clicked() {
                            transform.translation = EngineVec3::ZERO;
                            dirty = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        let (yaw, pitch, roll) = transform.rotation.to_euler_deg();
                        let mut euler = EngineVec3::new(pitch, yaw, roll);
                        let (changed, dragging) = vec3_editor_with_step(
                            ui,
                            tr.tr("inspector_rotation"),
                            &mut euler,
                            1.0,
                            pal,
                        );
                        dirty |= changed;
                        any_transform_dragging |= dragging;
                        transform.rotation = Quat::from_euler_deg(euler.y, euler.x, euler.z);
                        if small_reset_button(ui, tr.tr("inspector_reset"), 36.0, pal).clicked() {
                            transform.rotation = Quat::IDENTITY;
                            dirty = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        let (changed, dragging) = vec3_editor_with_step(
                            ui,
                            tr.tr("inspector_scale"),
                            &mut transform.scale,
                            0.01,
                            pal,
                        );
                        dirty |= changed;
                        any_transform_dragging |= dragging;
                        if small_reset_button(ui, tr.tr("inspector_reset"), 36.0, pal).clicked() {
                            transform.scale = EngineVec3::ONE;
                            dirty = true;
                        }
                    });
                    if transform != transform_before {
                        project.scene.transforms_mut().set_local(entity, transform);
                    }
                }
            }

            let components = project.scene.components(entity).unwrap_or(&[]).to_vec();
            let assets = project.assets.clone();
            let mut remove_component = None;
            let mut confirm_matched = false;
            for mut component in components {
                let changed = draw_component_editor(
                    ui,
                    &mut component,
                    &assets,
                    &mut ui_state.inspector_collapsed,
                    pal,
                    tr,
                );
                let type_id = component.type_id().to_owned();
                let is_confirming = ui_state.remove_confirm.as_deref() == Some(&type_id);
                if is_confirming {
                    confirm_matched = true;
                }
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    let remove_label = if is_confirming {
                        tr.tr("inspector_confirm_remove")
                    } else {
                        icon::CLOSE
                    };
                    let remove_btn = small_chip(ui, &remove_label, 48.0, pal);
                    if remove_btn.clicked() {
                        if is_confirming {
                            remove_component = Some(type_id);
                        } else {
                            ui_state.remove_confirm = Some(type_id);
                        }
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
            if !confirm_matched {
                ui_state.remove_confirm = None;
            }
            if let Some(component_type) = remove_component {
                ui_state.remove_confirm = None;
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
                    ui.add_sized(
                        Vec2::new(ui.available_width().max(120.0), 20.0),
                        egui::TextEdit::singleline(&mut ui_state.add_component_filter)
                            .hint_text(tr.tr("inspector_add_component_search"))
                            .font(FontId::proportional(12.0))
                            .text_color(pal.text),
                    );
                    let filter = ui_state.add_component_filter.to_lowercase();
                    for (label, component) in default_components() {
                        if !filter.is_empty() && !label.to_lowercase().contains(&filter) {
                            continue;
                        }
                        if ui.selectable_label(false, label).clicked() {
                            if let Err(error) = project.scene.upsert_component(entity, component) {
                                errors.push(error.to_string());
                            } else {
                                dirty = true;
                            }
                            ui_state.add_component_filter.clear();
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
                if any_transform_dragging {
                    if ui_state.inspector_drag_before.is_none() {
                        ui_state.inspector_drag_before = before.clone();
                    }
                } else {
                    if let (Some(before_val), Ok(after)) =
                        (&before, project.scene.to_json("Inspector"))
                    {
                        undo_to_push = Some(UndoCommand::new(
                            "Inspector Edit",
                            format!("{:032x}", selected_id.as_u128()),
                            before_val.clone(),
                            after,
                        ));
                    }
                    ui_state.inspector_drag_before = None;
                }
            } else if !any_transform_dragging {
                ui_state.inspector_drag_before = None;
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
    collapsed_set: &mut Vec<String>,
    pal: &InfernuxPalette,
    tr: &Translations,
) -> bool {
    let registry = ComponentSchemaRegistry::builtin();
    let schema = registry.get(component.type_id());
    let title = schema
        .map(|schema| schema.display_name.as_str())
        .unwrap_or_else(|| component.type_id());
    let type_id = component.type_id().to_string();
    let collapsed = collapsed_set.contains(&type_id);
    let header_response = component_header(ui, title, true, collapsed, pal);
    if header_response.clicked() {
        if collapsed {
            collapsed_set.retain(|c| c != &type_id);
        } else {
            collapsed_set.push(type_id.clone());
        }
    }
    if collapsed {
        return false;
    }
    if let Some(schema) = schema {
        let mut dirty = draw_component_schema_fields(ui, component, schema, assets, pal, tr);
        if let ComponentData::Rigidbody(body) = component {
            dirty |= lock_axes_editor(
                ui,
                tr.tr("property_lock_position"),
                &mut body.lock_position,
                pal,
            );
            dirty |= lock_axes_editor(
                ui,
                tr.tr("property_lock_rotation"),
                &mut body.lock_rotation,
                pal,
            );
        }
        return dirty;
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
            dirty |= f32_property_row(ui, tr.tr("property_far"), &mut camera.far, pal);
            dirty |= bool_property_row(ui, tr.tr("property_primary"), &mut camera.primary, pal);
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
            dirty |= bool_property_row(
                ui,
                tr.tr("property_casts_shadows"),
                &mut renderer.casts_shadows,
                pal,
            );
        }
        ComponentData::Light(light) => {
            dirty |= enum_property_row(
                ui,
                tr.tr("property_type"),
                &mut light.kind,
                &["directional", "point", "spot"],
                pal,
            );
            dirty |= color_vec3_editor(ui, tr.tr("property_color"), &mut light.color, pal);
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
            dirty |= bool_property_row(
                ui,
                tr.tr("property_use_gravity"),
                &mut body.use_gravity,
                pal,
            );
        }
        ComponentData::Collider(collider) => {
            dirty |= enum_property_row(
                ui,
                tr.tr("property_shape"),
                &mut collider.shape,
                &["box", "sphere", "capsule"],
                pal,
            );
            dirty |= vec3_editor(ui, tr.tr("property_size"), &mut collider.size, pal);
            dirty |= bool_property_row(
                ui,
                tr.tr("property_is_trigger"),
                &mut collider.is_trigger,
                pal,
            );
            dirty |= u32_property_row(ui, tr.tr("property_mask"), &mut collider.mask, pal);
        }
        ComponentData::AudioSource(source) => {
            let mut builtin = None;
            dirty |= asset_ref_row(
                ui,
                tr.tr("property_clip"),
                &mut source.clip,
                &mut builtin,
                assets,
                &[ResourceKind::Audio],
                pal,
            );
            dirty |= f32_property_row(ui, tr.tr("property_volume"), &mut source.volume, pal);
            dirty |= bool_property_row(ui, tr.tr("property_looping"), &mut source.looping, pal);
            dirty |= bool_property_row(
                ui,
                tr.tr("property_play_on_start"),
                &mut source.play_on_start,
                pal,
            );
        }
        ComponentData::Script(script) => {
            dirty |= string_property_row(ui, tr.tr("property_backend"), &mut script.backend, pal);
            dirty |=
                string_property_row(ui, tr.tr("property_script_name"), &mut script.script, pal);
            dirty |= bool_property_row(
                ui,
                tr.tr("property_pending_recovery"),
                &mut script.pending_recovery,
                pal,
            );
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
            "vertical_fov_degrees" => f32_property_row_clamped(
                ui,
                &label,
                &mut camera.vertical_fov_degrees,
                20.0..=150.0,
                1.0,
                pal,
            ),
            "near" => {
                f32_property_row_clamped(ui, &label, &mut camera.near, 0.001..=10.0, 0.01, pal)
            }
            "far" => {
                f32_property_row_clamped(ui, &label, &mut camera.far, 10.0..=10000.0, 1.0, pal)
            }
            "primary" => bool_property_row(ui, &label, &mut camera.primary, pal),
            "clear_color" => vec3_editor(ui, &label, &mut camera.clear_color, pal),
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
            "receive_shadows" => bool_property_row(ui, &label, &mut renderer.receive_shadows, pal),
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
            "color" => color_vec3_editor(ui, &label, &mut light.color, pal),
            "intensity" => {
                f32_property_row_clamped(ui, &label, &mut light.intensity, 0.0..=100.0, 0.1, pal)
            }
            "range" => {
                f32_property_row_clamped(ui, &label, &mut light.range, 0.0..=1000.0, 0.5, pal)
            }
            "spot_angle" => {
                f32_property_row_clamped(ui, &label, &mut light.spot_angle, 1.0..=179.0, 1.0, pal)
            }
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
            "mass" => {
                f32_property_row_clamped(ui, &label, &mut body.mass, 0.01..=10000.0, 0.1, pal)
            }
            "use_gravity" => bool_property_row(ui, &label, &mut body.use_gravity, pal),
            "linear_damping" => f32_property_row_clamped(
                ui,
                &label,
                &mut body.linear_damping,
                0.0..=100.0,
                0.01,
                pal,
            ),
            "angular_damping" => f32_property_row_clamped(
                ui,
                &label,
                &mut body.angular_damping,
                0.0..=100.0,
                0.01,
                pal,
            ),
            _ => false,
        },
        ComponentData::Collider(collider) => match field.name.as_str() {
            "shape" => enum_property_row(
                ui,
                &label,
                &mut collider.shape,
                &["box", "sphere", "capsule", "cylinder", "mesh"],
                pal,
            ),
            "size" => vec3_editor(ui, &label, &mut collider.size, pal),
            "is_trigger" => bool_property_row(ui, &label, &mut collider.is_trigger, pal),
            "mask" => u32_property_row(ui, &label, &mut collider.mask, pal),
            "physics_material" => enum_property_row(
                ui,
                &label,
                &mut collider.physics_material,
                &["default", "metal", "ice", "rubber"],
                pal,
            ),
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
            "volume" => {
                f32_property_row_clamped(ui, &label, &mut source.volume, 0.0..=1.0, 0.01, pal)
            }
            "looping" => bool_property_row(ui, &label, &mut source.looping, pal),
            "play_on_start" => bool_property_row(ui, &label, &mut source.play_on_start, pal),
            "spatial_blend" => f32_property_row_clamped(
                ui,
                &label,
                &mut source.spatial_blend,
                0.0..=1.0,
                0.01,
                pal,
            ),
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
        "casts_shadows" => tr.tr("property_casts_shadows").to_owned(),
        "use_gravity" => tr.tr("property_use_gravity").to_owned(),
        "clip" => tr.tr("property_clip").to_owned(),
        "looping" => tr.tr("property_looping").to_owned(),
        "play_on_start" => tr.tr("property_play_on_start").to_owned(),
        "primary" => tr.tr("property_primary").to_owned(),
        "far" => tr.tr("property_far").to_owned(),
        "size" => tr.tr("property_size").to_owned(),
        "color" => tr.tr("property_color").to_owned(),
        "is_trigger" => tr.tr("property_is_trigger").to_owned(),
        "mask" => tr.tr("property_mask").to_owned(),
        "clear_color" => tr.tr("property_clear_color").to_owned(),
        "receive_shadows" => tr.tr("property_receive_shadows").to_owned(),
        "range" => tr.tr("property_range").to_owned(),
        "spot_angle" => tr.tr("property_spot_angle").to_owned(),
        "linear_damping" => tr.tr("property_linear_damping").to_owned(),
        "angular_damping" => tr.tr("property_angular_damping").to_owned(),
        "physics_material" => tr.tr("property_physics_material").to_owned(),
        "spatial_blend" => tr.tr("property_spatial_blend").to_owned(),
        "pending_recovery" => tr.tr("property_pending_recovery").to_owned(),
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

fn small_reset_button(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
    pal: &InfernuxPalette,
) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(10.0).color(pal.text_dim))
            .fill(pal.frame_bg)
            .min_size(Vec2::new(width, 20.0)),
    )
}
