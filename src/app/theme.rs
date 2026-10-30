//! The Roch look, in one place.
//!
//! Every colour is a (light, dark) pair, as it was for CustomTkinter, and is
//! resolved against the appearance mode when it is drawn -- so switching mode
//! repaints everything without anything having to be re-applied.
//!
//! The palette is the one Roch Viewer uses, kept identical on purpose -- the
//! programs sit side by side on the same desktop. Dark surfaces, grey chrome,
//! and red reserved for the thing you actually need to look at: here, a
//! failure, which is the one event this program exists to make unmissable.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Theme, Visuals,
};

/// A colour for each appearance mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair(pub Color32, pub Color32);

impl Pair {
    pub fn get(self, dark: bool) -> Color32 {
        if dark {
            self.1
        } else {
            self.0
        }
    }
}

const fn hex(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

const fn pair(light: u32, dark: u32) -> Pair {
    Pair(hex(light), hex(dark))
}

// Surfaces, lightest content plane first. The steps between the dark values
// are small because a dark theme reads as flat when they are not.
pub const BG_COLOR: Pair = pair(0xF1F5F9, 0x161616);
pub const BG_COLOR2: Pair = pair(0xFFFFFF, 0x1C1C1C);
pub const SECTION_COLOR: Pair = pair(0xE2E8F0, 0x1C1C1C);
pub const ROW_COLOR: Pair = pair(0xF8FAFC, 0x202020);
pub const BORDER_COLOR: Pair = pair(0xCBD5E1, 0x101010);
pub const HIGHLIGHT_COLOR: Pair = pair(0xE8EEF5, 0x1D1D1D);
pub const HEADER_COLOR: Pair = pair(0xE2E8F0, 0x222222);

// Type.
pub const TEXT_COLOR: Pair = pair(0x0F172A, 0xFFFFFF);
pub const SUBTITLE_COLOR: Pair = pair(0x475569, 0xB0B0B0);
pub const VALUE_COLOR: Pair = pair(0xB91C1C, 0xFF4D4D);
pub const RULE_COLOR: Pair = pair(0x94A3B8, 0x4A4A4A);

// Tabs and buttons.
pub const TAB_SELECTED_COLOR: Pair = pair(0x2563EB, 0x1A3C5D);
pub const TAB_UNSELECTED_COLOR: Pair = pair(0xD7E1EC, 0x2E2E2E);
pub const TAB_HOVER_COLOR: Pair = pair(0x3B82F6, 0x2A5579);
pub const TAB_UNSELECTED_HOVER_COLOR: Pair = pair(0xC5D2E0, 0x3A3A3A);

// Run states. Idle is deliberately quiet, and the three outcomes are the only
// saturated colours in the program.
pub const IDLE_COLOR: Pair = SUBTITLE_COLOR;
pub const RUNNING_COLOR: Pair = pair(0x2563EB, 0x6AA9FF);
pub const PASS_COLOR: Pair = pair(0x15803D, 0x4ADE80);
pub const FAIL_COLOR: Pair = pair(0xB91C1C, 0xFF4D4D);
pub const WARN_COLOR: Pair = pair(0xB45309, 0xFBBF24);

// The Start button is the one control that should read as a button rather
// than as chrome, so it gets the accent rather than the unselected-tab grey.
pub const START_COLOR: Pair = pair(0x15803D, 0x1F5F38);
pub const START_HOVER_COLOR: Pair = pair(0x166534, 0x2A7A48);
pub const STOP_COLOR: Pair = pair(0xB91C1C, 0x5D1A1A);
pub const STOP_HOVER_COLOR: Pair = pair(0x991B1B, 0x7A2626);

/// Consolas throughout, for the same reason Roch Viewer uses it: these
/// panels are full of numbers that want to line up in a column.
pub const FONT_FAMILY: &str = "Consolas";
pub const FONT_SIZE: f32 = 12.0;
pub const TITLE_SIZE: f32 = 14.0;
pub const LOG_SIZE: f32 = 11.0;
pub const HINT_SIZE: f32 = 10.0;

/// The family name registered for Consolas Bold.
pub const BOLD: &str = "bold";

pub const ROW_HEIGHT: f32 = 18.0;
pub const LABEL_MINSIZE: f32 = 150.0;
pub const FIELD_WIDTH: f32 = 130.0;

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

/// Register Consolas (and Consolas Bold) from the Windows font folder, in
/// front of egui's own fonts, which stay as the fallback for anything
/// Consolas lacks.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let folder = std::path::Path::new(&windir).join("Fonts");
    let regular = std::fs::read(folder.join("consola.ttf")).ok();
    let heavy = std::fs::read(folder.join("consolab.ttf")).ok();

    let fallback: Vec<String> = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    if let Some(bytes) = regular {
        fonts
            .font_data
            .insert(FONT_FAMILY.into(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Monospace, FontFamily::Proportional] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, FONT_FAMILY.into());
        }
    }
    let mut bold_family = Vec::new();
    if let Some(bytes) = heavy {
        fonts.font_data.insert(
            "Consolas Bold".into(),
            Arc::new(FontData::from_owned(bytes)),
        );
        bold_family.push("Consolas Bold".to_string());
    } else if fonts.font_data.contains_key(FONT_FAMILY) {
        bold_family.push(FONT_FAMILY.to_string());
    }
    bold_family.extend(fallback);
    fonts
        .families
        .insert(FontFamily::Name(BOLD.into()), bold_family);
    ctx.set_fonts(fonts);
}

