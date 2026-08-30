mod account;
mod app;
mod icons;
mod library;
mod preview;
mod props;
mod renderer;
mod settings;
pub mod theme;
mod tile;
mod updates;
mod widgets;

pub use account::Account;
pub use app::{Haru, Tab};
pub use library::Library;
pub use preview::Preview;
pub use settings::Settings;

use egui::{Align, Layout, RichText, Sense};
use haru_apply::Engine;
use haru_core::{Filters, TAG_GROUPS, TREND_PERIODS, human_size, plain_text};
use haru_media::Previews;
use haru_workshop::{Reply, Request, RequestId, Workshop};
use std::path::{Path, PathBuf};
use tapline::{BrowsePage, BrowseResult, BrowseSort, TextTarget};

const TILE: f32 = 250.0;

fn summary(label: &str, picked: &[String]) -> String {
    match picked.len() {
        0 => format!("Any {label}"),
        1 => picked.first().cloned().unwrap_or_default(),
        many => format!("{label}: {many} chosen"),
    }
}

const SETTLE: f64 = 0.35;

const LANDING: f64 = 300.0;

const DETAIL: f32 = 312.0;

enum Status {
    Idle,
    Searching,
    Failed(String),
}

pub struct Browser {
    workshop: std::rc::Rc<Workshop>,
    filters: Filters,
    typed: String,
    page: Option<BrowsePage>,
    appended: Vec<BrowseResult>,
    infinite: bool,
    awaiting: Option<RequestId>,
    status: Status,
    selected: Option<usize>,
    selected_id: Option<u64>,
    install_root: Option<PathBuf>,
    client: bool,
    downloading: Option<(u64, u64, u64)>,
    fetching: Option<RequestId>,
    installed: Vec<u64>,
    landed: Option<PathBuf>,
    fit: bool,
    settling: Option<(u32, f64)>,
    settings: crate::props::Panel,
    subscribing: Option<u64>,
    landing: Option<(u64, PathBuf, f64)>,
    needs_account: bool,
}

impl Browser {
    #[must_use]
    pub fn new() -> Self {
        Self::with_filters(Filters::new(), std::rc::Rc::new(Workshop::spawn()))
    }

