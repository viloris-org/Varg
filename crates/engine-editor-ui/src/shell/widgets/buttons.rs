//! Button widgets for the editor shell UI.

use egui::{Color32, RichText, Vec2};

use super::super::types::InfernuxPalette;
use super::icons::ui;

/// Renders a tool button with icon and keyboard shortcut in tooltip.
pub fn tool_button(
    ui: &mut egui::Ui,
    icon: &str,
    shortcut: &str,
    tooltip: &str,
    active: bool,
    pal: &InfernuxPalette,
) -> egui::Response {
    let icon_text = RichText::new(icon).size(16.0).color(if active { pal.text } else { pal.text_dim });

    let response = ui.add(
        egui::Button::new(icon_text)
            .fill(if active { pal.selection } else { pal.frame_bg })
            .stroke(if active {
                egui::Stroke::new(1.0, pal.border_highlight)
            } else {
                egui::Stroke::NONE
            })
            .min_size(Vec2::new(32.0, 28.0)),
    );

    response.on_hover_text(format!("{} ({})", tooltip, shortcut))
}

/// Renders a dropdown pill button.
pub fn dropdown_pill(ui: &mut egui::Ui, label: &str, width: f32, pal: &InfernuxPalette) -> egui::Response {
    let text = format!("{} {}", label, ui::DROPDOWN);
    ui.add(
        egui::Button::new(RichText::new(text).size(12.0).color(pal.text))
            .fill(pal.frame_bg)
            .min_size(Vec2::new(width, 24.0)),
    )
}

/// Renders a ghost button with transparent background.
pub fn ghost_button(ui: &mut egui::Ui, label: &str, width: f32, pal: &InfernuxPalette) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(pal.text))
            .fill(Color32::TRANSPARENT)
            .min_size(Vec2::new(width, 22.0)),
    )
}

/// Creates a small text button widget.
pub fn small_text_button_widget(label: &str, pal: &InfernuxPalette) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).size(12.0).color(pal.text))
        .fill(pal.frame_bg)
        .min_size(Vec2::new(60.0, 24.0))
}

/// Renders a small chip button.
pub fn small_chip(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
    pal: &InfernuxPalette,
) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(pal.text))
            .fill(pal.frame_bg)
            .min_size(Vec2::new(width, 22.0)),
    )
}

/// Renders a panel toggle button.
pub fn panel_toggle(ui: &mut egui::Ui, label: &str, state: &mut bool, pal: &InfernuxPalette) {
    let (fill, text_color) = if *state {
        (pal.selection, pal.text)
    } else {
        (pal.frame_bg, pal.text_dim)
    };

    let response = ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(text_color))
            .fill(fill)
            .min_size(Vec2::new(64.0, 24.0)),
    );

    if response.clicked() {
        *state = !*state;
    }
}
