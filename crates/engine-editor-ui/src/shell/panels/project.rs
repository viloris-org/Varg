//! Project panel for the editor shell.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Vec2};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::super::operations::asset_ops::{
    create_default_material, delete_asset, open_asset, reimport_asset, show_in_file_manager,
    thumbnail_color,
};
use super::super::operations::command::push_error;
use super::super::types::{InfernuxPalette, ShellUiState};
use super::super::widgets::buttons::small_chip;
use super::super::widgets::icons::ui as icon;
use super::super::widgets::layout::{empty_view, search_field, toolbar_row};
use super::super::widgets::text::paint_text_in_rect;
use crate::{resource_kind_label, EditorShell};
use engine_assets::ResourceState;
use engine_i18n::Translations;

/// Renders the project panel with asset browser.
pub fn draw_project_panel(
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
        if small_chip(ui, tr.tr("project_rescan"), 56.0, pal).clicked() {
            match shell.project_mut().map(|project| {
                project.rescan_assets()?;
                Ok::<_, engine_core::EngineError>(format!(
                    "scan: {} assets",
                    project.database.iter_entries().count()
                ))
            }) {
                Some(Ok(status)) => ui_state.project_import_status = Some(status),
                Some(Err(error)) => push_error(shell, error.to_string()),
                None => {}
            }
        }
        if small_chip(ui, tr.tr("project_import"), 64.0, pal).clicked() {
            let path = PathBuf::from(ui_state.project_import_path.trim());
            if path.as_os_str().is_empty() {
                ui_state.project_import_status = Some(tr.tr("project_import_hint").to_owned());
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
                .hint_text(tr.tr("project_file_path_hint"))
                .font(FontId::proportional(11.0))
                .text_color(pal.text),
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
    let asset_root_str = project.manifest.asset_root.clone();
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

    let entries: Vec<_> = project.database.iter_entries().cloned().collect();
    let folders: BTreeSet<PathBuf> = project.database.folders().iter().cloned().collect();
    let selected_asset_info = shell
        .selection()
        .as_asset()
        .and_then(|p| project.database.entry_for_path(p).cloned());

    if entries.is_empty() && folders.is_empty() {
        empty_view(ui, tr.tr("project_empty"), pal);
        return;
    }

    let query = ui_state.project_filter.trim().to_lowercase();
    let mut tree: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    tree.insert(PathBuf::new(), Vec::new());

    for folder in &folders {
        tree.insert(folder.clone(), Vec::new());
        if let Some(parent) = folder.parent() {
            tree.entry(parent.to_path_buf()).or_default();
        }
    }

    let mut file_entries: BTreeMap<PathBuf, engine_assets::ResourceMeta> = BTreeMap::new();
    for entry in entries {
        let parent = entry.path.parent().unwrap_or(Path::new("")).to_path_buf();
        tree.entry(parent.clone()).or_default();
        tree.get_mut(&parent).unwrap().push(entry.path.clone());
        file_entries.insert(entry.path.clone(), entry);
    }

    if ui_state.expanded_folders.is_empty() && !folders.is_empty() {
        for folder in &folders {
            ui_state
                .expanded_folders
                .insert(folder.to_string_lossy().into_owned());
        }
    }

    egui::ScrollArea::vertical()
        .id_salt("infernux_project_assets_scroll")
        .show(ui, |ui| {
            draw_folder_tree(
                ui,
                shell,
                ui_state,
                pal,
                tr,
                PathBuf::new(),
                &tree,
                &folders,
                &file_entries,
                &query,
            );
        });

    if let Some(meta) = &selected_asset_info {
        ui.add_space(4.0);
        ui.painter().rect_filled(
            ui.available_rect_before_wrap(),
            CornerRadius::same(0),
            pal.frame_bg,
        );
        ui.add_space(2.0);
        let info_label = |ui: &mut egui::Ui, label: &str, value: &str| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}:", label))
                        .size(10.0)
                        .color(pal.text_dim),
                );
                ui.label(RichText::new(value).size(10.0).color(pal.text));
            });
        };
        info_label(
            ui,
            tr.tr("project_info_path"),
            &meta.path.display().to_string(),
        );
        info_label(
            ui,
            tr.tr("project_info_guid"),
            &format!("{:032x}", meta.guid.as_u128()),
        );
        info_label(
            ui,
            tr.tr("project_info_kind"),
            resource_kind_label(meta.kind, tr),
        );
        let (state_text, _) = import_status_badge(meta.import_state, tr);
        info_label(ui, tr.tr("project_info_state"), &state_text);
        let abs_path = project_root.join(asset_root_str.as_str()).join(&meta.path);
        if let Ok(file_meta) = std::fs::metadata(&abs_path) {
            info_label(
                ui,
                tr.tr("project_info_size"),
                &format_size(file_meta.len()),
            );
            if let Ok(modified) = file_meta.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    let secs = duration.as_secs();
                    let days = secs / 86400;
                    let hours = (secs % 86400) / 3600;
                    let mins = (secs % 3600) / 60;
                    info_label(
                        ui,
                        tr.tr("project_info_modified"),
                        &format!("{}d {}h {}m ago", days, hours, mins),
                    );
                }
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn draw_folder_tree(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
    folder: PathBuf,
    tree: &BTreeMap<PathBuf, Vec<PathBuf>>,
    folders: &BTreeSet<PathBuf>,
    file_entries: &BTreeMap<PathBuf, engine_assets::ResourceMeta>,
    query: &str,
) {
    let folder_key = folder.to_string_lossy().into_owned();
    let expanded = ui_state.expanded_folders.contains(&folder_key);

    let mut child_folders: Vec<&PathBuf> = folders
        .iter()
        .filter(|f| {
            f.parent() == Some(folder.as_path())
                || (folder.as_os_str().is_empty() && f.parent().is_none())
        })
        .collect();
    child_folders.sort();

    let child_files = tree.get(&folder).cloned().unwrap_or_default();

    let has_matching_files = |q: &str| -> bool {
        if q.is_empty() {
            return true;
        }
        for file_path in &child_files {
            if file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase()
                .contains(q)
            {
                return true;
            }
        }
        for sub in &child_folders {
            let sub_files = tree.get(*sub).cloned().unwrap_or_default();
            for file_path in &sub_files {
                if file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(q)
                {
                    return true;
                }
            }
        }
        false
    };

    if !query.is_empty() && !has_matching_files(query) {
        return;
    }

    if !folder.as_os_str().is_empty() {
        let folder_name = folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder");
        let arrow = if expanded {
            icon::CHEVRON_DOWN
        } else {
            icon::CHEVRON_RIGHT
        };
        let response = ui.allocate_response(Vec2::new(ui.available_width(), 18.0), Sense::click());
        let rect = response.rect;
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(0),
            if response.hovered() {
                pal.frame_hover
            } else {
                pal.panel_bg
            },
        );
        let x = rect.min.x + 4.0;
        let y = rect.min.y + 1.0;
        ui.painter().text(
            Pos2::new(x, y),
            Align2::LEFT_TOP,
            arrow,
            FontId::proportional(11.0),
            pal.text_dim,
        );
        ui.painter().text(
            Pos2::new(x + 14.0, y),
            Align2::LEFT_TOP,
            "📁",
            FontId::proportional(11.0),
            pal.text,
        );
        ui.painter().text(
            Pos2::new(x + 30.0, y),
            Align2::LEFT_TOP,
            folder_name,
            FontId::proportional(11.0),
            pal.text,
        );
        if response.clicked() {
            if expanded {
                ui_state.expanded_folders.remove(&folder_key);
            } else {
                ui_state.expanded_folders.insert(folder_key);
            }
        }
    }

    if expanded || folder.as_os_str().is_empty() {
        for sub in &child_folders {
            draw_folder_tree(
                ui,
                shell,
                ui_state,
                pal,
                tr,
                (*sub).clone(),
                tree,
                folders,
                file_entries,
                query,
            );
        }

        let mut sorted_files = child_files.clone();
        sorted_files.sort();
        for file_path in &sorted_files {
            if let Some(meta) = file_entries.get(file_path) {
                let file_name = file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("asset");
                if !query.is_empty()
                    && !file_name.to_lowercase().contains(query)
                    && !resource_kind_label(meta.kind, tr)
                        .to_lowercase()
                        .contains(query)
                {
                    continue;
                }

                let indent = if folder.as_os_str().is_empty() {
                    4.0
                } else {
                    20.0
                };
                draw_asset_row(ui, shell, ui_state, pal, tr, meta, file_name, indent);
            }
        }
    }

    if folder.as_os_str().is_empty()
        && query.is_empty()
        && child_folders.is_empty()
        && child_files.is_empty()
    {
        empty_view(ui, tr.tr("project_empty"), pal);
    }
}

