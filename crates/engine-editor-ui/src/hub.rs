//! egui rendering for [`HubState`].
//!
//! Call [`draw_hub`] once per frame inside an egui context.

#![allow(deprecated)]

use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, Stroke, StrokeKind, Vec2,
};

use crate::{
    ConfirmDeleteDialog, DesignTokens, HubAction, HubPage, HubState, NewProjectDialog,
    ProjectDeletionMode,
};
use engine_editor::{NewProjectRequest, ProjectMetadata, ThemePreference};
use engine_i18n::Translations;
use std::path::PathBuf;

// ── colour helpers ────────────────────────────────────────────────────────────

fn hex(s: &str) -> Color32 {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).unwrap_or(0);
    Color32::from_rgb(
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u8,
    )
}

fn darken(c: Color32, amount: u8) -> Color32 {
    Color32::from_rgb(
        c.r().saturating_sub(amount),
        c.g().saturating_sub(amount),
        c.b().saturating_sub(amount),
    )
}

struct Palette {
    base: Color32,
    surface: Color32,
    surface_hover: Color32,
    surface_selected: Color32,
    border: Color32,
    text_primary: Color32,
    text_secondary: Color32,
    accent: Color32,
    accent_text: Color32,
    danger: Color32,
}

fn make_palette(t: &DesignTokens, is_dark: bool) -> Palette {
    Palette {
        base: hex(t.base),
        surface: hex(t.surface),
        surface_hover: hex(t.surface_hover),
        surface_selected: if is_dark {
            Color32::from_rgb(0x33, 0x33, 0x33)
        } else {
            Color32::from_rgb(0xe8, 0xe8, 0xe6)
        },
        border: hex(t.border),
        text_primary: hex(t.text_primary),
        text_secondary: hex(t.text_secondary),
        accent: hex(t.accent),
        accent_text: if is_dark {
            Color32::from_rgb(0x19, 0x19, 0x19)
        } else {
            Color32::WHITE
        },
        danger: hex(t.danger),
    }
}

fn apply_visuals(ctx: &egui::Context, pal: &Palette) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = pal.base;
    v.window_fill = pal.surface;
    v.extreme_bg_color = pal.surface;
    v.faint_bg_color = pal.surface;
    v.window_stroke = Stroke::new(1.0, pal.border);
    v.widgets.noninteractive.bg_fill = pal.surface;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text_primary);
    v.widgets.inactive.bg_fill = pal.surface;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text_secondary);
    v.widgets.hovered.bg_fill = pal.surface_hover;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.text_primary);
    v.widgets.active.bg_fill = pal.surface_hover;
    v.widgets.active.fg_stroke = Stroke::new(1.0, pal.text_primary);
    ctx.set_visuals(v);
}

// ── public entry point ────────────────────────────────────────────────────────

/// Draw the full Hub window into `ctx`.
/// Returns `true` when the user requests the window to close.
pub fn draw_hub(ctx: &egui::Context, hub: &mut HubState) -> bool {
    let is_dark = hub.preferences().theme != ThemePreference::Light;
    let pal = make_palette(&hub.design_tokens(), is_dark);
    let tr = Translations::load(hub.preferences().locale);
    apply_visuals(ctx, &pal);

    let mut close = false;

    egui::SidePanel::left("hub_sidebar")
        .exact_size(200.0)
        .resizable(false)
        .frame(
            Frame::NONE
                .fill(darken(pal.base, 8))
                .inner_margin(Margin::ZERO),
        )
        .show(ctx, |ui| {
            draw_sidebar(ui, hub, &pal, &tr, &mut close);
        });

    egui::CentralPanel::default()
        .frame(Frame::NONE.fill(pal.base))
        .show(ctx, |ui| match hub.page() {
            HubPage::Projects => draw_projects_page(ui, hub, &pal, &tr),
            HubPage::Installs => draw_installs_page(ui, hub, &pal, &tr),
            HubPage::Settings => draw_settings_page(ui, hub, &pal, &tr),
        });

    // Overlay dialogs
    draw_new_project_dialog(ctx, hub, &pal, &tr);
    draw_confirm_delete_dialog(ctx, hub, &pal, &tr);

    close
}