/// Apply the palette and the type sizes to both of egui's themes.
pub fn install_style(ctx: &egui::Context) {
    for theme in [Theme::Dark, Theme::Light] {
        let dark = theme == Theme::Dark;
        ctx.style_mut_of(theme, |style| {
            style.text_styles = [
                (TextStyle::Small, font(HINT_SIZE)),
                (TextStyle::Body, font(FONT_SIZE)),
                (TextStyle::Monospace, font(LOG_SIZE)),
                (TextStyle::Button, bold(FONT_SIZE)),
                (TextStyle::Heading, bold(TITLE_SIZE)),
            ]
            .into();
            style.spacing.item_spacing = egui::vec2(6.0, 3.0);
            style.spacing.interact_size.y = 20.0;
            style.spacing.button_padding = egui::vec2(6.0, 2.0);
            style.visuals = visuals(dark);
        });
    }
}

fn visuals(dark: bool) -> Visuals {
    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let text = TEXT_COLOR.get(dark);
    visuals.override_text_color = Some(text);
    visuals.panel_fill = BG_COLOR.get(dark);
    visuals.window_fill = BG_COLOR2.get(dark);
    visuals.extreme_bg_color = BG_COLOR2.get(dark);
    visuals.faint_bg_color = ROW_COLOR.get(dark);
    visuals.code_bg_color = HIGHLIGHT_COLOR.get(dark);
    visuals.hyperlink_color = VALUE_COLOR.get(dark);
    visuals.selection.bg_fill = TAB_SELECTED_COLOR.get(dark);
    visuals.selection.stroke = Stroke::new(1.0_f32, text);
    let radius = CornerRadius::same(4);
    for (widget, fill) in [
        (
            &mut visuals.widgets.inactive,
            TAB_UNSELECTED_COLOR.get(dark),
        ),
        (
            &mut visuals.widgets.hovered,
            TAB_UNSELECTED_HOVER_COLOR.get(dark),
        ),
        (&mut visuals.widgets.active, TAB_HOVER_COLOR.get(dark)),
        (
            &mut visuals.widgets.open,
            TAB_UNSELECTED_HOVER_COLOR.get(dark),
        ),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.corner_radius = radius;
        widget.fg_stroke = Stroke::new(1.0_f32, text);
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_COLOR.get(dark));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, text);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_COLOR.get(dark));
    visuals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_resolve_per_mode() {
        assert_eq!(BG_COLOR.get(true), Color32::from_rgb(0x16, 0x16, 0x16));
        assert_eq!(BG_COLOR.get(false), Color32::from_rgb(0xF1, 0xF5, 0xF9));
        assert_eq!(FAIL_COLOR.get(true), Color32::from_rgb(0xFF, 0x4D, 0x4D));
    }
}