fn draw_asset_row(
    ui: &mut egui::Ui,
    shell: &mut EditorShell,
    ui_state: &mut ShellUiState,
    pal: &InfernuxPalette,
    tr: &Translations,
    meta: &engine_assets::ResourceMeta,
    file_name: &str,
    indent: f32,
) {
    let row_height = 18.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), row_height),
        Sense::click_and_drag(),
    );

    let is_selected = shell
        .selection()
        .as_asset()
        .map_or(false, |p| p == &meta.path);

    let bg = if is_selected {
        pal.selection
    } else if response.hovered() {
        pal.frame_hover
    } else {
        pal.panel_bg
    };
    ui.painter().rect_filled(rect, CornerRadius::same(0), bg);

    let x = rect.min.x + indent;
    let y = rect.min.y + 1.0;

    let icon_rect = Rect::from_min_max(
        Pos2::new(x, rect.min.y + 2.0),
        Pos2::new(x + 14.0, rect.max.y - 2.0),
    );
    ui.painter()
        .rect_filled(icon_rect, CornerRadius::same(2), thumbnail_color(meta.kind));

    let renaming = ui_state
        .asset_rename
        .as_ref()
        .map_or(false, |(p, _)| p == &meta.path);

    if renaming {
        let rename_id = egui::Id::new("asset_rename").with(&meta.path);
        if let Some((_, rename_text)) = &mut ui_state.asset_rename {
            let text_rect = Rect::from_min_max(
                Pos2::new(x + 18.0, rect.min.y),
                Pos2::new(rect.max.x - 64.0, rect.max.y),
            );
            ui.put(
                text_rect,
                egui::TextEdit::singleline(rename_text)
                    .id(rename_id)
                    .font(FontId::proportional(11.0))
                    .text_color(pal.text)
                    .background_color(pal.input_bg),
            );
            if ui.memory(|mem| !mem.has_focus(rename_id)) {
                ui.memory_mut(|mem| mem.request_focus(rename_id));
            }
            let confirm = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if confirm || cancel {
                if confirm && !rename_text.is_empty() && rename_text != file_name {
                    if let Some(project) = shell.project_mut() {
                        let asset_root = project.root.join(&project.manifest.asset_root);
                        let old_abs = asset_root.join(&meta.path);
                        if let Some(parent) = old_abs.parent() {
                            let new_abs = parent.join(rename_text.as_str());
                            if std::fs::rename(&old_abs, &new_abs).is_ok() {
                                if project.rescan_assets().is_ok() {
                                    ui_state.status_toast =
                                        Some(tr.tr("project_asset_renamed").to_owned());
                                    ui_state.status_toast_frames = 180;
                                }
                            }
                        }
                    }
                }
                ui_state.asset_rename = None;
            }
        }
    } else {
        ui.painter().text(
            Pos2::new(x + 18.0, y),
            Align2::LEFT_TOP,
            file_name,
            FontId::proportional(11.0),
            pal.text,
        );
    }

    let (badge_text, badge_color) = import_status_badge(meta.import_state, tr);
    let badge_x = rect.max.x - 60.0;
    let badge_rect = Rect::from_min_max(
        Pos2::new(badge_x, rect.min.y + 3.0),
        Pos2::new(rect.max.x - 4.0, rect.max.y - 3.0),
    );
    ui.painter()
        .rect_filled(badge_rect, CornerRadius::same(2), badge_color);
    paint_text_in_rect(
        ui,
        badge_rect,
        &badge_text,
        FontId::proportional(9.0),
        pal.text,
        Align2::CENTER_CENTER,
    );

    if response.clicked() {
        shell
            .selection_mut()
            .select(engine_editor::Selection::Asset(meta.path.clone()));
    }

    if response.double_clicked() {
        open_asset(shell, ui_state, meta.guid, meta.kind, &meta.path, tr);
    }

    if response.drag_started() {
        ui_state.dragged_asset = Some(meta.guid);
    }

    response.context_menu(|ui| {
        if ui.button(tr.tr("project_delete")).clicked() {
            if ui_state.asset_delete_confirm.as_ref() == Some(&meta.path) {
                delete_asset(shell, ui_state, &meta.path, tr);
                ui_state.asset_delete_confirm = None;
            } else {
                ui_state.asset_delete_confirm = Some(meta.path.clone());
            }
            ui.close();
        }
        if ui.button(tr.tr("project_rename")).clicked() {
            ui_state.asset_rename = Some((meta.path.clone(), file_name.to_owned()));
            ui.close();
        }
        if ui.button(tr.tr("project_reimport")).clicked() {
            reimport_asset(shell, ui_state, &meta.path, tr);
            ui.close();
        }
        ui.separator();
        if ui.button(tr.tr("project_show_in_files")).clicked() {
            show_in_file_manager(shell, &meta.path);
            ui.close();
        }
        if ui.button(tr.tr("project_copy_guid")).clicked() {
            ui.ctx().copy_text(format!("{:032x}", meta.guid.as_u128()));
            ui_state.status_toast = Some(tr.tr("project_guid_copied").to_owned());
            ui_state.status_toast_frames = 180;
            ui.close();
        }
    });
}

fn import_status_badge(state: ResourceState, tr: &Translations) -> (String, Color32) {
    match state {
        ResourceState::Unloaded => (
            tr.tr("project_status_unloaded").to_owned(),
            Color32::from_rgb(100, 100, 100),
        ),
        ResourceState::LoadingCpu | ResourceState::UploadQueued => (
            tr.tr("project_status_loading").to_owned(),
            Color32::from_rgb(180, 160, 50),
        ),
        ResourceState::CpuReady | ResourceState::GpuReady => (
            tr.tr("project_status_ready").to_owned(),
            Color32::from_rgb(50, 130, 70),
        ),
        ResourceState::Stale => (
            tr.tr("project_status_stale").to_owned(),
            Color32::from_rgb(180, 130, 50),
        ),
        ResourceState::Failed => (
            tr.tr("project_status_failed").to_owned(),
            Color32::from_rgb(180, 50, 50),
        ),
    }
}
