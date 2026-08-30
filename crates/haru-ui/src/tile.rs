use egui::{Align, Color32, Layout, Rounding, Sense, Stroke, Vec2};
use haru_core::{human_size, plain_text};
use haru_media::Previews;

use crate::theme;
use tapline::BrowseResult;

const CAPTION: f32 = 42.0;

/// Picture height as a share of the tile's width — wallpapers are landscape.
pub const ASPECT: f32 = 0.60;

const FILL: f32 = 0.65;

pub fn columns_for(available: f32, min_tile: f32, spacing: f32) -> (usize, f32) {
    const MAX_TILE: f32 = 420.0;

    let columns = ((available + spacing) / (min_tile + spacing))
        .floor()
        .max(1.0);
    let width = ((available - spacing * (columns - 1.0)) / columns).min(MAX_TILE);
    (columns as usize, width.max(min_tile))
}

pub fn rows_for(available: f32, tile_width: f32, spacing: f32) -> usize {
    let row = tile_width * ASPECT + CAPTION + spacing;
    if row <= 0.0 {
        return 1;
    }
    let rows = ((available + spacing) / row + 1.0 - FILL).floor();
    if rows.is_finite() && rows >= 1.0 {
        rows as usize
    } else {
        1
    }
}

pub fn show(
    ui: &mut egui::Ui,
    previews: &mut Previews,
    found: &BrowseResult,
    size: f32,
    selected: bool,
) -> bool {
    let title = plain_text(&found.item.title);
    let height = size * ASPECT + CAPTION;

    let response = ui
        .allocate_ui_with_layout(
            Vec2::new(size, height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_width(size);
                ui.set_max_width(size);

                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(size, height), Sense::click());
                let rounding = Rounding::same(10.0);
                ui.painter().rect_filled(rect, rounding, theme::CARD);

                let picture = ui.is_rect_visible(rect).then(|| {
                    found
                        .preview_url
                        .as_deref()
                        .and_then(|url| previews.texture(ui.ctx(), url))
                });

                match picture.flatten() {
                    Some(texture) => {
                        egui::Image::new(&texture)
                            .rounding(rounding)
                            .maintain_aspect_ratio(true)
                            .fit_to_exact_size(rect.size())
                            .paint_at(ui, rect);
                    }
                    None => {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "…",
                            egui::FontId::proportional(18.0),
                            ui.visuals().weak_text_color(),
                        );
                    }
                }

                caption(ui, rect, rounding, &title, found);

                let edge = if selected {
                    Stroke::new(2.0_f32, theme::ACCENT)
                } else if response.hovered() {
                    Stroke::new(1.0_f32, theme::ACCENT.gamma_multiply(0.5))
                } else {
                    Stroke::new(1.0_f32, theme::HAIRLINE)
                };
                ui.painter().rect_stroke(rect, rounding, edge);

                response
            },
        )
        .inner;

    response.on_hover_text(title).clicked()
}

/// The name sits on the picture, over a fade, the way a poster does.
fn caption(ui: &egui::Ui, rect: egui::Rect, rounding: Rounding, title: &str, found: &BrowseResult) {
    let band = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - CAPTION - 14.0),
        rect.max,
    );
    fade(ui, band, rounding);

    let painter = ui.painter();
    let left = band.left() + 12.0;
    let loved = (found.favorites > 0).then(|| format!("♥ {}", human_count(found.favorites)));

    // Measure rather than count characters: a CJK title is half the glyphs and
    // twice the width, and the old guess ran it under the heart.
    let mut kept = band.width() - 24.0;
    if let Some(loved) = &loved {
        let hearts = one_line(ui, loved, 11.0, theme::LOVE, band.width());
        painter.galley(
            egui::pos2(band.right() - 12.0 - hearts.size().x, band.bottom() - 20.0),
            hearts.clone(),
            theme::LOVE,
        );
        kept -= hearts.size().x + 10.0;
    }

    let title = one_line(ui, title, 13.0, theme::TEXT, band.width() - 24.0);
    painter.galley(egui::pos2(left, band.bottom() - 38.0), title, theme::TEXT);

    let meta = one_line(ui, &meta(found), 11.0, theme::MUTED, kept.max(24.0));
    painter.galley(egui::pos2(left, band.bottom() - 20.0), meta, theme::MUTED);
}