// ── sidebar ───────────────────────────────────────────────────────────────────

fn draw_sidebar(
    ui: &mut egui::Ui,
    hub: &mut HubState,
    pal: &Palette,
    tr: &Translations,
    _close: &mut bool,
) {
    let r = ui.max_rect();
    ui.painter().line_segment(
        [r.right_top(), r.right_bottom()],
        Stroke::new(1.0, pal.border),
    );

    ui.add_space(24.0);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(tr.tr("app_name"))
                    .size(22.0)
                    .strong()
                    .color(pal.text_primary),
            );
            ui.label(
                RichText::new(tr.tr("app_subtitle"))
                    .size(13.0)
                    .color(pal.text_secondary),
            );
        });
    });
    ui.add_space(16.0);

    for (label_key, page) in [
        ("sidebar_projects", HubPage::Projects),
        ("sidebar_installs", HubPage::Installs),
        ("sidebar_settings", HubPage::Settings),
    ] {
        let active = hub.page() == page;
        nav_item(ui, tr.tr(label_key), active, pal, || hub.set_page(page));
    }

    let remaining = ui.available_height() - 60.0;
    if remaining > 0.0 {
        ui.add_space(remaining);
    }

    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.label(
            RichText::new(tr.tr("sidebar_dark_mode"))
                .size(14.0)
                .color(pal.text_secondary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(20.0);
            let is_dark = hub.preferences().theme != ThemePreference::Light;
            let mut dark = is_dark;
            if ui.checkbox(&mut dark, "").changed() {
                hub.set_theme(if dark {
                    ThemePreference::Dark
                } else {
                    ThemePreference::Light
                });
            }
        });
    });
    ui.add_space(16.0);
}

fn nav_item(ui: &mut egui::Ui, label: &str, active: bool, pal: &Palette, on_click: impl FnOnce()) {
    let desired = Vec2::new(ui.available_width(), 42.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

    let bg = if active {
        pal.surface_hover
    } else if response.hovered() {
        pal.surface
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);

    if active {
        let bar = egui::Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height()));
        ui.painter()
            .rect_filled(bar, CornerRadius::ZERO, pal.accent);
    }

    let color = if active {
        pal.text_primary
    } else {
        pal.text_secondary
    };
    paint_text_in_rect(
        ui,
        rect.shrink2(Vec2::new(20.0, 0.0)),
        label,
        FontId::proportional(15.0),
        color,
        Align2::LEFT_CENTER,
    );

    if response.clicked() {
        on_click();
    }
}

// ── projects page ─────────────────────────────────────────────────────────────

