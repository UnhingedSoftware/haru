//! What is already installed, and what is on each screen.
//!
//! The other half of a picker: the Workshop tab finds wallpapers, this one
//! puts them up. A screen is picked on the left, a wallpaper in the middle,
//! and the two together are the whole interaction.

use std::path::PathBuf;

use egui::{Align, Layout, RichText, Rounding, Sense, Stroke, Vec2};
use haru_apply::{Backend, Screen};
use haru_core::{Config, Installed, human_size, library, overrides, properties};
use haru_media::Previews;
use haru_workshop::{Reply, Request, Workshop};

use crate::theme;

/// How wide a wallpaper tile is.
const TILE: f32 = 168.0;

/// How a library can be ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    /// Most recently installed first.
    Newest,
    /// Least recently installed first.
    Oldest,
    /// By title.
    Name,
    /// By kind, then title.
    Kind,
    /// Largest first, which is what someone reclaiming disk wants.
    Size,
}

impl Order {
    /// What it is called in the window.
    const fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest",
            Self::Oldest => "Oldest",
            Self::Name => "Name",
            Self::Kind => "Type",
            Self::Size => "Size",
        }
    }

    /// Every order, in the order they are offered.
    const ALL: [Self; 5] = [
        Self::Newest,
        Self::Oldest,
        Self::Name,
        Self::Kind,
        Self::Size,
    ];
}

/// The installed-wallpaper view.
pub struct Library {
    items: Vec<Installed>,
    /// What the filter box holds. Local, so it costs nothing to type.
    filter: String,
    order: Order,
    selected: Option<usize>,
    /// Which screen an apply goes to.
    target: Option<String>,
    screens: Vec<Screen>,
    /// The last thing that happened, shown in the bar.
    status: String,
    /// The item a delete is waiting to be confirmed for.
    ///
    /// Deleting is the one irreversible thing here — Steam has to be told to
    /// unsubscribe separately or it downloads the item again — so it asks.
    confirming: Option<String>,
    /// The settings of the wallpaper the sidebar is showing.
    settings: Vec<properties::Property>,
    /// Which wallpaper those settings belong to.
    ///
    /// Read from disk, so they are reread when the subject changes rather than
    /// on every frame.
    settings_for: Option<PathBuf>,
    /// The connection unsubscribing goes over, shared with the browser.
    workshop: std::rc::Rc<Workshop>,
    /// The unsubscribe in flight, so its failure lands here and not elsewhere.
    unsubscribing: Option<haru_workshop::RequestId>,
}

impl Library {
    /// Takes anything the connection has answered.
    ///
    /// Only unsubscribes are sent from here, so anything else belongs to the
    /// browser and is left for it.
    fn collect(&mut self) {
        if let Some(id) = self.unsubscribing
            && let Some(reply) = self.workshop.take(id)
        {
            self.unsubscribing = None;
            self.status = match reply {
                Reply::Unsubscribed => "unsubscribed".to_owned(),
                Reply::Failed(why) => why,
                _ => return,
            };
        }
    }
}

impl Library {
    /// An empty library, before anything is scanned.
    #[must_use]
    pub fn new(workshop: std::rc::Rc<Workshop>) -> Self {
        Self {
            items: Vec::new(),
            filter: String::new(),
            order: Order::Newest,
            selected: None,
            target: None,
            screens: Vec::new(),
            status: String::new(),
            confirming: None,
            settings: Vec::new(),
            settings_for: None,
            workshop,
            unsubscribing: None,
        }
    }

    /// The screens the renderer knows about.
    #[must_use]
    pub fn screens(&self) -> &[Screen] {
        &self.screens
    }

    /// Which screen an apply goes to.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// Chooses the screen an apply goes to.
    pub fn set_target(&mut self, screen: String) {
        self.target = Some(screen);
    }

