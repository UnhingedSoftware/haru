use egui::{FontFamily, FontId, Response, Sense, Ui, Vec2};

use crate::theme;

const BUTTON: f32 = 22.0;

const GLYPH: f32 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Close,
    External,
    Menu,
    Previous,
    Next,
}

impl Icon {
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            Self::Close => '\u{eb99}',
            Self::External => '\u{ecaf}',
            Self::Menu => '\u{ef3e}',
            Self::Previous => '\u{ea64}',
            Self::Next => '\u{ea6e}',
        }
    }

    #[cfg(test)]
    const fn all() -> [Self; 5] {
        [
            Self::Close,
            Self::External,
            Self::Menu,
            Self::Previous,
            Self::Next,
        ]
    }
}

fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(theme::ICONS.into()))
}

#[must_use]
pub fn text(icon: Icon) -> egui::RichText {
    egui::RichText::new(icon.glyph()).font(font(16.0))
}

pub fn button(ui: &mut Ui, icon: Icon, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::click());

    let visuals = ui.style().interact_selectable(&response, active);
    ui.painter()
        .rect_filled(rect, visuals.rounding, visuals.weak_bg_fill);

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon.glyph(),
        font(GLYPH),
        visuals.fg_stroke.color,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_its_own_glyph() {
        let mut seen = Vec::new();
        for icon in Icon::all() {
            assert!(
                !seen.contains(&icon.glyph()),
                "{icon:?} repeats a codepoint"
            );
            seen.push(icon.glyph());
        }
    }

    #[test]
    fn every_glyph_is_in_the_bundled_subset() {
        let font = include_bytes!("../assets/remixicon.ttf");
        let parsed = ttf_parser::Face::parse(font, 0);
        assert!(parsed.is_ok(), "assets/remixicon.ttf does not parse");
        if let Ok(face) = parsed {
            for icon in Icon::all() {
                assert!(
                    face.glyph_index(icon.glyph()).is_some(),
                    "{icon:?} is not in assets/remixicon.ttf — re-run assets/remixicon.sh"
                );
            }
        }
    }
}
