//! Font configuration for CJK text support.
//!
//! egui's built-in fonts lack CJK coverage. This module provides the
//! [`setup_egui_fonts`] function that adds a bundled fallback font so
//! Chinese text renders correctly instead of as tofu (missing-glyph boxes).

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// Configures [`egui::Context`] font definitions to support CJK characters.
///
/// Must be called **once** on the egui context **before** any frames are drawn.
/// Adds the bundled DroidSansFallback font (which covers CJK) as a fallback
/// for both the `Proportional` and `Monospace` font families.
pub fn setup_egui_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "DroidSansFallback".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../fonts/DroidSansFallbackFull.ttf"
        ))),
    );

    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.push("DroidSansFallback".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
        family.push("DroidSansFallback".to_owned());
    }

    ctx.set_fonts(fonts);
}