/// One line, cut with an ellipsis at whatever width is left.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    size: f32,
    colour: Color32,
    room: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        egui::FontId::proportional(size),
        colour,
        room.max(16.0),
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.fonts(|fonts| fonts.layout_job(job))
}

fn fade(ui: &egui::Ui, band: egui::Rect, rounding: Rounding) {
    use egui::epaint::{Mesh, Vertex};

    let clear = Color32::from_black_alpha(0);
    let dark = Color32::from_black_alpha(216);
    let mut mesh = Mesh::default();
    for (at, colour) in [
        (band.left_top(), clear),
        (band.right_top(), clear),
        (band.right_bottom(), dark),
        (band.left_bottom(), dark),
    ] {
        mesh.vertices.push(Vertex {
            pos: at,
            uv: egui::epaint::WHITE_UV,
            color: colour,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);

    let _ = rounding;
    ui.painter().add(egui::Shape::mesh(mesh));
}

#[must_use]
pub fn human_count(count: u64) -> String {
    match count {
        0..=999 => count.to_string(),
        1_000..=999_999 => format!("{:.1}K", count as f64 / 1_000.0),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

fn meta(found: &BrowseResult) -> String {
    let kind = found
        .tags
        .iter()
        .find(|tag| matches!(tag.as_str(), "Scene" | "Video" | "Web" | "Application"))
        .cloned()
        .unwrap_or_else(|| "Item".to_owned());

    match found.score {
        Some(score) => format!(
            "{kind} · {} · {:.0}%",
            human_size(found.item.size),
            score * 100.0
        ),
        None => format!("{kind} · {}", human_size(found.item.size)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One row's height, from the same numbers the drawing uses.
    fn row_height(tile_width: f32, spacing: f32) -> f32 {
        tile_width * ASPECT + CAPTION + spacing
    }

    #[test]
    fn a_row_of_tiles_uses_the_whole_width() {
        for available in [600.0_f32, 741.0, 1280.0, 1919.0] {
            let (columns, width) = columns_for(available, 250.0, 8.0);
            let used = width * columns as f32 + 8.0 * (columns as f32 - 1.0);
            assert!(columns >= 1);
            assert!(
                used >= available - 1.0 || width >= 419.0,
                "{available}: {columns} x {width} = {used}"
            );
        }
    }

    #[test]
    fn rows_fill_the_height_they_are_given() {
        let row = row_height(200.0, 10.0);
        let three = row * 3.0 - 10.0;
        assert_eq!(rows_for(three, 200.0, 10.0), 3);
        assert_eq!(rows_for(three - 1.0, 200.0, 10.0), 3);
    }

    #[test]
    fn a_row_that_is_mostly_visible_is_asked_for() {
        let row = row_height(200.0, 10.0);
        let two = row * 2.0 - 10.0;
        assert_eq!(rows_for(two, 200.0, 10.0), 2);
        assert_eq!(rows_for(two + row * 0.8, 200.0, 10.0), 3);
        assert_eq!(rows_for(two + row * 0.2, 200.0, 10.0), 2);
    }

    #[test]
    fn a_short_window_still_asks_for_one_row() {
        assert_eq!(rows_for(0.0, 200.0, 10.0), 1);
        assert_eq!(rows_for(-50.0, 200.0, 10.0), 1);
    }

    #[test]
    fn a_narrow_window_still_shows_one_tile() {
        let (columns, width) = columns_for(50.0, 168.0, 8.0);
        assert_eq!(columns, 1);
        assert!(width >= 168.0, "a tile never shrinks below its minimum");
    }
}
