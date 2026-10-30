//! The small widgets every tool panel is built out of.
//!
//! Each of these takes its colours from `theme` rather than being handed a
//! palette, so a colour is changed in one place and the whole program follows.
//! The point of the file is that a tool panel should be a list of fields, not
//! a hundred lines of layout arithmetic.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, Margin, Response, RichText, Sense, Ui, Vec2,
};

use crate::app::panel::{FieldRow, RowValue};
use crate::app::theme::{self, Pair};
use crate::core::toolbase::FieldKind;

/// A titled block with a rule under the heading; the closure draws the body.
pub fn section<R>(ui: &mut Ui, dark: bool, title: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::SECTION_COLOR.get(dark))
        .corner_radius(5)
        .inner_margin(Margin {
            left: 7,
            right: 7,
            top: 3,
            bottom: 4,
        })
        .outer_margin(Margin {
            left: 4,
            right: 4,
            top: 0,
            bottom: 3,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .font(theme::bold(theme::FONT_SIZE))
                    .color(theme::TEXT_COLOR.get(dark)),
            );
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, 0.0, theme::RULE_COLOR.get(dark));
            ui.add_space(2.0);
            body(ui)
        })
        .inner
}

/// Plain text in the row font.
pub fn text(ui: &mut Ui, text: &str, colour: Pair, dark: bool, bold: bool) -> Response {
    let font = if bold {
        theme::bold(theme::FONT_SIZE)
    } else {
        theme::font(theme::FONT_SIZE)
    };
    ui.label(RichText::new(text).font(font).color(colour.get(dark)))
}

/// Grey explanatory text under a field. Wrapped rather than truncated: these
/// say why a setting exists, and half a sentence is worse than none.
pub fn hint(ui: &mut Ui, text: &str, dark: bool, wrap: f32) {
    if text.is_empty() {
        return;
    }
    ui.scope(|ui| {
        ui.set_max_width(wrap);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(theme::font(theme::HINT_SIZE))
                    .color(theme::SUBTITLE_COLOR.get(dark)),
            )
            .wrap(),
        );
    });
}

/// The three roles this program has for a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Start,
    Stop,
    Normal,
}

/// A button's three colours: its face, its face under the pointer, its text.
struct Colours {
    fill: Color32,
    hover: Color32,
    text: Color32,
}

fn paint_button(
    ui: &mut Ui,
    label: &str,
    size: Vec2,
    enabled: bool,
    colours: Colours,
    radius: u8,
) -> Response {
    let Colours { fill, hover, text } = colours;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let colour = if !enabled {
            fill.gamma_multiply(0.45)
        } else if response.hovered() {
            hover
        } else {
            fill
        };
        let text = if enabled {
            text
        } else {
            text.gamma_multiply(0.55)
        };
        ui.painter().rect_filled(rect, radius, colour);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::bold(theme::FONT_SIZE),
            text,
        );
    }
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A button in one of the three roles, the size CustomTkinter drew it.
pub fn action_button(
    ui: &mut Ui,
    label: &str,
    kind: ButtonKind,
    width: f32,
    enabled: bool,
    dark: bool,
) -> Response {
    let (fill, hover) = match kind {
        ButtonKind::Start => (theme::START_COLOR, theme::START_HOVER_COLOR),
        ButtonKind::Stop => (theme::STOP_COLOR, theme::STOP_HOVER_COLOR),
        ButtonKind::Normal => (
            theme::TAB_UNSELECTED_COLOR,
            theme::TAB_UNSELECTED_HOVER_COLOR,
        ),
    };
    let text = if kind == ButtonKind::Normal {
        theme::TEXT_COLOR.get(dark)
    } else {
        Color32::WHITE
    };
    // Wide enough for its label whatever the font turns out to be.
    let needed = ui
        .fonts_mut(|f| f.layout_no_wrap(label.to_string(), theme::bold(theme::FONT_SIZE), text))
        .size()
        .x
        + 16.0;
    paint_button(
        ui,
        label,
        egui::vec2(width.max(needed), 24.0),
        enabled,
        Colours {
            fill: fill.get(dark),
            hover: hover.get(dark),
            text,
        },
        6,
    )
}