fn draw_projects_page(ui: &mut egui::Ui, hub: &mut HubState, pal: &Palette, tr: &Translations) {
    // Collect data before borrowing hub mutably for actions
    let projects: Vec<_> = hub
        .filtered_projects()
        .into_iter()
        .map(|p| {
            (
                p.name.clone(),
                p.path.clone(),
                p.last_touched.clone(),
                p.toolchain_version.clone(),
            )
        })
        .collect();
    let selected_path = hub.selected_project.clone();

    egui::ScrollArea::vertical()
        .id_salt("hub_projects_scroll")
        .show(ui, |ui| {
            ui.add_space(24.0);

            // ── Header row ──
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.label(
                    RichText::new(tr.tr("hub_projects_title"))
                        .size(24.0)
                        .strong()
                        .color(pal.text_primary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(28.0);

                    // Launch button
                    let can_launch = selected_path.is_some();
                    ui.add_enabled_ui(can_launch, |ui| {
                        if ui
                            .button(RichText::new(tr.tr("hub_launch")).size(14.0))
                            .clicked()
                        {
                            if let Some(ref path) = selected_path {
                                if let Some(proj) = hub
                                    .filtered_projects()
                                    .into_iter()
                                    .find(|p| &p.path == path)
                                {
                                    hub.pending_action = hub.launch_editor_action(proj).ok();
                                }
                            }
                        }
                    });

                    ui.add_space(6.0);

                    // Delete button
                    ui.add_enabled_ui(can_launch, |ui| {
                        if ui
                            .button(
                                RichText::new(tr.tr("hub_delete"))
                                    .color(pal.danger)
                                    .size(14.0),
                            )
                            .clicked()
                        {
                            if let Some(ref path) = selected_path {
                                hub.confirm_delete = Some(ConfirmDeleteDialog {
                                    path: path.clone(),
                                    mode: ProjectDeletionMode::RemoveRecent,
                                });
                            }
                        }
                    });

                    ui.add_space(6.0);

                    // New Project button
                    if ui
                        .button(RichText::new(tr.tr("hub_new_project")).size(14.0))
                        .clicked()
                    {
                        let last_loc = hub
                            .preferences()
                            .last_project_location
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let _first_version = hub
                            .installs()
                            .first()
                            .map(|i| i.version.clone())
                            .unwrap_or_default();
                        hub.new_project_dialog = Some(NewProjectDialog {
                            location: last_loc,
                            template_idx: 0,
                            version_idx: 0,
                            name: String::new(),
                            error: None,
                        });
                    }
                });
            });

            ui.add_space(12.0);

            // ── Search bar ──
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                let mut search = hub.search.clone();
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut search)
                        .hint_text(tr.tr("hub_search"))
                        .desired_width((ui.available_width() - 56.0).max(120.0)),
                );
                if resp.changed() {
                    hub.set_search(&search);
                }
            });

            ui.add_space(12.0);

            // ── Project cards ──
            if projects.is_empty() {
                ui.add_space(40.0);
                ui.horizontal(|ui| {
                    ui.add_space(28.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(tr.tr("hub_empty"))
                                .size(14.0)
                                .color(pal.text_secondary),
                        )
                        .wrap(),
                    );
                });
            } else {
                let mut new_selection: Option<Option<PathBuf>> = None;
                let mut launch_path: Option<PathBuf> = None;

                for (name, path, touched, version) in &projects {
                    let is_selected = selected_path.as_ref() == Some(path);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(28.0);
                        let action = project_card(
                            ui,
                            &name,
                            path.to_string_lossy().as_ref(),
                            &touched,
                            &version,
                            is_selected,
                            pal,
                        );
                        match action {
                            CardAction::Select => new_selection = Some(Some(path.clone())),
                            CardAction::Launch => launch_path = Some(path.clone()),
                            CardAction::OpenFolder => {
                                hub.pending_action = Some(HubAction::OpenFolder(path.clone()));
                            }
                            CardAction::None => {}
                        }
                        ui.add_space(28.0);
                    });
                }

                if let Some(sel) = new_selection {
                    hub.selected_project = sel;
                }
                if let Some(path) = launch_path {
                    if let Some(proj) = hub.filtered_projects().into_iter().find(|p| p.path == path)
                    {
                        hub.pending_action = hub.launch_editor_action(proj).ok();
                    }
                }
            }

            ui.add_space(24.0);
        });
}

enum CardAction {
    None,
    Select,
    Launch,
    OpenFolder,
}

