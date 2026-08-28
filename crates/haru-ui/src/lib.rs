//! The picker window.
//!
//! A filter sidebar on the left, a grid of results in the middle, and a detail
//! pane for whatever is selected. Steam's own layout, because that is the one
//! people already know how to use.
//!
//! Two habits from the shell this replaces, both learned the hard way:
//! searching happens on Enter rather than per keystroke — every keystroke is a
//! round trip to a CM — and a page of results asks the cache for its pictures
//! every frame, because the grid does not know what it already has.

mod app;
mod library;
mod settings;
mod tile;
pub mod theme;

pub use app::{Haru, Tab};
pub use library::Library;
pub use settings::Settings;

use egui::{Align, Layout, RichText};
use haru_core::{Filters, TAG_GROUPS, TREND_PERIODS, human_size, plain_text};
use haru_media::Previews;
use haru_workshop::{Reply, Request, RequestId, Workshop};
use tapline::{BrowsePage, BrowseResult, BrowseSort, TextTarget};

/// How wide a tile is, before spacing.
const TILE: f32 = 168.0;

/// What the window is doing.
enum Status {
    /// Nothing in flight.
    Idle,
    /// A search is out.
    Searching,
    /// Steam, or the connection, said no.
    Failed(String),
}

/// The picker.
pub struct Browser {
    workshop: Workshop,
    filters: Filters,
    /// What the search box holds, which is not a search until Enter.
    typed: String,
    page: Option<BrowsePage>,
    /// The cursor of each page walked through, so Back works.
    ///
    /// Steam hands out a cursor for the *next* page and nothing for the
    /// previous one, so going back means remembering where each page started.
    history: Vec<Option<String>>,
    /// The search whose answer is still wanted.
    ///
    /// Answers arrive in whatever order Steam manages, and a picker changes
    /// its mind constantly; without this a slow first search overwrites the
    /// fast second one.
    awaiting: Option<RequestId>,
    status: Status,
    selected: Option<usize>,
}

impl Browser {
    /// Opens the picker on the default view.
    #[must_use]
    pub fn new() -> Self {
        Self::with_filters(Filters::new())
    }

    /// Reruns the search with settings the window has changed.
    pub fn reconfigure(&mut self, adult: bool, per_page: u32) {
        if self.filters.adult == adult && self.filters.per_page == per_page {
            return;
        }
        self.filters.adult = adult;
        self.filters.per_page = per_page;
        self.search();
    }

    /// Opens the picker on a search someone already knows they want.
    ///
    /// What a command line, a URL handler or the studio hands over: the window
    /// should come up showing the answer rather than the front page.
    #[must_use]
    pub fn with_filters(filters: Filters) -> Self {
        let workshop = Workshop::spawn();
        let awaiting = Some(workshop.send(Request::Browse(filters.to_query())));

        Self {
            workshop,
            typed: filters.text.clone(),
            filters,
            page: None,
            history: vec![None],
            awaiting,
            status: Status::Searching,
            selected: None,
        }
    }

    /// Draws a frame.
    pub fn ui(&mut self, ctx: &egui::Context, previews: &mut Previews) {
        self.collect();

        egui::SidePanel::left("filters")
            .resizable(false)
            .exact_width(238.0)
            .frame(theme::panel_frame(theme::Side::Left))
            .show(ctx, |ui| self.sidebar(ui));

        if let Some(index) = self.selected {
            egui::SidePanel::right("detail")
                .resizable(false)
                .exact_width(312.0)
                .frame(theme::panel_frame(theme::Side::Right))
                .show(ctx, |ui| self.detail(ui, previews, index));
        }

        egui::TopBottomPanel::bottom("paging")
            .frame(theme::panel_frame(theme::Side::Left))
            .show(ctx, |ui| self.paging(ui, previews));
        egui::CentralPanel::default()
            .frame(theme::panel_frame(theme::Side::Middle))
            .show(ctx, |ui| self.grid(ui, previews));
    }

    /// Takes any answer that has arrived.
    fn collect(&mut self) {
        while let Some((id, reply)) = self.workshop.poll() {
            // An answer to a search that has already been replaced.
            if Some(id) != self.awaiting {
                continue;
            }
            self.awaiting = None;
            match reply {
                Reply::Page(page) => {
                    self.selected = None;
                    self.page = Some(*page);
                    self.status = Status::Idle;
                }
                Reply::Count(_) => self.status = Status::Idle,
                Reply::Failed(why) => self.status = Status::Failed(why),
            }
        }
    }

    /// Runs the current filters, from the first page.
    fn search(&mut self) {
        self.filters.cursor = None;
        self.history = vec![None];
        self.run();
    }

