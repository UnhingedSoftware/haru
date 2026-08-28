//! One result in the grid.
//!
//! A square of preview art with the title and a line of numbers under it. The
//! square is fixed so rows line up whatever shape the art is; art that does not
//! match gets cropped rather than letterboxed, because a grid of
//! different-sized pictures reads as broken.

use egui::{Align, Color32, Layout, RichText, Rounding, Sense, Stroke, Vec2};
use haru_core::{human_size, plain_text};
use haru_media::Previews;
use tapline::BrowseResult;

/// How tall the caption under the art is.
const CAPTION: f32 = 40.0;

/// Draws one tile. Returns whether it was clicked.
pub fn show(
    ui: &mut egui::Ui,
    previews: &mut Previews,
    found: &BrowseResult,
    size: f32,
    selected: bool,
) -> bool {
    let title = plain_text(&found.item.title);

    // Top-down explicitly: the grid puts tiles in a horizontal row, and a tile
    // that inherits that lays its own caption out beside the picture instead
    // of under it.
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

                // The cache is asked every frame on purpose: the grid does not
                // track what it already has, and asking is what starts the
                // fetch.
                match found
                    .preview_url
                    .as_deref()
                    .and_then(|url| previews.texture(ui.ctx(), url))
                {
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

/// The numbers under a tile: what it is, how big, how well liked.
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
