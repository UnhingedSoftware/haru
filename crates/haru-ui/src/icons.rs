//! Small icons, drawn rather than typed.
//!
//! The obvious way is a character — `✕`, `↗`, `☰` — and it works until the
//! font does not have one, which is what happened here: three buttons rendered
//! as empty squares because egui's bundled faces cover Latin and little else,
//! and the CJK fallback loaded for wallpaper titles does not carry arrows.
//!
//! Four lines of painter cost less than a font that has to be shipped, and
//! they cannot go missing.

use egui::{Rect, Response, Sense, Stroke, Ui, Vec2, vec2};

/// How big an icon button is.
const BUTTON: f32 = 22.0;

/// How much of it the glyph fills.
const GLYPH: f32 = 0.42;

/// What an icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// A cross, for closing.
    Close,
    /// An arrow leaving a box, for opening something elsewhere.
    External,
    /// Three lines, for the panel.
    Menu,
}

/// Draws one icon as a button.
pub fn button(ui: &mut Ui, icon: Icon, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::click());

    let visuals = ui.style().interact_selectable(&response, active);
    ui.painter()
        .rect_filled(rect, visuals.rounding, visuals.weak_bg_fill);

    let stroke = Stroke::new(1.4_f32, visuals.fg_stroke.color);
    paint(ui, icon, rect, stroke);
    response
}

/// Draws the glyph itself inside `rect`.
fn paint(ui: &Ui, icon: Icon, rect: Rect, stroke: Stroke) {
    let painter = ui.painter();
    let middle = rect.center();
    let reach = rect.width() * GLYPH;

    match icon {
        Icon::Close => {
            painter.line_segment(
                [middle + vec2(-reach, -reach), middle + vec2(reach, reach)],
                stroke,
            );
            painter.line_segment(
                [middle + vec2(reach, -reach), middle + vec2(-reach, reach)],
                stroke,
            );
        }
        Icon::External => {
            // An arrow to the top right, with the corner it leaves behind.
            let from = middle + vec2(-reach, reach);
            let to = middle + vec2(reach, -reach);
            painter.line_segment([from, to], stroke);
            painter.line_segment([to, to + vec2(-reach * 0.9, 0.0)], stroke);
            painter.line_segment([to, to + vec2(0.0, reach * 0.9)], stroke);
        }
        Icon::Menu => {
            for offset in [-reach * 0.8, 0.0, reach * 0.8] {
                painter.line_segment(
                    [middle + vec2(-reach, offset), middle + vec2(reach, offset)],
                    stroke,
                );
            }
        }
    }
}