    /// Runs the current filters as they stand, cursor included.
    fn run(&mut self) {
        self.status = Status::Searching;
        self.awaiting = Some(
            self.workshop
                .send(Request::Browse(self.filters.to_query())),
        );
    }

    /// The filter sidebar.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("haru");
        ui.label(
            RichText::new("Wallpaper Engine Workshop")
                .small()
                .weak(),
        );
        ui.add_space(10.0);

        let search = ui.add(
            egui::TextEdit::singleline(&mut self.typed)
                .hint_text("Search, then Enter")
                .desired_width(f32::INFINITY),
        );
        if search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.filters.text = self.typed.clone();
            self.search();
        }

        if !self.typed.trim().is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (label, target) in [
                    ("Anywhere", TextTarget::Everything),
                    ("Title", TextTarget::Title),
                    ("Body", TextTarget::Description),
                ] {
                    if ui
                        .selectable_label(self.filters.search_in == target, label)
                        .clicked()
                    {
                        self.filters.search_in = target;
                        self.filters.text = self.typed.clone();
                        self.search();
                    }
                }
            });
        }

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);

        let mut changed = false;

        ui.label(RichText::new("Sort").small().weak());
        egui::ComboBox::from_id_salt("sort")
            .selected_text(sort_label(self.filters.sort))
            .width(200.0)
            .show_ui(ui, |ui| {
                for sort in [
                    BrowseSort::Vote,
                    BrowseSort::Subscribed,
                    BrowseSort::Trend,
                    BrowseSort::Recent,
                    BrowseSort::Updated,
                ] {
                    changed |= ui
                        .selectable_value(&mut self.filters.sort, sort, sort_label(sort))
                        .changed();
                }
            });

        // Only Steam's trend ranking honours a period; on any other sort the
        // number is refused rather than quietly ignored, so the control only
        // exists where it means something.
        if self.filters.sort == BrowseSort::Trend {
            ui.add_space(6.0);
            ui.label(RichText::new("Period").small().weak());
            egui::ComboBox::from_id_salt("period")
                .selected_text(period_label(self.filters.trend_days))
                .width(200.0)
                .show_ui(ui, |ui| {
                    for (label, days) in TREND_PERIODS {
                        changed |= ui
                            .selectable_value(&mut self.filters.trend_days, Some(*days), *label)
                            .changed();
                    }
                });
        }

        ui.add_space(12.0);
        ui.label(RichText::new("Filters").small().weak());
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height() - 76.0)
            .show(ui, |ui| {
                for (index, group) in TAG_GROUPS.iter().enumerate() {
                    let Some(chosen) = self.filters.chosen.get(index) else {
                        continue;
                    };
                    let selected = chosen.clone();
                    egui::ComboBox::from_id_salt(group.label)
                        .selected_text(selected.clone().unwrap_or_else(|| group.label.to_owned()))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(selected.is_none(), format!("Any {}", group.label))
                                .clicked()
                                && let Some(slot) = self.filters.chosen.get_mut(index)
                            {
                                *slot = None;
                                changed = true;
                            }
                            for tag in group.tags {
                                if ui
                                    .selectable_label(selected.as_deref() == Some(*tag), *tag)
                                    .clicked()
                                    && let Some(slot) = self.filters.chosen.get_mut(index)
                                {
                                    *slot = Some((*tag).to_owned());
                                    changed = true;
                                }
                            }
                        });
                    ui.add_space(4.0);
                }
            });

        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut self.filters.adult, "18+").changed();
            if ui.button("Clear").clicked() {
                self.filters.clear();
                self.typed.clear();
                changed = true;
            }
        });

        if changed {
            self.search();
        }
    }

    /// The result grid.
    fn grid(&mut self, ui: &mut egui::Ui, previews: &mut Previews) {
        let Some(page) = self.page.as_ref() else {
            ui.centered_and_justified(|ui| match &self.status {
                Status::Failed(why) => {
                    ui.label(RichText::new(why).color(ui.visuals().error_fg_color));
                }
                _ => {
                    ui.spinner();
                }
            });
            return;
        };

        if page.items.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Nothing matched those filters.").weak());
            });
            return;
        }

        // Whatever fits, so a wide window shows more rather than more padding.
        let columns = ((ui.available_width() / (TILE + 12.0)).floor() as usize).max(1);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let items: Vec<(usize, BrowseResult)> = page
                    .items
                    .iter()
                    .cloned()
                    .enumerate()
                    .collect();
                for row in items.chunks(columns) {
                    ui.horizontal(|ui| {
                        for (index, found) in row {
                            let clicked =
                                tile::show(ui, previews, found, TILE, self.selected == Some(*index));
                            if clicked {
                                self.selected = Some(*index);
                            }
                        }
                    });
                    ui.add_space(10.0);
                }
            });
    }

    /// The detail pane for one result.
    fn detail(&mut self, ui: &mut egui::Ui, previews: &mut Previews, index: usize) {
        let Some(found) = self
            .page
            .as_ref()
            .and_then(|page| page.items.get(index))
            .cloned()
        else {
            self.selected = None;
            return;
        };

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.heading(plain_text(&found.item.title));
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        if ui.small_button("✕").clicked() {
                            self.selected = None;
                        }
                    });
                });

                if let Some(url) = found.preview_url.as_deref()
                    && let Some(texture) = previews.texture(ui.ctx(), url)
                {
                    ui.add_space(6.0);
                    ui.add(
                        egui::Image::new(&texture)
                            .max_width(ui.available_width())
                            .rounding(6.0),
                    );
                }

                ui.add_space(8.0);
                ui.label(RichText::new(human_size(found.item.size)).strong());
                ui.label(format!("{} subscribers", thousands(found.subscriptions)));
                ui.label(format!("{} views", thousands(found.views)));
                if let Some(score) = found.score {
                    ui.label(format!(
                        "{:.0}% of {} votes",
                        score * 100.0,
                        thousands(found.votes_up.saturating_add(found.votes_down))
                    ));
                }

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    for tag in &found.tags {
                        ui.label(RichText::new(tag).small().weak());
                        ui.add_space(2.0);
                    }
                });

                let description = plain_text(&found.description);
                if !description.is_empty() {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(description);
                }
            });
    }

    /// The status and paging bar.
    fn paging(&mut self, ui: &mut egui::Ui, previews: &mut Previews) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (total, more) = self
                .page
                .as_ref()
                .map_or((0, false), |page| (page.total, page.has_more()));

            match &self.status {
                Status::Searching => {
                    ui.spinner();
                    ui.label("searching…");
                }
                Status::Failed(why) => {
                    ui.label(RichText::new(why).color(ui.visuals().error_fg_color));
                }
                Status::Idle => {
                    let page = self.history.len();
                    ui.label(format!("{} matches · page {page}", thousands(u64::from(total))));
                    if previews.loading() > 0 {
                        ui.weak(format!("· {} previews loading", previews.loading()));
                    }
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let waiting = self.awaiting.is_some();
                if ui
                    .add_enabled(more && !waiting, egui::Button::new("Next ›"))
                    .clicked()
                    && let Some(next) = self
                        .page
                        .as_ref()
                        .and_then(|page| page.next_cursor.clone())
                {
                    self.history.push(Some(next.clone()));
                    self.filters.cursor = Some(next);
                    self.run();
                }
                if ui
                    .add_enabled(self.history.len() > 1 && !waiting, egui::Button::new("‹ Back"))
                    .clicked()
                {
                    self.history.pop();
                    self.filters.cursor = self.history.last().cloned().flatten();
                    self.run();
                }
            });
        });
        ui.add_space(4.0);
    }
}