fn project_card(
    ui: &mut egui::Ui,
    name: &str,
    path: &str,
    touched: &str,
    version: &str,
    selected: bool,
    pal: &Palette,
) -> CardAction {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 72.0), egui::Sense::click());

    let bg = if selected {
        pal.surface_selected
    } else if response.hovered() {
        pal.surface_hover
    } else {
        pal.surface
    };
    let border_color = if selected { pal.accent } else { pal.border };
    ui.painter().rect(
        rect,
        CornerRadius::same(8),
        bg,
        Stroke::new(1.0, border_color),
        StrokeKind::Outside,
    );

    // Avatar
    let initials: String = name
        .split_whitespace()
        .take(2)
        .map(|w| w.chars().next().unwrap_or(' ').to_ascii_uppercase())
        .collect();
    let av = egui::Rect::from_min_size(rect.min + Vec2::new(12.0, 14.0), Vec2::splat(44.0));
    ui.painter()
        .rect_filled(av, CornerRadius::same(6), pal.accent);
    ui.painter().text(
        av.center(),
        egui::Align2::CENTER_CENTER,
        &initials,
        egui::FontId::proportional(16.0),
        pal.accent_text,
    );

    let folder_btn_rect =
        egui::Rect::from_min_size(rect.max - Vec2::new(40.0, 52.0), Vec2::new(32.0, 32.0));
    let meta_width = 88.0_f32.min((rect.width() * 0.24).max(54.0));
    let text_right = (folder_btn_rect.left() - meta_width - 8.0).max(rect.min.x + 96.0);
    let name_rect = Rect::from_min_max(
        rect.min + Vec2::new(70.0, 14.0),
        egui::pos2(text_right, rect.min.y + 33.0),
    );
    paint_text_in_rect(
        ui,
        name_rect,
        name,
        FontId::proportional(15.0),
        pal.text_primary,
        Align2::LEFT_CENTER,
    );

    let path_rect = Rect::from_min_max(
        rect.min + Vec2::new(70.0, 34.0),
        egui::pos2(text_right, rect.min.y + 55.0),
    );
    paint_text_in_rect(
        ui,
        path_rect,
        path,
        FontId::proportional(12.0),
        pal.text_secondary,
        Align2::LEFT_CENTER,
    );

    // Version + date (right side)
    let meta_rect = Rect::from_min_max(
        egui::pos2(text_right + 8.0, rect.min.y + 18.0),
        egui::pos2(folder_btn_rect.left() - 4.0, rect.min.y + 34.0),
    );
    paint_text_in_rect(
        ui,
        meta_rect,
        version,
        FontId::proportional(12.0),
        pal.text_secondary,
        Align2::LEFT_CENTER,
    );
    let date = if touched.len() >= 10 {
        &touched[..10]
    } else {
        touched
    };
    let date_rect = Rect::from_min_max(
        egui::pos2(text_right + 8.0, rect.min.y + 36.0),
        egui::pos2(folder_btn_rect.left() - 4.0, rect.min.y + 52.0),
    );
    paint_text_in_rect(
        ui,
        date_rect,
        date,
        FontId::proportional(11.0),
        pal.text_secondary,
        Align2::LEFT_CENTER,
    );

    // Open-folder button (⌂)
    let btn_resp = ui.allocate_rect(folder_btn_rect, egui::Sense::click());
    if btn_resp.hovered() {
        ui.painter()
            .rect_filled(folder_btn_rect, CornerRadius::same(6), pal.surface_hover);
    }
    paint_text_in_rect(
        ui,
        folder_btn_rect,
        "⌂",
        FontId::proportional(16.0),
        pal.text_secondary,
        Align2::CENTER_CENTER,
    );

    if btn_resp.clicked() {
        return CardAction::OpenFolder;
    }
    if response.double_clicked() {
        return CardAction::Launch;
    }
    if response.clicked() {
        return CardAction::Select;
    }
    CardAction::None
}

// ── installs page ─────────────────────────────────────────────────────────────

