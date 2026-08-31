use egui::{Align, Color32, Layout, Rounding, Sense, Stroke, Vec2};
use haru_core::{human_size, plain_text};
use haru_media::Previews;

use crate::theme;
use tapline::BrowseResult;

const CAPTION: f32 = 42.0;

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

                let (slot, response) =
                    ui.allocate_exact_size(Vec2::new(size, height), Sense::click());

                let lift = ui.ctx().animate_bool_with_time(
                    response.id,
                    response.hovered() || selected,
                    0.12,
                );
                let rect = slot.expand(lift * 4.0);
                let rounding = Rounding::same(12.0);
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
                    None => shimmer(ui, rect, rounding),
                }

                caption(ui, rect, &title, found);

                if selected {
                    theme::glow(ui, rect, 12.0, theme::ACCENT, 1.0);
                } else if lift > 0.01 {
                    theme::glow(ui, rect, 12.0, theme::BLOSSOM, lift * 0.7);
                }
                let edge = if selected {
                    Stroke::new(2.0_f32, theme::ACCENT)
                } else {
                    Stroke::new(1.0_f32, theme::HAIRLINE.gamma_multiply(1.0 - lift))
                };
                ui.painter().rect_stroke(rect, rounding, edge);

                response
            },
        )
        .inner;

    response.on_hover_text(title).clicked()
}

fn caption(ui: &egui::Ui, rect: egui::Rect, title: &str, found: &BrowseResult) {
    let loved = (found.favorites > 0).then(|| format!("♥ {}", human_count(found.favorites)));
    caption_for(ui, rect, title, &meta(found), loved.as_deref(), theme::LOVE);
}

pub fn caption_for(
    ui: &egui::Ui,
    rect: egui::Rect,
    title: &str,
    under: &str,
    aside: Option<&str>,
    aside_colour: Color32,
) {
    let band = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - CAPTION - 14.0),
        rect.max,
    );
    fade(ui, band);

    let painter = ui.painter();
    let left = band.left() + 12.0;

    let mut kept = band.width() - 24.0;
    if let Some(aside) = aside {
        let side = one_line(ui, aside, 11.0, aside_colour, band.width());
        painter.galley(
            egui::pos2(band.right() - 12.0 - side.size().x, band.bottom() - 20.0),
            side.clone(),
            aside_colour,
        );
        kept -= side.size().x + 10.0;
    }

    let title = one_line(ui, title, 13.0, theme::TEXT, band.width() - 24.0);
    painter.galley(egui::pos2(left, band.bottom() - 38.0), title, theme::TEXT);

    let under = one_line(ui, under, 11.0, theme::MUTED, kept.max(24.0));
    painter.galley(egui::pos2(left, band.bottom() - 20.0), under, theme::MUTED);
}

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

pub fn shimmer(ui: &egui::Ui, rect: egui::Rect, rounding: Rounding) {
    use egui::epaint::{Mesh, Vertex};

    ui.painter().rect_filled(rect, rounding, theme::CARD);

    let time = ui.input(|input| input.time) as f32;
    let sweep = (time * 0.6).sin() * 0.5 + 0.5;
    let centre = rect.left() + rect.width() * sweep;
    let half = rect.width() * 0.22;

    let mut mesh = Mesh::default();
    let sheen = theme::ACCENT.gamma_multiply(0.10);
    let clear = Color32::TRANSPARENT;
    for (x, colour) in [
        (centre - half, clear),
        (centre, sheen),
        (centre + half, clear),
    ] {
        let x = x.clamp(rect.left(), rect.right());
        mesh.vertices.push(Vertex {
            pos: egui::pos2(x, rect.top()),
            uv: egui::epaint::WHITE_UV,
            color: colour,
        });
        mesh.vertices.push(Vertex {
            pos: egui::pos2(x, rect.bottom()),
            uv: egui::epaint::WHITE_UV,
            color: colour,
        });
    }
    mesh.indices
        .extend_from_slice(&[0, 1, 3, 0, 3, 2, 2, 3, 5, 2, 5, 4]);
    ui.painter().add(egui::Shape::mesh(mesh));

    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(60));
}

fn fade(ui: &egui::Ui, band: egui::Rect) {
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