    pub fn reconfigure(&mut self, adult: bool, per_page: u32, infinite: bool, fit: bool) {
        let same = self.filters.adult == adult
            && self.infinite == infinite
            && self.fit == fit
            && (fit || self.filters.per_page == per_page);
        if same {
            return;
        }
        self.filters.adult = adult;
        self.infinite = infinite;
        self.fit = fit;
        if !fit {
            self.filters.per_page = per_page;
        }
        self.settling = None;
        self.search();
    }

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
            selected_id: None,
            install_root: None,
            client: false,
            downloading: None,
            fetching: None,
            installed: Vec::new(),
            landed: None,
            fit: true,
            settling: None,
            settings: crate::props::Panel::default(),
            subscribing: None,
            landing: None,
            needs_account: false,
        }
    }

    pub fn take_needs_account(&mut self) -> bool {
        std::mem::take(&mut self.needs_account)
    }

    pub fn take_landed(&mut self) -> Option<PathBuf> {
        self.landed.take()
    }

    pub fn set_install_root(&mut self, root: Option<PathBuf>) {
        self.install_root = root;
    }

    pub fn set_client(&mut self, client: bool) {
        self.client = client;
    }

    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        previews: &mut Previews,
        sidebar: bool,
        engine: &haru_apply::Engine,
    ) {
        self.collect();
        self.check_files(ctx);

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
                .exact_width(DETAIL)
                .frame(theme::panel_frame(theme::Side::Right))
                .show(ctx, |ui| self.detail(ui, previews, index, engine));
        }

        egui::TopBottomPanel::bottom("paging")
            .frame(theme::panel_frame(theme::Side::Left))
            .show(ctx, |ui| self.paging(ui, previews));
        egui::CentralPanel::default()
            .frame(theme::panel_frame(theme::Side::Middle))
            .show(ctx, |ui| {
                self.toolbar(ui);
                ui.add_space(10.0);
                self.grid(ui, previews);
            });
    }

    fn collect(&mut self) {
        if let Some(id) = self.awaiting
            && let Some(reply) = self.workshop.take(id)
        {
            self.awaiting = None;
            match reply {
                Reply::Page(page) => {
                    if self.infinite && self.filters.page > 1 {
                        if let Some(previous) = self.page.take() {
                            self.appended.extend(previous.items);
                        }
                    } else {
                        self.appended.clear();
                    }
                    self.selected = None;
                    self.reselect(&page);
                    self.page = Some(*page);
                    self.status = Status::Idle;
                }
                Reply::Count(_) => self.status = Status::Idle,
                Reply::Failed(why) => self.status = Status::Failed(why),
                _ => {}
            }
        }

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
                    self.landed = Some(dir);
                }
                Reply::Subscribed => {
                    self.fetching = None;
                    self.downloading = None;
                    self.status = Status::Idle;
                    self.wait_for_files();
                }
                Reply::NeedsAccount => {
                    self.fetching = None;
                    self.downloading = None;
                    self.subscribing = None;
                    self.needs_account = true;
                    self.status = Status::Failed(
                        "haru has no way to fetch this yet — sign in, or start Steam".to_owned(),
                    );
                }
                Reply::Failed(why) => {
                    self.fetching = None;
                    self.downloading = None;
                    self.subscribing = None;
                    self.status = Status::Failed(why);
                }
                _ => {}
            }
        }
    }

    fn item_dir(&self, id: u64) -> Option<PathBuf> {
        self.install_root
            .as_ref()
            .map(|root| root.join(format!("steamapps/workshop/content/431960/{id}")))
            .filter(|dir| dir.join("project.json").is_file())
    }

    fn settings_for(&mut self, ui: &mut egui::Ui, id: &str, dir: &Path, engine: &Engine) {
        let live = engine
            .snapshot()
            .screens
            .iter()
            .any(|screen| screen.current.as_deref() == Some(dir));
        let on = engine
            .snapshot()
            .screens
            .iter()
            .find(|screen| screen.current.as_deref() == Some(dir))
            .map(|screen| screen.name.clone());

        match self.settings.show(ui, id, dir, live) {
            crate::props::Outcome::Changed(key, value) => {
                if let Some(screen) = on {
                    engine.property(&screen, &key, &value);
                }
                self.status = Status::Idle;
            }
            crate::props::Outcome::Failed(why) => self.status = Status::Failed(why),
            crate::props::Outcome::Reset | crate::props::Outcome::Nothing => {}
        }
    }

    fn reselect(&mut self, page: &BrowsePage) {
        let Some(wanted) = self.selected_id else {
            return;
        };
        let found = self
            .appended
            .iter()
            .chain(page.items.iter())
            .position(|item| item.item.id.get() == wanted);
        self.selected = found;
        if found.is_none() {
            self.selected_id = None;
        }
    }

    fn wait_for_files(&mut self) {
        let Some(id) = self.subscribing.take() else {
            return;
        };
        let Some(root) = self.install_root.as_ref() else {
            self.status = Status::Failed(
                "Steam has it, but there is no Steam library set to look in — Settings".to_owned(),
            );
            return;
        };
        let dir = root.join(format!("steamapps/workshop/content/431960/{id}"));
        self.landing = Some((id, dir, 0.0));
    }

    fn check_files(&mut self, ctx: &egui::Context) {
        let Some((id, dir, since)) = self.landing.clone() else {
            return;
        };

        let now = ctx.input(|input| input.time);
        let since = if since <= 0.0 {
            self.landing = Some((id, dir.clone(), now));
            now
        } else {
            since
        };

        if dir.join("project.json").is_file() {
            self.landing = None;
            self.installed.push(id);
            self.landed = Some(dir);
            self.status = Status::Idle;
            return;
        }
        if now - since > LANDING {
            self.landing = None;
            self.status = Status::Failed(
                "Steam took the subscription but the files have not arrived — check its Downloads"
                    .to_owned(),
            );
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(700));
    }

    fn search(&mut self) {
        if let Some(id) = self.awaiting.take() {
            self.workshop.discard(id);
        }
        self.filters.page = 1;
        self.appended.clear();
        self.page = None;
        self.run();
    }

    fn go_to(&mut self, page: u32) {
        if self.awaiting.is_some() || page == self.filters.page {
            return;
        }
        self.filters.page = page.max(1);
        self.run();
    }

    fn run(&mut self) {
        self.status = Status::Searching;
        self.awaiting = Some(self.workshop.send(Request::Browse(self.filters.to_query())));
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            theme::heading(ui, "Filters");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.filters.is_narrowed()
                    && ui
                        .add(egui::Button::new(
                            RichText::new("Clear all").size(11.0).color(theme::MUTED),
                        ))
                        .clicked()
                {
                    self.filters.clear();
                    self.typed.clear();
                    changed = true;
                }
            });
        });
        ui.add_space(8.0);

        self.search_box(ui);
        ui.add_space(10.0);

        changed |= self.tag_groups(ui);

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        changed |= ui
            .checkbox(&mut self.filters.adult, "Show adult content")
            .changed();

        if changed {
            self.search();
        }
    }

    /// Sort, period and what the search found — above the grid, where the eye
    /// lands before it reaches the pictures.
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let total = self.page.as_ref().map_or(0, |page| page.total);

        ui.horizontal(|ui| {
            theme::heading(ui, "Workshop");
            ui.add_space(6.0);
            if total > 0 {
                ui.label(
                    RichText::new(format!("{} wallpapers", thousands(u64::from(total))))
                        .size(11.0)
                        .color(theme::MUTED),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("sort")
                    .selected_text(RichText::new(sort_label(self.filters.sort)).size(12.0))
                    .width(150.0)
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

                if self.filters.sort == BrowseSort::Trend {
                    egui::ComboBox::from_id_salt("period")
                        .selected_text(
                            RichText::new(period_label(self.filters.trend_days)).size(12.0),
                        )
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for (label, days) in TREND_PERIODS {
                                changed |= ui
                                    .selectable_value(
                                        &mut self.filters.trend_days,
                                        Some(*days),
                                        *label,
                                    )
                                    .changed();
                            }
                        });
                }
            });
        });

        changed |= self.chosen_pills(ui);
        if changed {
            self.search();
        }
    }

    /// Every tag currently narrowing the search, each one droppable on its own.
    fn chosen_pills(&mut self, ui: &mut egui::Ui) -> bool {
        let picked: Vec<(usize, String)> = self
            .filters
            .chosen
            .iter()
            .enumerate()
            .flat_map(|(group, tags)| tags.iter().map(move |tag| (group, tag.clone())))
            .collect();
        if picked.is_empty() {
            return false;
        }

        let mut dropped = None;
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for (group, tag) in &picked {
                if theme::chip(ui, &format!("{tag}  ✕"), true)
                    .interact(egui::Sense::click())
                    .on_hover_text("Stop filtering by this")
                    .clicked()
                {
                    dropped = Some((*group, tag.clone()));
                }
            }
        });

        if let Some((group, tag)) = dropped
            && let Some(slot) = self.filters.chosen.get_mut(group)
        {
            slot.retain(|held| held != &tag);
            return true;
        }
        false
    }

    fn download_row(&mut self, ui: &mut egui::Ui, found: &BrowseResult, engine: &Engine) {
        let id = found.item.id.get();
        let on_disk = self.installed.contains(&id)
            || self.install_root.as_ref().is_some_and(|root| {
                root.join(format!("steamapps/workshop/content/431960/{id}"))
                    .join("project.json")
                    .is_file()
            });

        if self.subscribing == Some(id) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("asking Steam to subscribe\u{2026}")
                        .small()
                        .color(theme::MUTED),
                );
            });
            return;
        }
        if let Some((waiting, _, _)) = self.landing
            && waiting == id
        {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Steam is downloading it\u{2026}")
                        .small()
                        .color(theme::MUTED),
                );
            });
            return;
        }

        match self.downloading {
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
            Some(_) => {
                ui.add_enabled(
                    false,
                    egui::Button::new("Download").min_size(egui::vec2(ui.available_width(), 30.0)),
                );
            }
            None if on_disk => {
                ui.horizontal(|ui| {
                    theme::chip(ui, "Installed", true);
                    ui.label(
                        RichText::new("ready to put up")
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                });
                if let Some(dir) = self.item_dir(id) {
                    ui.add_space(10.0);
                    self.settings_for(ui, &id.to_string(), &dir, engine);
                }
            }
            None => {
                if theme::primary(ui, "Download").clicked() {
                    if self.client {
                        self.subscribing = Some(id);
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

    fn tiles(
        &mut self,
        ui: &mut egui::Ui,
        previews: &mut Previews,
        items: &[BrowseResult],
        columns: usize,
        tile_width: f32,
    ) -> bool {
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
                                self.selected_id = Some(found.item.id.get());
                            }
                        }
                    });
                    ui.add_space(10.0);
                }

                if self.infinite {
                    let (rect, _) = ui
                        .allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
                    hit_bottom = ui.is_rect_visible(rect);
                    if self.awaiting.is_some() {
                        ui.vertical_centered(|ui| ui.spinner());
                        ui.add_space(8.0);
                    }
                }
            });
        hit_bottom
    }

    fn waiting_or_error(&self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| match &self.status {
            Status::Failed(why) => {
                ui.label(RichText::new(why).color(ui.visuals().error_fg_color));
            }
            _ => {
                ui.spinner();
            }
        });
    }

    fn page_strip(&mut self, ui: &mut egui::Ui, total: u32) {
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
    }

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
                    let picked = chosen.clone();
                    egui::CollapsingHeader::new(summary(group.label, &picked))
                        .id_salt(group.label)
                        .show(ui, |ui| {
                            for tag in group.tags {
                                let mut on = picked.iter().any(|held| held == tag);
                                if ui.checkbox(&mut on, *tag).changed()
                                    && let Some(slot) = self.filters.chosen.get_mut(index)
                                {
                                    if on {
                                        slot.push((*tag).to_owned());
                                    } else {
                                        slot.retain(|held| held != tag);
                                    }
                                    changed = true;
                                }
                            }
                            if !picked.is_empty()
                                && ui.button(format!("Any {}", group.label)).clicked()
                                && let Some(slot) = self.filters.chosen.get_mut(index)
                            {
                                slot.clear();
                                changed = true;
                            }
                        });
                    ui.add_space(4.0);
                }
            });
        changed
    }
    fn grid(&mut self, ui: &mut egui::Ui, previews: &mut Previews) {
        if self.page.is_none() {
            self.waiting_or_error(ui);
            return;
        }
        let items: Vec<BrowseResult> = self
            .appended
            .iter()
            .chain(self.page.iter().flat_map(|page| page.items.iter()))
            .cloned()
            .collect();

        if items.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Nothing matched those filters.").weak());
            });
            return;
        }

        let (columns, tile_width) = tile::columns_for(ui.available_width(), TILE, 8.0);

        // The page is sized to the window, not to the grid: the detail pane
        // takes width from the grid, and a page that shrank when a wallpaper
        // was opened would search again and throw away the thing being looked
        // at.
        let whole = ui.available_width() + if self.selected.is_some() { DETAIL } else { 0.0 };
        let (across, wide) = tile::columns_for(whole, TILE, 8.0);
        let rows = tile::rows_for(ui.available_height(), wide, 10.0);
        self.fit_page(ui, across.saturating_mul(rows));

        let hit_bottom = self.tiles(ui, previews, &items, columns, tile_width);
        if hit_bottom
            && self.awaiting.is_none()
            && self.page.as_ref().is_some_and(BrowsePage::has_more)
        {
            self.filters.page = self.filters.page.saturating_add(1);
            self.run();
        }
    }

    fn fit_page(&mut self, ui: &egui::Ui, wanted: usize) {
        if !self.fit {
            return;
        }

        let wanted = u32::try_from(wanted)
            .unwrap_or(tapline::MAX_PER_PAGE)
            .clamp(1, tapline::MAX_PER_PAGE);
        if wanted == self.filters.per_page {
            self.settling = None;
            return;
        }

        let now = ui.input(|input| input.time);
        let since = match self.settling {
            Some((held, at)) if held == wanted => at,
            _ => {
                self.settling = Some((wanted, now));
                now
            }
        };
        if now - since < SETTLE {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(120));
            return;
        }
        if self.awaiting.is_some() {
            return;
        }

        self.settling = None;
        self.filters.per_page = wanted;
        if self.infinite {
            return;
        }
        self.filters.page = 1;
        self.run();
    }

    fn detail(
        &mut self,
        ui: &mut egui::Ui,
        previews: &mut Previews,
        index: usize,
        engine: &haru_apply::Engine,
    ) {
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
                // Picture first: it is what the choice is actually made on.
                if let Some(url) = found.preview_url.as_deref()
                    && let Some(texture) = previews.texture(ui.ctx(), url)
                {
                    ui.add(
                        egui::Image::new(&texture)
                            .max_width(ui.available_width())
                            .rounding(10.0),
                    );
                    ui.add_space(10.0);
                }

                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(plain_text(&found.item.title))
                                .size(15.0)
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

                Self::facts(ui, &found);
                ui.add_space(10.0);

                self.download_row(ui, &found, engine);

                ui.add_space(12.0);
                Self::numbers(ui, &found);

                let description = plain_text(&found.description);
                if !description.is_empty() {
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(RichText::new(description).size(12.0).color(theme::MUTED));
                }
            });
    }

    /// What kind of wallpaper this is, said in chips rather than a tag soup:
    /// type and resolution first, then anything worth knowing before it runs.
    fn facts(ui: &mut egui::Ui, found: &BrowseResult) {
        const TELLING: [&str; 6] = [
            "Audio responsive",
            "Customizable",
            "Interactive",
            "Puppet Warp",
            "Two Screens",
            "Three Screens",
        ];

        let kind = found
            .tags
            .iter()
            .find(|tag| matches!(tag.as_str(), "Scene" | "Video" | "Web" | "Application"));
        let shape = found.tags.iter().find(|tag| {
            tag.contains("16:9") || tag.contains("21:9") || tag.contains("4:3") || tag.contains(':')
        });
        let size = found
            .tags
            .iter()
            .find(|tag| tag.ends_with('K') || tag.ends_with('p'));

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for chip in kind.into_iter().chain(size).chain(shape) {
                theme::chip(ui, chip, true);
            }
            for tag in &found.tags {
                if TELLING.contains(&tag.as_str()) {
                    theme::chip(ui, tag, false);
                }
            }
        });
    }

    fn numbers(ui: &mut egui::Ui, found: &BrowseResult) {
        let votes = found.votes_up.saturating_add(found.votes_down);
        let rows = [
            ("Size", human_size(found.item.size)),
            ("Subscribers", thousands(found.subscriptions)),
            ("Favourites", thousands(found.favorites)),
            ("Views", thousands(found.views)),
            (
                "Rating",
                match found.score {
                    Some(score) if votes > 0 => {
                        format!("{:.0}% of {} votes", score * 100.0, thousands(votes))
                    }
                    _ => "not rated yet".to_owned(),
                },
            ),
        ];

        for (what, value) in rows {
            ui.horizontal(|ui| {
                ui.label(RichText::new(what).size(11.0).color(theme::MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(value).size(11.5));
                });
            });
        }
    }

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
                    // The count lives in the toolbar now; down here only say
                    // what is still happening.
                    if previews.loading() > 0 {
                        ui.label(
                            RichText::new(format!("{} previews loading", previews.loading()))
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    }
                }
            }

            if self.infinite {
                let shown =
                    self.appended.len() + self.page.as_ref().map_or(0, |page| page.items.len());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.weak(format!("{} loaded", thousands(shown as u64)));
                });
                return;
            }

            self.page_strip(ui, total);
        });
        ui.add_space(4.0);
    }
}

impl Default for Browser {
    fn default() -> Self {
        Self::new()
    }
}

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

fn strip(current: u32, pages: u32) -> Vec<Option<u32>> {
    const AROUND: u32 = 2;

    if pages <= 1 {
        return vec![Some(1)];
    }

    let first_shown = current.saturating_sub(AROUND).max(1);
    let last_shown = current.saturating_add(AROUND).min(pages);

    let mut out = Vec::new();
    if first_shown > 1 {
        out.push(Some(1));
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
        assert!(strip(60_000, 132_618).len() <= 9);
    }

    #[test]
    fn a_gap_of_one_page_is_shown_rather_than_hidden() {
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
