//! How the window looks.
//!
//! Dark, translucent and rounded. The translucency is the point: a picker sits
//! over a desktop whose wallpaper is the thing being chosen, and letting some
//! of it through keeps that in view. On a compositor with blur — Hyprland,
//! KWin, macOS — the same alpha reads as frosted glass; without one it is a
//! plain dark panel, which is why nothing here depends on blur existing.
//!
//! Every colour is defined once, here, so a widget never invents one.

use egui::{Color32, Rounding, Stroke, Vec2};

/// The accent: selection, focus, anything asking to be clicked.
pub const ACCENT: Color32 = Color32::from_rgb(0x8B, 0x7C, 0xFF);

/// What a button that takes something away is drawn in.
pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x6B, 0x6F);

/// Body text.
pub const TEXT: Color32 = Color32::from_rgb(0xE9, 0xE9, 0xF2);

/// Text that should recede: counts, tags, captions.
pub const MUTED: Color32 = Color32::from_rgb(0x96, 0x96, 0xA8);

/// The window behind everything, translucent.
pub const BACKDROP: Color32 = Color32::from_rgba_premultiplied(9, 9, 13, 214);

/// A panel over the backdrop.
const PANEL: Color32 = Color32::from_rgba_premultiplied(15, 15, 21, 205);

/// A surface that should read as raised: a card, a well, a text field.
const SURFACE: Color32 = Color32::from_rgba_premultiplied(30, 30, 40, 190);

/// The same, under the pointer.
const SURFACE_HOVER: Color32 = Color32::from_rgba_premultiplied(44, 44, 58, 205);

/// Hairlines between surfaces.
const HAIRLINE: Color32 = Color32::from_rgba_premultiplied(58, 58, 74, 150);

/// What the bundled icon face is called, as a font and as a family.
pub const ICONS: &str = "icons";

/// Where a font that covers Chinese, Japanese and Korean lives, per platform.
///
/// Most Workshop titles are CJK — a page of Wallpaper Engine's most-subscribed
/// is mostly Chinese — and egui ships Latin glyphs only, so without one of
/// these every other title is a row of empty boxes.
///
/// Loaded from the system rather than bundled: a CJK face is 18 MB, which is
/// more than the whole binary, and every desktop that can display those titles
/// anywhere already has one.
const CJK_FONTS: [&str; 8] = [
    // Arch, Fedora
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    // Debian, Ubuntu
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    // openSUSE
    "/usr/share/fonts/truetype/NotoSansCJK-Regular.ttc",
    // macOS
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    // Windows
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/meiryo.ttc",
];

/// Loads the fonts: the icon face, and a CJK fallback if this machine has one.
///
/// The icon face is bundled because it has to be — it is five glyphs no system
/// ships. The CJK face is not, and must not be: it is 18 MB, more than the
/// whole binary, and every desktop that can display those titles anywhere
/// already has one.
///
/// Both are fallbacks rather than replacements: egui's own face is what the
/// interface is designed in, and it stays first.
fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        ICONS.to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/remixicon.ttf")),
    );
    // Its own family, not a fallback on the text families: an icon codepoint
    // is in the private-use area, where a text font may have something else
    // entirely, and whichever font is asked first would win.
    fonts
        .families
        .insert(egui::FontFamily::Name(ICONS.into()), vec![ICONS.to_owned()]);

    if let Some(bytes) = CJK_FONTS.iter().find_map(|path| std::fs::read(path).ok()) {
        fonts.font_data.insert(
            "cjk".to_owned(),
            // These are collections; index 0 is the face the rest are weights of.
            egui::FontData::from_owned(bytes).tweak(egui::FontTweak::default()),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("cjk".to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

/// Applies the theme to a context.
pub fn apply(ctx: &egui::Context) {
    load_fonts(ctx);

    let mut style = (*ctx.style()).clone();
    let visuals = &mut style.visuals;

    *visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = SURFACE;
    visuals.faint_bg_color = Color32::from_rgba_premultiplied(255, 255, 255, 8);
    visuals.override_text_color = Some(TEXT);
    visuals.window_rounding = Rounding::same(12.0);
    visuals.menu_rounding = Rounding::same(10.0);
    visuals.window_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);

    // Six, not eight: a text field is a wide short box, and a corner that
    // round eats into the first and last glyph of what is typed in it.
    let rounding = Rounding::same(6.0);
    visuals.widgets.noninteractive.rounding = rounding;
    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);

    visuals.widgets.inactive.rounding = rounding;
    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    visuals.widgets.hovered.rounding = rounding;
    visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.weak_bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.6));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);

    visuals.widgets.active.rounding = rounding;
    visuals.widgets.active.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);

    visuals.widgets.open.rounding = rounding;
    visuals.widgets.open.bg_fill = SURFACE_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.6));

    // A shadow under menus, so a popover reads as floating over the grid
    // rather than punched into it.
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: Vec2::new(0.0, 6.0),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(140),
    };
    visuals.window_shadow = visuals.popup_shadow;

    // Room to breathe: the default spacing is tuned for dense tool panels, and
    // this is a picker people look at.
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    // Text starts inside the box rather than under its corner.
    style.spacing.text_edit_width = 200.0;
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.menu_margin = egui::Margin::same(6.0);
    style.spacing.window_margin = egui::Margin::same(10.0);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.combo_height = 360.0;

    ctx.set_style(style);
}

/// The frame a panel draws itself with: translucent, hairlined, padded.
pub fn panel_frame(side: Side) -> egui::Frame {
    egui::Frame::none()
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .stroke(match side {
            Side::Left | Side::Right => Stroke::new(1.0_f32, HAIRLINE),
            Side::Middle => Stroke::NONE,
        })
}

/// Which edge a panel is on, for the hairline that separates it.
pub enum Side {
    /// The filter sidebar.
    Left,
    /// The detail pane.
    Right,
    /// The grid.
    Middle,
}