    /// Rereads the libraries and the screens.
    pub fn refresh(&mut self, config: &Config, backend: Option<&dyn Backend>) {
        self.items = library::scan(&config.libraries());
        self.screens = backend
            .map(|backend| backend.screens().unwrap_or_default())
            .unwrap_or_default();
        if self.target.is_none() {
            self.target = self.screens.first().map(|screen| screen.name.clone());
        }
        self.selected = None;
    }

    /// Draws the view.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        previews: &mut Previews,
        config: &Config,
        backend: Option<&dyn Backend>,
        sidebar: bool,
    ) {
        // Whatever the connection has answered since the last frame — an
        // unsubscribe is sent from here, and its outcome belongs in the bar.
        self.collect();
        if self.unsubscribing.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }

        if sidebar {
            egui::SidePanel::left("screens")
                .resizable(false)
                .exact_width(238.0)
                .frame(theme::panel_frame(theme::Side::Left))
                .show(ctx, |ui| self.sidebar(ui, previews, config, backend));
        }

        if let Some(index) = self.selected {
            egui::SidePanel::right("wallpaper")
                .resizable(false)
                .exact_width(312.0)
                .frame(theme::panel_frame(theme::Side::Right))
                .show(ctx, |ui| self.detail(ui, previews, index, backend));
        }

        egui::TopBottomPanel::bottom("library-status")
            .frame(theme::panel_frame(theme::Side::Left))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let total: u64 = self.items.iter().map(|item| item.size).sum();
                    ui.label(format!(
                        "{} installed · {}",
                        self.items.len(),
                        human_size(total)
                    ));
                    if !self.status.is_empty() {
                        ui.separator();
                        ui.label(RichText::new(&self.status).color(theme::MUTED));
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(theme::panel_frame(theme::Side::Middle))
            .show(ctx, |ui| self.grid(ui, previews, backend));
    }

    /// Screens on the left, with what is on them.
    fn sidebar(
        &mut self,
        ui: &mut egui::Ui,
        previews: &mut Previews,
        config: &Config,
        backend: Option<&dyn Backend>,
    ) {
        if self.screens.is_empty() {
            ui.label(
                RichText::new(match backend {
                    Some(backend) => format!("{} is not running", backend.name()),
                    None => "No renderer found".to_owned(),
                })
                .small()
                .color(theme::MUTED),
            );
        } else {
            ui.heading("Screens");
            ui.add_space(6.0);
        }

        for screen in self.screens.clone() {
            let chosen = self.target.as_deref() == Some(screen.name.as_str());
            let response =
                ui.allocate_response(Vec2::new(ui.available_width(), 92.0), Sense::click());
            let rect = response.rect;
            let rounding = Rounding::same(8.0);
            ui.painter()
                .rect_filled(rect, rounding, ui.visuals().extreme_bg_color);

            // The wallpaper that is up, as the card's own background: the
            // fastest way to answer "which screen is which".
            if let Some(current) = screen.current.as_ref() {
                if let Some(texture) = self
                    .items
                    .iter()
                    .find(|item| &item.dir == current)
                    .and_then(|item| item.preview.as_ref())
                    .and_then(|path| previews.texture_path(ui.ctx(), path))
                {
                    egui::Image::new(&texture)
                        .rounding(rounding)
                        .maintain_aspect_ratio(true)
                        .fit_to_exact_size(rect.size())
                        .tint(egui::Color32::from_white_alpha(150))
                        .paint_at(ui, rect);
                }
            }

            if chosen {
                ui.painter()
                    .rect_stroke(rect, rounding, Stroke::new(2.0_f32, theme::ACCENT));
            }

            let title = self
                .items
                .iter()
                .find(|item| Some(&item.dir) == screen.current.as_ref())
                .map_or_else(|| "nothing".to_owned(), |item| item.title.clone());
            ui.painter().text(
                rect.left_top() + Vec2::new(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                &screen.name,
                egui::FontId::proportional(14.0),
                theme::TEXT,
            );
            ui.painter().text(
                rect.left_bottom() + Vec2::new(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                title,
                egui::FontId::proportional(11.0),
                theme::MUTED,
            );

            if response.clicked() {
                self.target = Some(screen.name.clone());
            }
            ui.add_space(6.0);
        }

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);

        ui.label(RichText::new("Filter").small().color(theme::MUTED));
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Title or type")
                .desired_width(f32::INFINITY),
        );

        ui.add_space(8.0);
        ui.label(RichText::new("Order").small().color(theme::MUTED));
        egui::ComboBox::from_id_salt("library-order")
            .selected_text(self.order.label())
            .width(200.0)
            .show_ui(ui, |ui| {
                for order in Order::ALL {
                    ui.selectable_value(&mut self.order, order, order.label());
                }
            });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        self.settings_panel(ui, backend);
        ui.add_space(6.0);

        ui.add_space(10.0);
        if ui.button("Rescan").clicked() {
            self.refresh(config, backend);
            self.status = "rescanned".to_owned();
        }
    }

    /// The settings of the wallpaper being looked at.
    ///
    /// Whatever is selected in the grid, or — with nothing selected — whatever
    /// is on the chosen screen. Selecting is how you ask "what can this one
    /// do", and answering with a different wallpaper's knobs would be worse
    /// than answering with none.
    ///
    /// Where a change goes depends on whether that wallpaper is up. On the
    /// screen: straight to the renderer, visible immediately. Not on it: kept
    /// against the wallpaper's id and staged when it is next applied, because
    /// there is nothing loaded to change.
    fn settings_panel(&mut self, ui: &mut egui::Ui, backend: Option<&dyn Backend>) {
        // With something selected, its own pane carries this — showing it in
        // both places means two panels of controls for two wallpapers.
        if self.selected.is_some() {
            return;
        }

        let on_screen = self
            .screens
            .iter()
            .find(|screen| Some(screen.name.as_str()) == self.target.as_deref())
            .and_then(|screen| screen.current.clone());

        let subject = self
            .selected
            .and_then(|index| self.items.get(index))
            .map(|item| item.dir.clone())
            .or_else(|| on_screen.clone());

        // Reread only when the subject changes, with whatever was saved for it
        // folded in so the panel shows what you last set rather than the
        // wallpaper's defaults.
        if self.settings_for != subject {
            self.settings = match subject.as_deref() {
                Some(dir) => {
                    let mut read = properties::read(dir);
                    if let Some(item) = self.items.iter().find(|item| item.dir == dir) {
                        let saved = overrides::read(&item.id);
                        for property in &mut read {
                            if let Some(value) = saved.get(&property.key) {
                                property.set_from_wire(value);
                            }
                        }
                    }
                    read
                }
                None => Vec::new(),
            };
            self.settings_for = subject.clone();
        }

        let Some(dir) = subject else {
            ui.label(RichText::new("Settings").small().color(theme::MUTED));
            ui.add_space(2.0);
            ui.label(
                RichText::new("Pick a wallpaper to see what it can do.")
                    .small()
                    .color(theme::MUTED),
            );
            return;
        };

        let item = self.items.iter().find(|item| item.dir == dir).cloned();
        let title = item
            .as_ref()
            .map_or_else(|| "Current wallpaper".to_owned(), |item| item.title.clone());
        let live = Some(&dir) == on_screen.as_ref();

        // Reached only with nothing selected, which means this *is* the
        // wallpaper on the screen. A wallpaper being looked at rather than
        // shown is the pane's business, not the sidebar's.
        if !live {
            ui.label(RichText::new("Wallpaper").small().color(theme::MUTED));
            ui.add(egui::Label::new(RichText::new(&title).size(13.0)).truncate());
            ui.add_space(4.0);
            ui.label(
                RichText::new("Its settings appear once it is on a screen.")
                    .small()
                    .color(theme::MUTED),
            );
            return;
        }

        ui.label(RichText::new("Settings").small().color(theme::MUTED));
        ui.add(egui::Label::new(RichText::new(&title).size(12.0)).truncate());
        ui.label(
            RichText::new("On this screen — changes show at once.")
                .small()
                .color(theme::MUTED),
        );
        ui.add_space(4.0);

        if self.settings.is_empty() {
            ui.label(
                RichText::new("This wallpaper has no settings.")
                    .small()
                    .color(theme::MUTED),
            );
            return;
        }

        let screen = self.target.clone();
        let mut changed: Option<(String, String)> = None;
        let mut reset = false;

        egui::ScrollArea::vertical()
            .id_salt("wallpaper-settings")
            .auto_shrink([false, true])
            .max_height(300.0)
            .show(ui, |ui| {
                for property in &mut self.settings {
                    if crate::widgets::property(ui, property) {
                        changed = Some((property.key.clone(), property.wire()));
                    }
                    ui.add_space(6.0);
                }
                if item.is_some() {
                    ui.add_space(4.0);
                    reset = ui.button("Reset to defaults").clicked();
                }
            });

        if reset && let Some(item) = item.as_ref() {
            self.status = match overrides::clear(&item.id) {
                Ok(()) => {
                    // Reread from the wallpaper itself, with nothing folded in.
                    self.settings = properties::read(&dir);
                    "settings reset".to_owned()
                }
                Err(why) => why,
            };
            return;
        }

        let Some((key, value)) = changed else { return };

        // Kept first, so a change survives whether or not a renderer takes it.
        if let Some(item) = item.as_ref()
            && let Err(why) = overrides::set(&item.id, &key, &value)
        {
            self.status = why;
            return;
        }

        // Only reached for the wallpaper on the screen, so there is always
        // something loaded to change.
        self.status = match (backend, screen) {
            (Some(backend), Some(screen)) => match backend.set_property(&screen, &key, &value) {
                Ok(()) => format!("{key} = {value}"),
                Err(why) => why,
            },
            _ => "no renderer to change it with".to_owned(),
        };
    }

    /// The wallpapers themselves.
    fn grid(&mut self, ui: &mut egui::Ui, previews: &mut Previews, backend: Option<&dyn Backend>) {
        let shown = self.shown();

        if shown.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(if self.items.is_empty() {
                        "No wallpapers installed yet — find some in the Workshop tab."
                    } else {
                        "Nothing matches that filter."
                    })
                    .color(theme::MUTED),
                );
            });
            return;
        }

        let (columns, tile_width) = crate::tile::columns_for(ui.available_width(), TILE, 8.0);
        let mut apply: Option<(String, PathBuf)> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in shown.chunks(columns) {
                    ui.horizontal(|ui| {
                        for (index, item) in row {
                            let response = tile(
                                ui,
                                previews,
                                item,
                                tile_width,
                                self.selected == Some(*index),
                            );
                            // Clicking a wallpaper puts it up. Selecting was
                            // the old meaning and is still what happens — the
                            // pane opens on it — but choosing a wallpaper in a
                            // wallpaper picker means wanting to see it.
                            if response.clicked() {
                                self.selected = Some(*index);
                                if let Some(target) = self.target.clone() {
                                    apply = Some((target, item.dir.clone()));
                                }
                            }
                        }
                    });
                    ui.add_space(10.0);
                }
            });

        if let Some((screen, dir)) = apply {
            self.apply(&screen, &dir, backend);
        }
    }

    /// One wallpaper, in full.
    fn detail(
        &mut self,
        ui: &mut egui::Ui,
        previews: &mut Previews,
        index: usize,
        backend: Option<&dyn Backend>,
    ) {
        let Some(item) = self.items.get(index).cloned() else {
            self.selected = None;
            return;
        };

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Everything in the pane is bounded by it: a Workshop title
                // runs as long as its author liked, and an unbounded label is
                // drawn straight past the panel's edge.
                ui.set_max_width(ui.available_width());

                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        if ui.small_button("✕").on_hover_text("Close").clicked() {
                            self.selected = None;
                        }
                        // An icon, not a row of buttons: opening the page is
                        // one thing you occasionally want, not a decision.
                        if ui
                            .small_button("↗")
                            .on_hover_text("Open the Workshop page")
                            .clicked()
                        {
                            open(std::path::Path::new(&format!(
                                "https://steamcommunity.com/sharedfiles/filedetails/?id={}",
                                item.id
                            )));
                        }
                        ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&item.title).size(15.0).strong())
                                    .truncate(),
                            )
                            .on_hover_text(&item.title);
                        });
                    });
                });

                if let Some(texture) = item
                    .preview
                    .as_ref()
                    .and_then(|path| previews.texture_path(ui.ctx(), path))
                {
                    ui.add_space(6.0);
                    ui.add(
                        egui::Image::new(&texture)
                            .max_width(ui.available_width())
                            .rounding(8.0),
                    );
                }

                ui.add_space(8.0);
                ui.label(format!("{} · {}", item.kind, human_size(item.size)));
                ui.label(RichText::new(&item.id).small().color(theme::MUTED));

                ui.add_space(12.0);

                // No Apply button: clicking the wallpaper in the grid is what
                // applies it, and a second way to do the same thing is a
                // button asking to be pressed for no reason.
                if self.confirming.as_deref() == Some(item.id.as_str()) {
                    ui.label(
                        RichText::new("Remove it and tell Steam you no longer want it?")
                            .small()
                            .color(theme::MUTED),
                    );
                    ui.add_space(6.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), 32.0],
                            egui::Button::new(RichText::new("Unsubscribe").color(theme::TEXT))
                                .fill(theme::DANGER.gamma_multiply(0.7)),
                        )
                        .clicked()
                    {
                        self.unsubscribe(&item);
                    }
                    ui.add_space(4.0);
                    if ui
                        .add_sized([ui.available_width(), 26.0], egui::Button::new("Keep"))
                        .clicked()
                    {
                        self.confirming = None;
                    }
                } else if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new(RichText::new("Unsubscribe").color(theme::DANGER))
                            .stroke(egui::Stroke::new(1.0_f32, theme::DANGER)),
                    )
                    .on_hover_text("Removes the files and the subscription")
                    .clicked()
                {
                    self.confirming = Some(item.id.clone());
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);
                self.properties_of(ui, &item, backend);
            });
    }

    /// Removes a wallpaper, and the reason Steam would bring it back.
    ///
    /// Both halves, because either alone leaves the job half done: the files
    /// go, and Steam is told the account no longer wants the item — otherwise
    /// the client downloads it again on its next sync.
    fn unsubscribe(&mut self, item: &Installed) {
        self.confirming = None;

        if let Ok(id) = item.id.parse::<u64>() {
            self.unsubscribing = Some(self.workshop.send(Request::Unsubscribe {
                app: haru_core::WALLPAPER_ENGINE,
                item: tapline_ids::PublishedFileId(id),
            }));
        }

        self.status = match std::fs::remove_dir_all(&item.dir) {
            Ok(()) => {
                self.items.retain(|other| other.id != item.id);
                self.selected = None;
                format!("removed {}", item.title)
            }
            Err(error) => format!("could not remove the files: {error}"),
        };
    }

    /// Puts a wallpaper on whichever screen is chosen.
    ///
    /// What a finished download asks for: the browser has a directory and no
    /// screen, and this is the half that knows which one.
    pub fn apply_to_target(&mut self, dir: &std::path::Path, backend: Option<&dyn Backend>) {
        let Some(screen) = self
            .target
            .clone()
            .or_else(|| self.screens.first().map(|screen| screen.name.clone()))
        else {
            self.status = "no screen to apply to".to_owned();
            return;
        };
        self.apply(&screen, dir, backend);
    }

    /// What this wallpaper can be told to do, and where telling it goes.
    ///
    /// Editable only while it is on a screen: a property is a message to a
    /// loaded wallpaper, and there is nothing to send it to otherwise. Shown
    /// either way, because "what can this one do" is worth answering before
    /// you commit to putting it up.
    fn properties_of(
        &mut self,
        ui: &mut egui::Ui,
        item: &Installed,
        backend: Option<&dyn Backend>,
    ) {
        let live = self
            .screens
            .iter()
            .any(|screen| screen.current.as_ref() == Some(&item.dir));

        ui.label(RichText::new("Settings").small().color(theme::MUTED));
        ui.add_space(2.0);

        if self.settings.is_empty() {
            ui.label(
                RichText::new("This wallpaper has no settings.")
                    .small()
                    .color(theme::MUTED),
            );
            return;
        }

        if !live {
            ui.label(
                RichText::new("Apply it to change these.")
                    .small()
                    .color(theme::MUTED),
            );
            ui.add_space(4.0);
        }

        let screen = self.target.clone();
        let mut changed: Option<(String, String)> = None;
        let mut reset = false;

        // No scroll area of its own: the pane already scrolls, and a list
        // inside a list means the outer one stops at a boundary the reader
        // cannot see.
        ui.add_enabled_ui(live, |ui| {
            for property in &mut self.settings {
                if crate::widgets::property(ui, property) {
                    changed = Some((property.key.clone(), property.wire()));
                }
                ui.add_space(6.0);
            }
            ui.add_space(4.0);
            reset = ui.button("Reset to defaults").clicked();
        });

        if reset {
            self.status = match overrides::clear(&item.id) {
                Ok(()) => {
                    self.settings = properties::read(&item.dir);
                    "settings reset".to_owned()
                }
                Err(why) => why,
            };
            return;
        }

        let Some((key, value)) = changed else { return };

        // Kept first, so the change survives a renderer that refuses it and is
        // staged again the next time this wallpaper goes up.
        if let Err(why) = overrides::set(&item.id, &key, &value) {
            self.status = why;
            return;
        }

        self.status = match (backend, screen) {
            (Some(backend), Some(screen)) => match backend.set_property(&screen, &key, &value) {
                Ok(()) => format!("{key} = {value}"),
                Err(why) => why,
            },
            _ => "no renderer to change it with".to_owned(),
        };
    }

    /// Applies one wallpaper and records what happened.
    fn apply(&mut self, screen: &str, dir: &std::path::Path, backend: Option<&dyn Backend>) {
        let Some(backend) = backend else {
            self.status = "no renderer to apply with".to_owned();
            return;
        };

        // Staged before the build rather than set after it: a value applied
        // afterwards means the wallpaper appears once with its defaults and
        // then changes, which reads as a glitch.
        if let Some(item) = self.items.iter().find(|item| item.dir == dir) {
            for (key, value) in overrides::read(&item.id) {
                // An older renderer without `stage` still gets the wallpaper
                // up, without the saved changes, which beats refusing.
                let _ = backend.stage(&key, &value);
            }
        }
        self.status = match backend.apply(screen, dir) {
            Ok(()) => {
                // The screen now shows this, and the sidebar card should say
                // so without waiting for a rescan.
                if let Some(found) = self
                    .screens
                    .iter_mut()
                    .find(|candidate| candidate.name == screen)
                {
                    found.current = Some(dir.to_owned());
                }
                format!("applied to {screen}")
            }
            Err(why) => why,
        };
    }

    /// The items the filter and order leave, with their real indices.
    fn shown(&self) -> Vec<(usize, Installed)> {
        let needle = self.filter.trim().to_lowercase();
        let mut shown: Vec<(usize, Installed)> = self
            .items
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, item)| {
                needle.is_empty()
                    || item.title.to_lowercase().contains(&needle)
                    || item.kind.contains(&needle)
            })
            .collect();

        match self.order {
            // Already newest-first from the scan.
            Order::Newest => {}
            Order::Oldest => shown.reverse(),
            Order::Name => shown.sort_by_key(|(_, item)| item.title.to_lowercase()),
            Order::Kind => {
                shown.sort_by_key(|(_, item)| (item.kind.clone(), item.title.to_lowercase()));
            }
            Order::Size => shown.sort_by_key(|(_, item)| std::cmp::Reverse(item.size)),
        }
        shown
    }
}