fn draw_installs_page(ui: &mut egui::Ui, hub: &mut HubState, pal: &Palette, tr: &Translations) {
    egui::ScrollArea::vertical()
        .id_salt("hub_installs_scroll")
        .show(ui, |ui| {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.label(
                    RichText::new(tr.tr("hub_installs_title"))
                        .size(24.0)
                        .strong()
                        .color(pal.text_primary),
                );
            });
            ui.add_space(24.0);

            let installs = hub.installs().to_vec();
            if installs.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(28.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(tr.tr("hub_installs_empty"))
                                .size(14.0)
                                .color(pal.text_secondary),
                        )
                        .wrap(),
                    );
                });
            } else {
                for install in installs {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(28.0);
                        let width = ui.available_width() - 28.0;
                        let (rect, _resp) =
                            ui.allocate_exact_size(Vec2::new(width, 56.0), egui::Sense::hover());
                        ui.painter().rect(
                            rect,
                            CornerRadius::same(8),
                            pal.surface,
                            Stroke::new(1.0, pal.border),
                            StrokeKind::Outside,
                        );

                        paint_text_in_rect(
                            ui,
                            Rect::from_min_max(
                                rect.min + Vec2::new(16.0, 8.0),
                                rect.max - Vec2::new(16.0, 28.0),
                            ),
                            &install.version,
                            FontId::proportional(15.0),
                            pal.text_primary,
                            Align2::LEFT_CENTER,
                        );

                        paint_text_in_rect(
                            ui,
                            Rect::from_min_max(
                                rect.min + Vec2::new(16.0, 29.0),
                                rect.max - Vec2::new(16.0, 8.0),
                            ),
                            install.path.to_string_lossy().as_ref(),
                            FontId::proportional(12.0),
                            pal.text_secondary,
                            Align2::LEFT_CENTER,
                        );
                    });
                }
            }
        });
}

// ── settings page ─────────────────────────────────────────────────────────────

fn draw_settings_page(ui: &mut egui::Ui, _hub: &mut HubState, pal: &Palette, tr: &Translations) {
    egui::ScrollArea::vertical()
        .id_salt("hub_settings_scroll")
        .show(ui, |ui| {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.label(
                    RichText::new(tr.tr("hub_settings_title"))
                        .size(24.0)
                        .strong()
                        .color(pal.text_primary),
                );
            });
            ui.add_space(24.0);

            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.label(
                    RichText::new(tr.tr("hub_settings_preferences"))
                        .size(16.0)
                        .strong()
                        .color(pal.text_primary),
                );
            });
            ui.add_space(12.0);

            // Language selector
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.label(
                    RichText::new(tr.tr("hub_settings_language"))
                        .size(14.0)
                        .color(pal.text_secondary),
                );
                let current = _hub.preferences().locale;
                for locale in engine_i18n::Locale::VARIANTS {
                    let selected = *locale == current;
                    if ui.selectable_label(selected, locale.label()).clicked() {
                        _hub.set_locale(*locale);
                    }
                }
            });
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(tr.tr("hub_settings_limited"))
                            .size(14.0)
                            .color(pal.text_secondary),
                    )
                    .wrap(),
                );
            });
        });
}

// ── dialogs ───────────────────────────────────────────────────────────────────

