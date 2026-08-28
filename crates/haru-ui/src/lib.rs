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

mod account;
mod app;
mod icons;
mod library;
mod preview;
mod renderer;
mod settings;
pub mod theme;
mod tile;
mod widgets;

pub use account::Account;
pub use app::{Haru, Tab};
pub use library::Library;
pub use preview::Preview;
pub use settings::Settings;

use egui::{Align, Layout, RichText, Sense};
use haru_core::{Filters, TAG_GROUPS, TREND_PERIODS, human_size, plain_text};
use haru_media::Previews;
use haru_workshop::{Reply, Request, RequestId, Workshop};
use std::path::PathBuf;
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
    /// Shared with the library, which unsubscribes over the same connection.
    workshop: std::rc::Rc<Workshop>,
    filters: Filters,
    /// What the search box holds, which is not a search until Enter.
    typed: String,
    page: Option<BrowsePage>,
    /// Results kept from earlier pages, when scrolling endlessly.
    ///
    /// Empty in paged mode: there, a page replaces the one before it.
    appended: Vec<BrowseResult>,
    /// Whether results continue as the grid is scrolled.
    infinite: bool,
    /// The search whose answer is still wanted.
    ///
    /// Answers arrive in whatever order Steam manages, and a picker changes
    /// its mind constantly; without this a slow first search overwrites the
    /// fast second one.
    awaiting: Option<RequestId>,
    status: Status,
    selected: Option<usize>,
    /// Where a download lands, from the settings.
    install_root: Option<PathBuf>,
    /// Whether a running Steam client can be asked to subscribe.
    ///
    /// The better route when it exists: Steam holds the depot key, so no login
    /// here, and it downloads into the library this already reads.
    client: bool,
    /// The download in flight, if any: its item, and how far it has got.
    downloading: Option<(u64, u64, u64)>,
    /// The request the download's answers will carry.
    fetching: Option<RequestId>,
    /// Items installed during this session, so a tile stops offering to fetch
    /// what is already on disk.
    installed: Vec<u64>,
    /// A wallpaper that just landed, for the window to put up.
    landed: Option<PathBuf>,
}

impl Browser {
    /// Opens the picker on the default view.
    #[must_use]
    pub fn new() -> Self {
        Self::with_filters(Filters::new(), std::rc::Rc::new(Workshop::spawn()))
    }

    /// Reruns the search with settings the window has changed.
    pub fn reconfigure(&mut self, adult: bool, per_page: u32, infinite: bool) {
        let same = self.filters.adult == adult
            && self.filters.per_page == per_page
            && self.infinite == infinite;
        if same {
            return;
        }
        self.filters.adult = adult;
        self.filters.per_page = per_page;
        self.infinite = infinite;
        self.search();
    }

    /// Opens the picker on a search someone already knows they want.
    ///
    /// What a command line, a URL handler or the studio hands over: the window
    /// should come up showing the answer rather than the front page.
    #[must_use]
    pub fn with_filters(filters: Filters, workshop: std::rc::Rc<Workshop>) -> Self {
        let awaiting = Some(workshop.send(Request::Browse(filters.to_query())));

        Self {
            workshop,
            typed: filters.text.clone(),
            filters,
            page: None,
            appended: Vec::new(),
            infinite: false,
            awaiting,
            status: Status::Searching,
            selected: None,
            install_root: None,
            client: false,
            downloading: None,
            fetching: None,
            installed: Vec::new(),
            landed: None,
        }
    }

    /// Takes the wallpaper that just finished downloading, if one did.
    ///
    /// Handed up rather than applied here: the browser does not own a screen
    /// or a renderer, and downloading is not the same decision as displaying.
    pub fn take_landed(&mut self) -> Option<PathBuf> {
        self.landed.take()
    }

    /// Tells the browser where downloads should go.
    pub fn set_install_root(&mut self, root: Option<PathBuf>) {
        self.install_root = root;
    }

    /// Tells the browser whether a Steam client can do the fetching.
    pub fn set_client(&mut self, client: bool) {
        self.client = client;
    }

    /// Draws a frame.
    pub fn ui(&mut self, ctx: &egui::Context, previews: &mut Previews, sidebar: bool) {
        self.collect();

        // Same reason as the account state: a reply is not an event, so a
        // window with nothing else happening would sleep on it.
        if self.awaiting.is_some() || self.fetching.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }

