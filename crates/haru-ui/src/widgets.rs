//! Controls that more than one view draws.
//!
//! A wallpaper's properties appear twice — beside the screen they are on, and
//! in the preview — and they have to look and behave the same in both. One
//! function, two callers.

use egui::RichText;
use haru_core::properties::{Kind, Property};

/// Draws one property. Returns whether its value changed.
///
/// "Changed" means *settled*, not *touched*: a slider reports when the drag
/// ends, because every intermediate value would otherwise rebuild a scene.
pub fn property(ui: &mut egui::Ui, property: &mut Property) -> bool {
    match &mut property.kind {
        Kind::Bool(on) => ui
            .checkbox(on, RichText::new(&property.label).size(12.0))
            .changed(),
        Kind::Slider {
            value,
            min,
            max,
            step,
        } => {
            ui.label(RichText::new(&property.label).size(12.0));
            ui.add(
                egui::Slider::new(value, *min..=*max)
                    .step_by(*step)
                    .show_value(true),
            )
            .drag_stopped()
        }
        Kind::Color(rgb) => {
            ui.label(RichText::new(&property.label).size(12.0));
            ui.color_edit_button_rgb(rgb).changed()
        }
        Kind::Combo { value, options } => {
            ui.label(RichText::new(&property.label).size(12.0));
            let shown = options
                .iter()
                .find(|(_, option)| option == value)
                .map_or_else(|| value.clone(), |(label, _)| label.clone());
            let mut picked = false;
            egui::ComboBox::from_id_salt(&property.key)
                .selected_text(shown)
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for (label, option) in options.iter() {
                        picked |= ui.selectable_value(value, option.clone(), label).clicked();
                    }
                });
            picked
        }
        Kind::Text(text) => {
            ui.label(RichText::new(&property.label).size(12.0));
            ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY))
                .lost_focus()
        }
    }
}
