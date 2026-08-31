use std::time::{Duration, Instant};

use egui::RichText;

use crate::theme;

const LIFE: Duration = Duration::from_secs(6);

const FADE: f32 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Said,
    Wrong,
}

struct Note {
    text: String,
    tone: Tone,
    born: Instant,
}

#[derive(Default)]
pub struct Toasts {
    notes: Vec<Note>,
}

impl Toasts {
    pub fn say(&mut self, text: impl Into<String>) {
        self.push(text.into(), Tone::Said);
    }

    pub fn wrong(&mut self, text: impl Into<String>) {
        self.push(text.into(), Tone::Wrong);
    }

    fn push(&mut self, text: String, tone: Tone) {
        if text.trim().is_empty() {
            return;
        }
        if self.notes.last().is_some_and(|last| last.text == text) {
            return;
        }
        self.notes.push(Note {
            text,
            tone,
            born: Instant::now(),
        });
        if self.notes.len() > 4 {
            self.notes.remove(0);
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.notes.retain(|note| note.born.elapsed() < LIFE);
        if self.notes.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(100));

        egui::Area::new("toasts".into())
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    for note in &self.notes {
                        let left = LIFE.saturating_sub(note.born.elapsed()).as_secs_f32();
                        let alpha = (left / FADE).clamp(0.0, 1.0)
                            * (note.born.elapsed().as_secs_f32() / 0.18).clamp(0.0, 1.0);
                        let edge = match note.tone {
                            Tone::Said => theme::ACCENT,
                            Tone::Wrong => theme::DANGER,
                        };

                        egui::Frame::none()
                            .fill(theme::MODAL.gamma_multiply(alpha))
                            .rounding(egui::Rounding::same(10.0))
                            .stroke(egui::Stroke::new(1.0_f32, edge.gamma_multiply(0.7 * alpha)))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.set_max_width(360.0);
                                ui.label(
                                    RichText::new(&note.text)
                                        .size(12.0)
                                        .color(theme::TEXT.gamma_multiply(alpha)),
                                );
                            });
                        ui.add_space(6.0);
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_said_about_nothing() {
        let mut toasts = Toasts::default();
        toasts.say("   ");
        assert!(toasts.notes.is_empty());
    }

    #[test]
    fn the_same_thing_twice_is_said_once() {
        let mut toasts = Toasts::default();
        toasts.say("applied to DP-1");
        toasts.say("applied to DP-1");
        assert_eq!(toasts.notes.len(), 1);
    }

    #[test]
    fn only_the_last_few_are_kept() {
        let mut toasts = Toasts::default();
        for at in 0..8 {
            toasts.say(format!("note {at}"));
        }
        assert_eq!(toasts.notes.len(), 4);
        assert_eq!(
            toasts.notes.last().map(|note| note.text.as_str()),
            Some("note 7")
        );
    }
}
