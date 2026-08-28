//! Icons, from Remix Icon.
//!
//! The obvious way is a character — `✕`, `↗`, `☰` — and it works until the
//! font does not have one, which is what happened here: three buttons rendered
//! as empty squares because egui's bundled faces cover Latin and little else,
//! and the CJK fallback loaded for wallpaper titles carries no arrows.
//!
//! So the glyphs are brought along. `assets/remixicon.ttf` is Remix Icon cut
//! down to the codepoints below — 880 bytes rather than the 599 KB of all
//! 3,229 — and lives in a font family of its own, because these are
//! private-use codepoints where a text font may have something else entirely.
//!
//! To add one: find its name at <https://remixicon.com>, take the codepoint
//! from `remixicon.glyph.json`, add it to `Icon` here and to the list in
//! `assets/remixicon.sh`, then run that script to cut a new subset.

use egui::{FontFamily, FontId, Response, Sense, Ui, Vec2};

use crate::theme;

/// How big an icon button is.
const BUTTON: f32 = 22.0;

/// How big the glyph inside it is drawn.
///
/// A Remix glyph is designed on a 24-unit box and its strokes sit inside about
/// half of that, so the drawn mark comes out near half this number.
const GLYPH: f32 = 20.0;

/// What an icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// A cross, for closing. `close-line`.
    Close,
    /// An arrow leaving a box, for opening something elsewhere.
    /// `external-link-line`.
    External,
    /// Three lines, for the panel. `menu-line`.
    Menu,
    /// A chevron back, for the previous page. `arrow-left-s-line`.
    Previous,
    /// A chevron on, for the next page. `arrow-right-s-line`.
    Next,
}

impl Icon {
    /// The codepoint this icon is at in the bundled font.
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

    /// Every icon, for the tests that check the font carries all of them.
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

/// The font an icon is drawn in.
fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(theme::ICONS.into()))
}

/// One icon as a `RichText`, for putting inside a button of someone else's.
#[must_use]
pub fn text(icon: Icon) -> egui::RichText {
    egui::RichText::new(icon.glyph()).font(font(16.0))
}

/// Draws one icon as a button.
pub fn button(ui: &mut Ui, icon: Icon, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::click());

    let visuals = ui.style().interact_selectable(&response, active);
    ui.painter()
        .rect_filled(rect, visuals.rounding, visuals.weak_bg_fill);

    // Centred on the glyph box rather than the baseline: the face's ascent and
    // descent are symmetrical about the middle of a Remix glyph, so this lands
    // the mark in the middle of the button at any size.
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
        // Two icons on one codepoint is a copy-paste that draws the wrong
        // picture and nothing complains.
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
        // The font is cut down to exactly these codepoints; adding an `Icon`
        // without re-cutting it draws an empty box, which is the bug this
        // module exists to fix.
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