/// One installed wallpaper in the grid.
fn tile(
    ui: &mut egui::Ui,
    previews: &mut Previews,
    item: &Installed,
    size: f32,
    selected: bool,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(size, size + 40.0),
        Layout::top_down(Align::Min),
        |ui| {
            ui.set_min_width(size);
            ui.set_max_width(size);
            ui.spacing_mut().item_spacing.y = 2.0;

            let (rect, response) = ui.allocate_exact_size(Vec2::new(size, size), Sense::click());
            let rounding = Rounding::same(6.0);
            ui.painter()
                .rect_filled(rect, rounding, ui.visuals().extreme_bg_color);

            // Only while it is on screen: a tile scrolled past stops asking,
            // and the sweep drops what nothing asked for.
            let picture = ui.is_rect_visible(rect).then(|| {
                item.preview
                    .as_ref()
                    .and_then(|path| previews.texture_path(ui.ctx(), path))
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
                        theme::MUTED,
                    );
                }
            }

            if selected {
                ui.painter()
                    .rect_stroke(rect, rounding, Stroke::new(2.0_f32, theme::ACCENT));
            }

            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(&item.title).size(12.0)).truncate());
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{} · {}", item.kind, human_size(item.size)))
                        .size(11.0)
                        .color(theme::MUTED),
                )
                .truncate(),
            );

            response
        },
    )
    .inner
}