/// One segment of the tab bar.
pub fn tab_button(ui: &mut Ui, label: &str, selected: bool, dark: bool) -> Response {
    let (fill, hover) = if selected {
        (theme::TAB_SELECTED_COLOR, theme::TAB_HOVER_COLOR)
    } else {
        (
            theme::TAB_UNSELECTED_COLOR,
            theme::TAB_UNSELECTED_HOVER_COLOR,
        )
    };
    let text = if selected && !dark {
        Color32::WHITE
    } else {
        theme::TEXT_COLOR.get(dark)
    };
    let width = ui
        .fonts_mut(|f| f.layout_no_wrap(label.to_string(), theme::bold(11.0), text))
        .size()
        .x
        + 22.0;
    let sense = Sense::click();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 26.0), sense);
    if ui.is_rect_visible(rect) {
        let colour = if response.hovered() {
            hover.get(dark)
        } else {
            fill.get(dark)
        };
        ui.painter().rect_filled(rect, 8, colour);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::bold(11.0),
            text,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// An on/off switch, as CTkSwitch drew it.
pub fn toggle(ui: &mut Ui, on: &mut bool, dark: bool) -> Response {
    let size = egui::vec2(36.0, 18.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        let track = if *on {
            theme::TAB_SELECTED_COLOR.get(dark)
        } else {
            theme::TAB_UNSELECTED_COLOR.get(dark)
        };
        let track = if ui.is_enabled() {
            track
        } else {
            track.gamma_multiply(0.5)
        };
        let radius = rect.height() / 2.0;
        ui.painter().rect_filled(rect, radius, track);
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let knob = if dark {
            Color32::from_gray(230)
        } else {
            Color32::WHITE
        };
        ui.painter()
            .circle_filled(egui::pos2(x, rect.center().y), radius - 3.0, knob);
    }
    if response.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A clickable word in the accent red, for the links in the corner.
pub fn link(ui: &mut Ui, text: &str, dark: bool) -> Response {
    let response = ui.add(
        egui::Label::new(
            RichText::new(text)
                .font(theme::bold(theme::FONT_SIZE))
                .color(theme::VALUE_COLOR.get(dark)),
        )
        .sense(Sense::click()),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A single-line entry box.
pub fn entry(ui: &mut Ui, value: &mut String, width: f32, hint_text: &str) -> Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(width)
            .font(theme::font(theme::FONT_SIZE))
            .hint_text(hint_text)
            .margin(egui::vec2(4.0, 2.0)),
    )
}

/// A drop-down of *choices*. True when the selection changed.
pub fn choice(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    selected: &mut String,
    choices: &[String],
    width: f32,
) -> bool {
    let before = selected.clone();
    egui::ComboBox::from_id_salt(id)
        .selected_text(RichText::new(selected.as_str()).font(theme::font(theme::FONT_SIZE)))
        .width(width)
        .show_ui(ui, |ui| {
            for option in choices {
                ui.selectable_value(
                    selected,
                    option.clone(),
                    RichText::new(option.as_str()).font(theme::font(theme::FONT_SIZE)),
                );
            }
        });
    *selected != before
}

/// One field's row in a three-column grid: label, control, unit. True when
/// the value was changed by hand.
pub fn field_row(ui: &mut Ui, id: &str, row: &mut FieldRow, dark: bool) -> bool {
    text(ui, row.field.label, theme::TEXT_COLOR, dark, false);
    let mut changed = false;
    let enabled = row.enabled;
    ui.add_enabled_ui(enabled, |ui| match (&mut row.value, row.field.kind) {
        (RowValue::Multi(ticks), _) => {
            // A tick per choice, two to a row. y-cruncher's algorithms are not
            // alternatives -- any combination is a valid run -- so a dropdown
            // would be the wrong shape for them entirely.
            // Rows of fixed-width cells rather than a grid inside the grid,
            // which egui sizes a frame late.
            let choices = &row.field.choices;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (pair, ticks) in choices.chunks(2).zip(ticks.chunks_mut(2)) {
                    ui.horizontal(|ui| {
                        for (choice, tick) in pair.iter().zip(ticks.iter_mut()) {
                            ui.allocate_ui_with_layout(
                                egui::vec2(88.0, 18.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_min_width(88.0);
                                    let text = RichText::new(choice.as_str())
                                        .font(theme::font(theme::FONT_SIZE));
                                    if ui.checkbox(tick, text).changed() {
                                        changed = true;
                                    }
                                },
                            );
                        }
                    });
                }
            });
        }
        (RowValue::Bool(flag), _) => {
            if toggle(ui, flag, dark).changed() {
                changed = true;
            }
        }
        (RowValue::Choice(selected), _) => {
            if choice(
                ui,
                id,
                selected,
                &row.field.choices,
                theme::FIELD_WIDTH + 60.0,
            ) {
                changed = true;
            }
        }
        (RowValue::Text(value), kind) => {
            // Numbers need room for six digits; free text holds things like a
            // KMP_AFFINITY string, which is unreadable at the numeric width.
            let width = if kind == FieldKind::Text {
                theme::FIELD_WIDTH + 190.0
            } else {
                theme::FIELD_WIDTH
            };
            if entry(ui, value, width, "").changed() {
                changed = true;
            }
        }
    });
    if row.field.unit.is_empty() {
        ui.label("");
    } else {
        text(ui, row.field.unit, theme::SUBTITLE_COLOR, dark, false);
    }
    changed
}
