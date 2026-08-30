use egui::{Color32, Rounding, Stroke, Vec2};

pub const ACCENT: Color32 = Color32::from_rgb(0x7C, 0x5C, 0xFF);

pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x24, 0x1E, 0x45);

pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x6B, 0x6F);

pub const LOVE: Color32 = Color32::from_rgb(0xF2, 0x55, 0x7A);

pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xF0);

pub const MUTED: Color32 = Color32::from_rgb(0x8B, 0x8B, 0x9E);

pub const BACKDROP: Color32 = Color32::from_rgba_premultiplied(6, 6, 10, 220);

pub const MODAL: Color32 = Color32::from_rgb(0x14, 0x14, 0x1D);

pub const BASE: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x12);

const PANEL: Color32 = Color32::from_rgb(0x10, 0x10, 0x18);

pub const CARD: Color32 = Color32::from_rgb(0x16, 0x16, 0x1F);

const SURFACE: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x24);

const SURFACE_HOVER: Color32 = Color32::from_rgb(0x24, 0x24, 0x30);

pub const HAIRLINE: Color32 = Color32::from_rgb(0x23, 0x23, 0x2E);

pub const ICONS: &str = "icons";

const CJK_FONTS: [&str; 8] = [
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/NotoSansCJK-Regular.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/meiryo.ttc",
];

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        ICONS.to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/remixicon.ttf")),
    );
    fonts
        .families
        .insert(egui::FontFamily::Name(ICONS.into()), vec![ICONS.to_owned()]);

    if let Some(bytes) = CJK_FONTS.iter().find_map(|path| std::fs::read(path).ok()) {
        fonts.font_data.insert(
            "cjk".to_owned(),
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
    visuals.window_rounding = Rounding::same(14.0);
    visuals.menu_rounding = Rounding::same(10.0);
    visuals.window_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);

    let rounding = Rounding::same(9.0);
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

    visuals.popup_shadow = egui::epaint::Shadow {
        offset: Vec2::new(0.0, 6.0),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(140),
    };
    visuals.window_shadow = visuals.popup_shadow;

    style.spacing.item_spacing = Vec2::new(8.0, 9.0);
    style.spacing.text_edit_width = 200.0;
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.menu_margin = egui::Margin::same(6.0);
    style.spacing.window_margin = egui::Margin::same(10.0);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.combo_height = 360.0;

    ctx.set_style(style);
}

/// A raised block with its own edge — tiles, the detail pane, filter groups.
pub fn card(rounding: f32) -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .rounding(Rounding::same(rounding))
        .stroke(Stroke::new(1.0_f32, HAIRLINE))
}

/// A small rounded label: a tag, a resolution, "Audio responsive".
pub fn chip(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    let (fill, ink) = if on {
        (ACCENT_SOFT, ACCENT)
    } else {
        (SURFACE, MUTED)
    };
    egui::Frame::none()
        .fill(fill)
        .rounding(Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).color(ink));
        })
        .response
}

pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(13.0).strong().color(TEXT));
}

pub fn panel_frame(side: Side) -> egui::Frame {
    egui::Frame::none()
        .fill(match side {
            Side::Middle => BASE,
            Side::Left | Side::Right => PANEL,
        })
        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
        .stroke(match side {
            Side::Left | Side::Right => Stroke::new(1.0_f32, HAIRLINE),
            Side::Middle => Stroke::NONE,
        })
}

pub enum Side {
    Left,
    Right,
    Middle,
}
