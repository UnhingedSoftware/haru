use egui::{Color32, Rounding, Stroke, Vec2};

pub const ACCENT: Color32 = Color32::from_rgb(0x8B, 0x5C, 0xF6);

pub const BLOSSOM: Color32 = Color32::from_rgb(0xFF, 0x6F, 0xA5);

pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x27, 0x1F, 0x4A);

pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x6B, 0x6F);

pub const LOVE: Color32 = BLOSSOM;

pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xF0);

pub const MUTED: Color32 = Color32::from_rgb(0x8B, 0x8B, 0x9E);

pub const BACKDROP: Color32 = Color32::from_rgba_premultiplied(6, 6, 10, 220);

pub const MODAL: Color32 = Color32::from_rgb(0x14, 0x14, 0x1D);

pub const BASE: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x12);

const PANEL: Color32 = Color32::from_rgb(0x10, 0x10, 0x18);

pub const CARD: Color32 = Color32::from_rgb(0x16, 0x16, 0x1F);

const SURFACE: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x24);

pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x24, 0x24, 0x30);

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

    let rounding = Rounding::same(8.0);
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
    style.spacing.interact_size = Vec2::new(40.0, 26.0);
    style.spacing.text_edit_width = 200.0;
    style.spacing.button_padding = Vec2::new(13.0, 7.0);
    style.spacing.menu_margin = egui::Margin::same(6.0);
    style.spacing.window_margin = egui::Margin::same(10.0);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.combo_height = 360.0;

    ctx.set_style(style);
}

pub fn card(rounding: f32) -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .rounding(Rounding::same(rounding))
        .stroke(Stroke::new(1.0_f32, HAIRLINE))
}

pub fn gradient(ui: &egui::Ui, rect: egui::Rect, rounding: f32, from: Color32, to: Color32) {
    use egui::epaint::{Mesh, Vertex};

    let mut mesh = Mesh::default();
    for (at, colour) in [
        (rect.left_top(), from),
        (rect.right_top(), to),
        (rect.right_bottom(), to),
        (rect.left_bottom(), from),
    ] {
        mesh.vertices.push(Vertex {
            pos: at,
            uv: egui::epaint::WHITE_UV,
            color: colour,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);

    ui.painter()
        .rect_filled(rect, Rounding::same(rounding), from);
    ui.painter().add(egui::Shape::mesh(mesh));
    ui.painter()
        .rect_filled(rect, Rounding::same(rounding), Color32::TRANSPARENT);
}

pub fn glow(ui: &egui::Ui, rect: egui::Rect, rounding: f32, colour: Color32, strength: f32) {
    for step in 1..=3 {
        let spread = step as f32 * 2.0;
        let fade = (strength * (0.30 / step as f32)).clamp(0.0, 1.0);
        ui.painter().rect_stroke(
            rect.expand(spread),
            Rounding::same(rounding + spread),
            Stroke::new(1.5_f32, colour.gamma_multiply(fade)),
        );
    }
}

pub fn chip(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    let (fill, ink) = if on {
        (ACCENT_SOFT, ACCENT)
    } else {
        (SURFACE, MUTED)
    };
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.0), ink);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + Vec2::new(20.0, 9.0), egui::Sense::click());
    let hot = response.hovered() && !on;
    ui.painter().rect_filled(
        rect,
        Rounding::same(999.0),
        if hot { fill.gamma_multiply(1.6) } else { fill },
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, ink);
    response
}

pub fn field<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .hint_text(hint)
        .margin(egui::Margin::symmetric(10.0, 7.0))
        .desired_width(f32::INFINITY)
        .font(egui::FontId::proportional(13.0))
}

pub fn dismiss_chip(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let label = ui
        .painter()
        .layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.0), ACCENT);
    let mark = ui.painter().layout_no_wrap(
        crate::icons::Icon::Close.glyph().to_string(),
        egui::FontId::new(11.0, egui::FontFamily::Name(ICONS.into())),
        ACCENT,
    );
    let gap = 6.0;
    let size = Vec2::new(
        label.size().x + gap + mark.size().x + 20.0,
        label.size().y.max(mark.size().y) + 9.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if response.hovered() {
        ACCENT_SOFT.gamma_multiply(1.6)
    } else {
        ACCENT_SOFT
    };
    ui.painter().rect_filled(rect, Rounding::same(999.0), fill);
    let left = rect.left() + 10.0;
    let middle = rect.center().y;
    ui.painter()
        .galley(egui::pos2(left, middle - label.size().y / 2.0), label.clone(), ACCENT);
    ui.painter().galley(
        egui::pos2(left + label.size().x + gap, middle - mark.size().y / 2.0),
        mark,
        ACCENT,
    );
    response
}

pub fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(text).size(13.0).color(Color32::WHITE))
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(9.0))
        .min_size(Vec2::new(ui.available_width(), 34.0));
    ui.add(button)
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