impl Default for Browser {
    fn default() -> Self {
        Self::new()
    }
}

/// What a sort is called in the window.
const fn sort_label(sort: BrowseSort) -> &'static str {
    match sort {
        BrowseSort::Vote => "Top rated",
        BrowseSort::Recent => "Newest",
        BrowseSort::Updated => "Recently updated",
        BrowseSort::Trend => "Trending",
        BrowseSort::Subscribed => "Most subscribed",
        BrowseSort::TextMatch => "Best match",
    }
}

/// What a trend window is called.
fn period_label(days: Option<u32>) -> String {
    days.map_or_else(
        || "Today".to_owned(),
        |days| {
            TREND_PERIODS
                .iter()
                .find(|(_, value)| *value == days)
                .map_or_else(|| format!("{days} days"), |(label, _)| (*label).to_owned())
        },
    )
}

/// A count with separators, because six digits are unreadable without them.
fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_the_way_they_are_read() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(3_182_822), "3,182,822");
    }

    #[test]
    fn every_sort_has_a_name_a_person_would_recognise() {
        for sort in [
            BrowseSort::Vote,
            BrowseSort::Recent,
            BrowseSort::Updated,
            BrowseSort::Trend,
            BrowseSort::Subscribed,
            BrowseSort::TextMatch,
        ] {
            assert!(!sort_label(sort).is_empty());
        }
    }

    #[test]
    fn a_period_falls_back_to_its_own_number() {
        assert_eq!(period_label(Some(180)), "Six months");
        assert_eq!(period_label(Some(42)), "42 days");
    }
}
