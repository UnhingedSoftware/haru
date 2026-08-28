use egui::{Align, Color32, Layout, RichText, Rounding, Sense, Stroke, Vec2};
use haru_core::{human_size, plain_text};
use haru_media::Previews;
use tapline::BrowseResult;

const CAPTION: f32 = 40.0;

pub fn columns_for(available: f32, min_tile: f32, spacing: f32) -> (usize, f32) {
    const MAX_TILE: f32 = 260.0;

    let columns = ((available + spacing) / (min_tile + spacing))
        .floor()
        .max(1.0);
    let width = ((available - spacing * (columns - 1.0)) / columns).min(MAX_TILE);
    (columns as usize, width.max(min_tile))
}

pub fn show(
    ui: &mut egui::Ui,
    previews: &mut Previews,
    found: &BrowseResult,
    size: f32,
    selected: bool,
) -> bool {
    let title = plain_text(&found.item.title);

    let response = ui
        .allocate_ui_with_layout(
            Vec2::new(size, size + CAPTION),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_width(size);
                ui.set_max_width(size);
                ui.spacing_mut().item_spacing.y = 2.0;

                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(size, size), Sense::click());

                let rounding = Rounding::same(6.0);
                ui.painter()
                    .rect_filled(rect, rounding, ui.visuals().extreme_bg_color);

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

                if selected {
                    ui.painter().rect_stroke(
                        rect,
                        rounding,
                        Stroke::new(2.0_f32, ui.visuals().selection.bg_fill),
                    );
                }

                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(&title).size(12.0)).truncate());
                ui.add(
                    egui::Label::new(
                        RichText::new(meta(found))
                            .size(11.0)
                            .color(Color32::from_gray(135)),
                    )
                    .truncate(),
                );

                response
            },
        )
        .inner;

    response.on_hover_text(title).clicked()
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

    #[test]
    fn a_row_of_tiles_uses_the_whole_width() {
        for available in [600.0_f32, 741.0, 1280.0, 1919.0] {
            let (columns, width) = columns_for(available, 168.0, 8.0);
            let used = width * columns as f32 + 8.0 * (columns as f32 - 1.0);
            assert!(columns >= 1);
            assert!(
                used >= available - 1.0 || width >= 259.0,
                "{available}: {columns} x {width} = {used}"
            );
        }
    }

    #[test]
    fn a_narrow_window_still_shows_one_tile() {
        let (columns, width) = columns_for(50.0, 168.0, 8.0);
        assert_eq!(columns, 1);
        assert!(width >= 168.0, "a tile never shrinks below its minimum");
    }
}