fn draw_new_project_dialog(
    ctx: &egui::Context,
    hub: &mut HubState,
    pal: &Palette,
    tr: &Translations,
) {
    if hub.new_project_dialog.is_none() {
        return;
    }

    let installs = hub.installs().to_vec();
    let templates = [
        (tr.tr("template_3d"), "three_d", tr.tr("template_3d_desc")),
        (tr.tr("template_2d"), "two_d", tr.tr("template_2d_desc")),
    ];
    let mut dialog = hub.new_project_dialog.take().unwrap();
    let mut is_open = true;
    let mut submit_req = None;
    let mut close_dialog = false;

    egui::Window::new(tr.tr("dialog_new_project"))
        .id(egui::Id::new("hub_new_project_dialog"))
        .collapsible(false)
        .resizable(false)
        .fixed_size(Vec2::new(560.0, 430.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            Frame::window(&ctx.style())
                .fill(pal.surface)
                .stroke(Stroke::new(1.0, pal.border)),
        )
        .open(&mut is_open)
        .show(ctx, |ui| {
            ui.set_width(528.0);
            ui.add_space(8.0);

            ui.add(
                egui::Label::new(
                    RichText::new(tr.tr("dialog_preset_hint"))
                        .size(13.0)
                        .color(pal.text_secondary),
                )
                .wrap(),
            );
            ui.add_space(16.0);

            ui.label(
                RichText::new(tr.tr("dialog_template"))
                    .size(12.0)
                    .strong()
                    .color(pal.text_secondary),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let card_gap = 8.0;
                let card_width = (ui.available_width() - card_gap) / templates.len() as f32;
                for (idx, (title, _, description)) in templates.iter().enumerate() {
                    let selected = dialog.template_idx == idx;
                    if template_choice(ui, title, description, selected, card_width, pal).clicked()
                    {
                        dialog.template_idx = idx;
                    }
                    if idx + 1 != templates.len() {
                        ui.add_space(card_gap);
                    }
                }
            });
            ui.add_space(16.0);

            ui.label(
                RichText::new(tr.tr("dialog_project_name"))
                    .size(12.0)
                    .strong()
                    .color(pal.text_secondary),
            );
            ui.add(
                egui::TextEdit::singleline(&mut dialog.name)
                    .hint_text(tr.tr("dialog_name_hint"))
                    .desired_width(ui.available_width()),
            );
            ui.add_space(12.0);

            ui.label(
                RichText::new(tr.tr("dialog_location"))
                    .size(12.0)
                    .strong()
                    .color(pal.text_secondary),
            );
            ui.horizontal(|ui| {
                let browse_width = 72.0;
                let gap_width = ui.spacing().item_spacing.x;
                let input_width = (ui.available_width() - browse_width - gap_width).max(120.0);
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.location)
                        .hint_text(tr.tr("dialog_location_hint"))
                        .desired_width(input_width),
                );
                if ui
                    .add_sized(
                        [browse_width, 22.0],
                        egui::Button::new(tr.tr("dialog_browse")),
                    )
                    .clicked()
                {
                    hub.pending_action = Some(HubAction::SelectProjectLocation);
                }
            });
            ui.add_space(12.0);

            if installs.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(tr.tr("dialog_warning_no_toolchain")).color(pal.danger),
                    )
                    .wrap(),
                );
            } else {
                dialog.version_idx = dialog.version_idx.min(installs.len().saturating_sub(1));
                ui.label(
                    RichText::new(tr.tr("dialog_toolchain"))
                        .size(12.0)
                        .strong()
                        .color(pal.text_secondary),
                );
                egui::ComboBox::from_id_salt("hub_new_project_toolchain_version")
                    .width(ui.available_width())
                    .selected_text(&installs[dialog.version_idx].version)
                    .show_ui(ui, |ui| {
                        for (i, install) in installs.iter().enumerate() {
                            ui.selectable_value(&mut dialog.version_idx, i, &install.version);
                        }
                    });
            }
            ui.add_space(12.0);

            if let Some(err) = &dialog.error {
                ui.add(egui::Label::new(RichText::new(err).color(pal.danger)).wrap());
                ui.add_space(8.0);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(RichText::new(tr.tr("dialog_create")).size(14.0).strong())
                    .clicked()
                {
                    dialog.template_idx =
                        dialog.template_idx.min(templates.len().saturating_sub(1));
                    submit_req = Some(NewProjectRequest {
                        name: dialog.name.clone(),
                        location: if dialog.location.is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(&dialog.location))
                        },
                        template_id: Some(templates[dialog.template_idx].1.to_owned()),
                        toolchain_version: installs
                            .get(dialog.version_idx)
                            .map(|i| i.version.clone()),
                    });
                }
                ui.add_space(8.0);
                if ui.button(tr.tr("dialog_cancel")).clicked() {
                    close_dialog = true;
                }
            });
        });

    if close_dialog {
        is_open = false;
    }

    if let Some(req) = submit_req {
        match hub.create_project_plan(&req) {
            Ok(plan) => match hub.create_project_files(&plan) {
                Ok(()) => {
                    hub.upsert_project(ProjectMetadata::new(
                        &plan.name,
                        &plan.path,
                        "just now",
                        &plan.toolchain_version,
                    ));
                    is_open = false;
                }
                Err(e) => {
                    dialog.error = Some(e.to_string());
                }
            },
            Err(e) => {
                dialog.error = Some(e.to_string());
            }
        }
    }

    if is_open {
        hub.new_project_dialog = Some(dialog);
    } else {
        hub.new_project_dialog = None;
    }
}