        if sidebar {
            egui::SidePanel::left("filters")
                .resizable(false)
                .exact_width(238.0)
                .frame(theme::panel_frame(theme::Side::Left))
                .show(ctx, |ui| self.sidebar(ui));
        }

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

    /// Takes the answers that belong to this view.
    ///
    /// Claimed by the request that asked, because the connection is shared and
    /// an answer's shape does not say whose it is.
    fn collect(&mut self) {
        if let Some(id) = self.awaiting
            && let Some(reply) = self.workshop.take(id)
        {
            self.awaiting = None;
            match reply {
                Reply::Page(page) => {
                    self.selected = None;
                    // Endless scrolling keeps what came before; paging
                    // replaces it, which is the whole difference.
                    if self.infinite && self.filters.page > 1 {
                        if let Some(previous) = self.page.take() {
                            self.appended.extend(previous.items);
                        }
                    } else {
                        self.appended.clear();
                    }
                    self.page = Some(*page);
                    self.status = Status::Idle;
                }
                Reply::Count(_) => self.status = Status::Idle,
                Reply::Failed(why) => self.status = Status::Failed(why),
                _ => {}
            }
        }

        // A download answers repeatedly — progress, then the outcome — so it
        // keeps its request until one of the endings arrives.
        while let Some(id) = self.fetching
            && let Some(reply) = self.workshop.take(id)
        {
            match reply {
                Reply::Progress {
                    id: item,
                    done,
                    total,
                } => {
                    self.downloading = Some((item, done, total));
                }
                Reply::Installed { id: item, dir } => {
                    self.fetching = None;
                    self.downloading = None;
                    self.installed.push(item);
                    self.status = Status::Idle;
                    // Straight onto a screen: asking for it was the decision,
                    // and a second trip to see it is a step nobody wants.
                    self.landed = Some(dir);
                }
                Reply::Subscribed => {
                    // Steam fetches it from here, into its own library — which
                    // is the one the Library tab rescans.
                    self.fetching = None;
                    self.downloading = None;
                    self.status = Status::Idle;
                }
                Reply::Failed(why) => {
                    self.fetching = None;
                    self.downloading = None;
                    self.status = Status::Failed(why);
                }
                _ => {}
            }
        }
    }

    /// Runs the current filters, from the first page.
    fn search(&mut self) {
        // The old search's answer is nobody's now.
        if let Some(id) = self.awaiting.take() {
            self.workshop.discard(id);
        }
        self.filters.page = 1;
        self.appended.clear();
        self.page = None;
        self.run();
    }

    /// Goes to one numbered page.
    fn go_to(&mut self, page: u32) {
        if self.awaiting.is_some() || page == self.filters.page {
            return;
        }
        self.filters.page = page.max(1);
        self.run();
    }

    /// Runs the current filters as they stand, cursor included.
    fn run(&mut self) {
        self.status = Status::Searching;
        self.awaiting = Some(self.workshop.send(Request::Browse(self.filters.to_query())));
    }

    /// The filter sidebar.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("haru");
        ui.label(RichText::new("Wallpaper Engine Workshop").small().weak());
        ui.add_space(10.0);

        self.search_box(ui);

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

        changed |= self.tag_groups(ui);

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