/// Hands a path or a URL to the desktop.
fn open(target: &std::path::Path) {
    // Detached and ignored: whether a file manager opened is not something the
    // picker can do anything about, and waiting on one would stall the frame.
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, title: &str, kind: &str, size: u64) -> Installed {
        Installed {
            id: id.to_owned(),
            dir: PathBuf::from(format!("/tmp/{id}")),
            title: title.to_owned(),
            kind: kind.to_owned(),
            preview: None,
            size,
            installed: std::time::UNIX_EPOCH,
        }
    }

    fn library() -> Library {
        Library {
            items: vec![
                item("1", "Neon", "scene", 300),
                item("2", "Rain", "video", 100),
                item("3", "aurora", "scene", 200),
            ],
            ..Library::new(std::rc::Rc::new(Workshop::spawn()))
        }
    }

    #[test]
    fn filtering_matches_title_and_kind_case_insensitively() {
        let mut library = library();
        library.filter = "VIDEO".to_owned();
        assert_eq!(library.shown().len(), 1);

        library.filter = "aur".to_owned();
        assert_eq!(
            library.shown().first().map(|(_, item)| item.title.clone()),
            Some("aurora".to_owned())
        );
    }

    #[test]
    fn ordering_by_name_ignores_case() {
        // Otherwise every lowercase title sorts after every uppercase one,
        // which reads as the sort being broken.
        let mut library = library();
        library.order = Order::Name;
        let titles: Vec<String> = library
            .shown()
            .into_iter()
            .map(|(_, item)| item.title)
            .collect();
        assert_eq!(titles, vec!["aurora", "Neon", "Rain"]);
    }

    #[test]
    fn ordering_by_size_puts_the_biggest_first() {
        // The order for reclaiming disk, so it has to be descending.
        let mut library = library();
        library.order = Order::Size;
        let sizes: Vec<u64> = library
            .shown()
            .into_iter()
            .map(|(_, item)| item.size)
            .collect();
        assert_eq!(sizes, vec![300, 200, 100]);
    }

    #[test]
    fn the_filter_keeps_real_indices_so_a_click_selects_what_was_clicked() {
        // The grid draws a filtered list and stores the index it is given; if
        // that were the position in the filtered list, selecting would pick a
        // different wallpaper as soon as anything was typed.
        let mut library = library();
        library.filter = "rain".to_owned();
        assert_eq!(library.shown().first().map(|(index, _)| *index), Some(1));
    }

    #[test]
    fn applying_with_no_renderer_says_so_rather_than_doing_nothing() {
        let mut library = library();
        library.apply("DP-1", std::path::Path::new("/tmp/1"), None);
        assert!(library.status.contains("no renderer"), "{}", library.status);
    }
}