fn template_choice(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    selected: bool,
    width: f32,
    pal: &Palette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 92.0), egui::Sense::click());
    let bg = if selected {
        pal.surface_selected
    } else if response.hovered() {
        pal.surface_hover
    } else {
        pal.surface
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(8),
        bg,
        Stroke::new(1.0, if selected { pal.accent } else { pal.border }),
        StrokeKind::Outside,
    );
    paint_text_in_rect(
        ui,
        Rect::from_min_max(
            rect.min + Vec2::new(14.0, 10.0),
            rect.max - Vec2::new(14.0, 56.0),
        ),
        title,
        FontId::proportional(18.0),
        pal.text_primary,
        Align2::LEFT_CENTER,
    );
    paint_wrapped_text_in_rect(
        ui,
        Rect::from_min_max(
            rect.min + Vec2::new(14.0, 42.0),
            rect.max - Vec2::new(14.0, 10.0),
        ),
        description,
        FontId::proportional(12.0),
        pal.text_secondary,
    );
    response
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

fn paint_wrapped_text_in_rect(ui: &egui::Ui, rect: Rect, text: &str, font: FontId, color: Color32) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let galley = ui
        .painter()
        .layout(text.to_owned(), font, color, rect.width());
    ui.painter()
        .with_clip_rect(rect)
        .galley(rect.min, galley, color);
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

fn draw_confirm_delete_dialog(
    ctx: &egui::Context,
    hub: &mut HubState,
    pal: &Palette,
    tr: &Translations,
) {
    if hub.confirm_delete.is_none() {
        return;
    }

    let dialog = hub.confirm_delete.take().unwrap();
    let mut is_open = true;
    let mut remove_recent = false;
    let mut delete_files = false;
    let mut close_dialog = false;

    egui::Window::new(tr.tr("dialog_confirm_delete"))
        .id(egui::Id::new("hub_confirm_delete_dialog"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            Frame::window(&ctx.style())
                .fill(pal.surface)
                .stroke(Stroke::new(1.0, pal.border)),
        )
        .open(&mut is_open)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    RichText::new(tr.tr_fmt(
                        "dialog_confirm_message",
                        &[&dialog.path.display().to_string()],
                    ))
                    .color(pal.text_primary),
                )
                .wrap(),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui.button(tr.tr("dialog_remove_recents")).clicked() {
                    remove_recent = true;
                    close_dialog = true;
                }
                if ui
                    .button(RichText::new(tr.tr("dialog_delete_files")).color(pal.danger))
                    .clicked()
                {
                    delete_files = true;
                    close_dialog = true;
                }
                if ui.button(tr.tr("dialog_cancel")).clicked() {
                    close_dialog = true;
                }
            });
        });

    if close_dialog {
        is_open = false;
    }

    if remove_recent {
        let _ = hub.request_project_deletion(&dialog.path, ProjectDeletionMode::RemoveRecent, true);
    } else if delete_files {
        let _ = hub.request_project_deletion(&dialog.path, ProjectDeletionMode::DeleteFiles, true);
    }

    if is_open {
        hub.confirm_delete = Some(dialog);
    } else {
        hub.confirm_delete = None;
    }
}