    /// Getting this wallpaper: the button, the bar, or the news that it is
    /// already here.
    fn download_row(&mut self, ui: &mut egui::Ui, found: &BrowseResult) {
        let id = found.item.id.get();
        let on_disk = self.installed.contains(&id)
            || self.install_root.as_ref().is_some_and(|root| {
                root.join(format!("steamapps/workshop/content/431960/{id}"))
                    .join("project.json")
                    .is_file()
            });

        match self.downloading {
            // This one, in flight.
            Some((downloading, done, total)) if downloading == id => {
                let share = if total == 0 {
                    0.0
                } else {
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a progress bar, not an accounting figure"
                    )]
                    let share = done as f32 / total as f32;
                    share
                };
                ui.add(egui::ProgressBar::new(share).text(format!(
                    "{} of {}",
                    human_size(done),
                    human_size(total)
                )));
            }
            // Something else is downloading; one at a time, because the
            // session behind it is one connection.
            Some(_) => {
                ui.add_enabled(
                    false,
                    egui::Button::new("Download").min_size(egui::vec2(ui.available_width(), 30.0)),
                );
            }
            None if on_disk => {
                ui.label(RichText::new("Already installed").color(theme::ACCENT));
            }
            None => {
                if ui
                    .add_sized([ui.available_width(), 30.0], egui::Button::new("Download"))
                    .clicked()
                {
                    // A running client is the better route: it holds
                    // the depot key, so nothing here needs a login, and
                    // it downloads into the library this already reads.
                    if self.client {
                        self.downloading = Some((id, 0, found.item.size));
                        self.fetching = Some(self.workshop.send(Request::SubscribeViaClient {
                            item: found.item.id,
                        }));
                    } else {
                        match self.install_root.clone() {
                            Some(root) => {
                                self.downloading = Some((id, 0, found.item.size));
                                self.fetching = Some(self.workshop.send(Request::Install {
                                    item: Box::new(found.item.clone()),
                                    into: root,
                                }));
                            }
                            None => {
                                self.status = Status::Failed(
                                    "no Steam library to install into — set one in Settings"
                                        .to_owned(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// The search box, and where a search looks.
    ///
    /// Searching happens on Enter rather than per keystroke: every search is a
    /// round trip to Steam, and one per letter is a query storm that answers
    /// out of order.
    fn search_box(&mut self, ui: &mut egui::Ui) {
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
    }

    /// The six filter axes. Returns whether one changed.
    fn tag_groups(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height((ui.available_height() - 64.0).max(120.0))
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
                                .selectable_label(
                                    selected.is_none(),
                                    format!("Any {}", group.label),
                                )
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
        changed
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

        // In endless mode this is every result loaded so far; in paged mode
        // the kept list is empty and this is just the page.
        let items: Vec<BrowseResult> = self
            .appended
            .iter()
            .chain(page.items.iter())
            .cloned()
            .collect();

        if items.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Nothing matched those filters.").weak());
            });
            return;
        }

        // Whatever fits, at a size that uses the whole width — a fixed tile
        // leaves the remainder as a gap down the side of the grid.
        let (columns, tile_width) = tile::columns_for(ui.available_width(), TILE, 8.0);
        let mut hit_bottom = false;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (row, chunk) in items.chunks(columns).enumerate() {
                    ui.horizontal(|ui| {
                        for (column, found) in chunk.iter().enumerate() {
                            let index = row * columns + column;
                            let clicked = tile::show(
                                ui,
                                previews,
                                found,
                                tile_width,
                                self.selected == Some(index),
                            );
                            if clicked {
                                self.selected = Some(index);
                            }
                        }
                    });
                    ui.add_space(10.0);
                }

                if self.infinite {
                    // A marker at the end of the list: once it is on screen,
                    // there is nothing below and the next page is wanted.
                    let (rect, _) = ui
                        .allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
                    hit_bottom = ui.is_rect_visible(rect);
                    if self.awaiting.is_some() {
                        ui.vertical_centered(|ui| ui.spinner());
                        ui.add_space(8.0);
                    }
                }
            });

        if hit_bottom
            && self.awaiting.is_none()
            && self.page.as_ref().is_some_and(BrowsePage::has_more)
        {
            self.filters.page = self.filters.page.saturating_add(1);
            self.run();
        }
    }

    /// The detail pane for one result.
    fn detail(&mut self, ui: &mut egui::Ui, previews: &mut Previews, index: usize) {
        let Some(found) = self
            .appended
            .iter()
            .chain(self.page.iter().flat_map(|page| page.items.iter()))
            .nth(index)
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
                    // Truncated: a Workshop title runs as long as its author
                    // liked, and the pane is 300 pixels wide.
                    ui.add(
                        egui::Label::new(
                            RichText::new(plain_text(&found.item.title))
                                .size(16.0)
                                .strong(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(plain_text(&found.item.title));
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        if crate::icons::button(ui, crate::icons::Icon::Close, false)
                            .on_hover_text("Close")
                            .clicked()
                        {
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

                ui.add_space(10.0);

                self.download_row(ui, &found);

                ui.add_space(10.0);
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
            let total = self.page.as_ref().map_or(0, |page| page.total);

            match &self.status {
                Status::Searching => {
                    ui.spinner();
                    ui.label("searching…");
                }
                Status::Failed(why) => {
                    ui.label(RichText::new(why).color(ui.visuals().error_fg_color));
                }
                Status::Idle => {
                    ui.label(format!("{} matches", thousands(u64::from(total))));
                    if previews.loading() > 0 {
                        ui.weak(format!("· {} previews loading", previews.loading()));
                    }
                }
            }

            // Endless scrolling has no pages to number, and a strip that said
            // "page 3 of 400" beside a grid that never ends would be a lie.
            if self.infinite {
                let shown =
                    self.appended.len() + self.page.as_ref().map_or(0, |page| page.items.len());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.weak(format!("{} loaded", thousands(shown as u64)));
                });
                return;
            }

            let pages = self.filters.pages(total);
            let current = self.filters.page;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let waiting = self.awaiting.is_some();
                let mut jump = None;

                if ui
                    .add_enabled(
                        current < pages && !waiting,
                        egui::Button::new(crate::icons::text(crate::icons::Icon::Next)),
                    )
                    .clicked()
                {
                    jump = Some(current.saturating_add(1));
                }

                // Right to left, so the numbers are built backwards and read
                // forwards.
                for number in strip(current, pages).into_iter().rev() {
                    match number {
                        Some(number) if number == current => {
                            let _ = ui.selectable_label(true, RichText::new(number.to_string()));
                        }
                        Some(number) => {
                            if ui
                                .add_enabled(!waiting, egui::Button::new(number.to_string()))
                                .clicked()
                            {
                                jump = Some(number);
                            }
                        }
                        None => {
                            ui.weak("…");
                        }
                    }
                }

                if ui
                    .add_enabled(
                        current > 1 && !waiting,
                        egui::Button::new(crate::icons::text(crate::icons::Icon::Previous)),
                    )
                    .clicked()
                {
                    jump = Some(current.saturating_sub(1));
                }

                if let Some(page) = jump {
                    self.go_to(page);
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

/// Which page numbers to show around the current one.
///
/// `None` is a gap. Wallpaper Engine's Workshop is 130,000 pages of twenty-four,
/// so every number is not an option and the ends have to be reachable anyway:
/// the first, the last, and a window around where you are.
fn strip(current: u32, pages: u32) -> Vec<Option<u32>> {
    /// How many either side of the current page.
    const AROUND: u32 = 2;

    if pages <= 1 {
        return vec![Some(1)];
    }

    let first_shown = current.saturating_sub(AROUND).max(1);
    let last_shown = current.saturating_add(AROUND).min(pages);

    let mut out = Vec::new();
    if first_shown > 1 {
        out.push(Some(1));
        // A gap of exactly one page is worth spelling out rather than hiding.
        if first_shown > 3 {
            out.push(None);
        } else if first_shown == 3 {
            out.push(Some(2));
        }
    }
    for number in first_shown..=last_shown {
        out.push(Some(number));
    }
    if last_shown < pages {
        if last_shown + 2 < pages {
            out.push(None);
        } else if last_shown + 2 == pages {
            out.push(Some(pages - 1));
        }
        out.push(Some(pages));
    }
    out
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
    fn the_page_strip_always_reaches_both_ends() {
        // Whatever the window shows, page one and the last page must be one
        // click away, or a deep search is a trap.
        for (current, pages) in [
            (1_u32, 1_u32),
            (1, 9),
            (5, 9),
            (60, 132_618),
            (132_618, 132_618),
        ] {
            let shown: Vec<u32> = strip(current, pages).into_iter().flatten().collect();
            assert!(shown.contains(&1), "no first page at {current}/{pages}");
            assert!(shown.contains(&pages), "no last page at {current}/{pages}");
            assert!(
                shown.contains(&current),
                "no current page at {current}/{pages}"
            );
        }
    }

    #[test]
    fn the_strip_stays_short_however_deep_the_results_go() {
        // 132,618 pages is a real number for this Workshop; the strip has to
        // be a fixed width regardless.
        assert!(strip(60_000, 132_618).len() <= 9);
    }

    #[test]
    fn a_gap_of_one_page_is_shown_rather_than_hidden() {
        // "1 … 3 4 5" hides exactly one number behind an ellipsis that costs
        // more space than the number would.
        let shown = strip(4, 20);
        assert_eq!(shown.first(), Some(&Some(1)));
        assert_eq!(shown.get(1), Some(&Some(2)), "{shown:?}");
    }

    #[test]
    fn a_single_page_is_just_itself() {
        assert_eq!(strip(1, 1), vec![Some(1)]);
        assert_eq!(strip(1, 0), vec![Some(1)]);
    }

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
