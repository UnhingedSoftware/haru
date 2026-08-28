use std::path::PathBuf;

use egui::{Align, Layout, RichText, Rounding, Sense, Stroke, Vec2};
use haru_apply::{Backend, Screen};
use haru_core::{Config, Installed, human_size, library, overrides, properties};
use haru_media::Previews;
use haru_workshop::{Reply, Request, Workshop};

use crate::theme;

const TILE: f32 = 168.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    Newest,
    Oldest,
    Name,
    Kind,
    Size,
}

impl Order {
    const fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest",
            Self::Oldest => "Oldest",
            Self::Name => "Name",
            Self::Kind => "Type",
            Self::Size => "Size",
        }
    }

    const ALL: [Self; 5] = [
        Self::Newest,
        Self::Oldest,
        Self::Name,
        Self::Kind,
        Self::Size,
    ];
}

pub struct Library {
    items: Vec<Installed>,
    filter: String,
    order: Order,
    selected: Option<usize>,
    target: Option<String>,
    screens: Vec<Screen>,
    status: String,
    confirming: Option<String>,
    settings: Vec<properties::Property>,
    settings_for: Option<PathBuf>,
    workshop: std::rc::Rc<Workshop>,
    unsubscribing: Option<haru_workshop::RequestId>,
    owned: Vec<String>,
    applied: Option<(String, PathBuf)>,
    pending: Option<(String, PathBuf)>,
}

impl Library {
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
            owned: Vec::new(),
            applied: None,
            pending: None,
            confirming: None,
            settings: Vec::new(),
            settings_for: None,
            workshop,
            unsubscribing: None,
        }
    }

    #[must_use]
    pub fn screens(&self) -> &[Screen] {
        &self.screens
    }

    #[must_use]
    pub fn title_of(&self, dir: &std::path::Path) -> Option<String> {
        self.items
            .iter()
            .find(|item| item.dir == dir)
            .map(|item| item.title.clone())
    }

    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub fn set_target(&mut self, screen: String) {
        self.target = Some(screen);
    }

    pub fn refresh(&mut self, config: &Config, backend: Option<&dyn Backend>) {
        self.items = library::scan(&config.libraries());
        let live: Vec<Screen> = backend
            .map(|backend| backend.screens().unwrap_or_default())
            .unwrap_or_default();
        self.owned = live.iter().map(|screen| screen.name.clone()).collect();

        self.screens = live;
        for name in haru_apply::launch::connectors() {
            if !self.screens.iter().any(|screen| screen.name == name) {
                self.screens.push(Screen {
                    name,
                    current: None,
                });
            }
        }
        if self.target.is_none() {
            self.target = self.screens.first().map(|screen| screen.name.clone());
        }
        self.selected = None;
    }

    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        previews: &mut Previews,
        config: &Config,
        backend: Option<&dyn Backend>,
        sidebar: bool,
    ) {
        self.collect();
        if self.unsubscribing.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }

        if sidebar {
            egui::SidePanel::left("screens")
                .resizable(false)
                .exact_width(238.0)
                .frame(theme::panel_frame(theme::Side::Left))
                .show(ctx, |ui| self.sidebar(ui, config, backend));
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

    fn sidebar(&mut self, ui: &mut egui::Ui, config: &Config, backend: Option<&dyn Backend>) {
        ui.heading("Library");
        ui.label(
            RichText::new(match (self.screens.is_empty(), backend) {
                (false, _) => "Click a wallpaper to put it up".to_owned(),
                (true, Some(backend)) => format!("{} is not running", backend.name()),
                (true, None) => "No renderer found".to_owned(),
            })
            .small()
            .color(theme::MUTED),
        );
        ui.add_space(10.0);

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
        if ui.button("Rescan").clicked() {
            self.refresh(config, backend);
            self.status = "rescanned".to_owned();
        }
    }

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
                ui.set_max_width(ui.available_width());

                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        if crate::icons::button(ui, crate::icons::Icon::Close, false)
                            .on_hover_text("Close")
                            .clicked()
                        {
                            self.selected = None;
                        }
                        if crate::icons::button(ui, crate::icons::Icon::External, false)
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

    pub fn take_applied(&mut self) -> Option<(String, PathBuf)> {
        self.applied.take()
    }

    pub fn take_pending(&mut self) -> Option<(String, PathBuf)> {
        self.pending.take()
    }

    pub fn say(&mut self, what: impl Into<String>) {
        self.status = what.into();
    }

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

        self.load_settings(item);

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

    fn load_settings(&mut self, item: &Installed) {
        if self.settings_for.as_deref() == Some(item.dir.as_path()) {
            return;
        }

        let mut read = properties::read(&item.dir);
        let saved = overrides::read(&item.id);
        for property in &mut read {
            if let Some(value) = saved.get(&property.key) {
                property.set_from_wire(value);
            }
        }

        self.settings = read;
        self.settings_for = Some(item.dir.clone());
    }

    fn apply(&mut self, screen: &str, dir: &std::path::Path, backend: Option<&dyn Backend>) {
        let owns = self.owned.iter().any(|name| name == screen);
        let Some(backend) = backend.filter(|_| owns) else {
            self.pending = Some((screen.to_owned(), dir.to_owned()));
            return;
        };

        if let Some(item) = self.items.iter().find(|item| item.dir == dir) {
            for (key, value) in overrides::read(&item.id) {
                let _ = backend.stage(&key, &value);
            }
        }
        self.status = match backend.apply(screen, dir) {
            Ok(()) => {
                if let Some(found) = self
                    .screens
                    .iter_mut()
                    .find(|candidate| candidate.name == screen)
                {
                    found.current = Some(dir.to_owned());
                }
                self.applied = Some((screen.to_owned(), dir.to_owned()));
                format!("applied to {screen}")
            }
            Err(why) => why,
        };
    }

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

fn open(target: &std::path::Path) {
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
        let mut library = library();
        library.filter = "rain".to_owned();
        assert_eq!(library.shown().first().map(|(index, _)| *index), Some(1));
    }

    #[test]
    fn applying_with_no_renderer_asks_for_one_to_be_started() {
        let mut library = library();
        library.apply("DP-1", std::path::Path::new("/tmp/1"), None);
        assert_eq!(
            library.take_pending(),
            Some(("DP-1".to_owned(), PathBuf::from("/tmp/1")))
        );
        assert_eq!(library.take_pending(), None);
    }
}
