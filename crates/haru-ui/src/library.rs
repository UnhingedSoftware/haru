//! What is already installed, and what is on each screen.
//!
//! The other half of a picker: the Workshop tab finds wallpapers, this one
//! puts them up. A screen is picked on the left, a wallpaper in the middle,
//! and the two together are the whole interaction.

use std::path::PathBuf;

use egui::{Align, Layout, RichText, Rounding, Sense, Stroke, Vec2};
use haru_apply::{Backend, Screen};
use haru_core::{Config, Installed, human_size, library, properties};
use haru_media::Previews;

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
    /// The settings of whatever is on the chosen screen.
    settings: Vec<properties::Property>,
    /// Which wallpaper those settings belong to.
    ///
    /// Read from disk, so they are reread when the screen or its wallpaper
    /// changes and not on every frame.
    settings_for: Option<PathBuf>,
    /// A wallpaper the detail pane asked to open in the preview.
    preview_requested: Option<Installed>,
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}

impl Library {
    /// An empty library, before anything is scanned.
    #[must_use]
    pub fn new() -> Self {
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
            preview_requested: None,
        }
    }

    /// Takes the wallpaper the detail pane asked to preview, if any.
    ///
    /// Handed up rather than acted on: the library does not own the preview
    /// or the tab it lives in.
    pub fn take_preview_request(&mut self) -> Option<Installed> {
        self.preview_requested.take()
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

        ui.add_space(10.0);
        if ui.button("Rescan").clicked() {
            self.refresh(config, backend);
            self.status = "rescanned".to_owned();
        }
    }

    /// The settings of the wallpaper that is currently up.
    ///
    /// A wallpaper's own knobs — its sliders, colours and switches — belong to
    /// the copy the renderer has loaded, so this follows the chosen screen
    /// rather than whatever is selected in the grid. Changing one takes effect
    /// immediately; there is nothing to confirm and nothing to save.
    fn settings_panel(&mut self, ui: &mut egui::Ui, backend: Option<&dyn Backend>) {
        let current = self
            .screens
            .iter()
            .find(|screen| Some(screen.name.as_str()) == self.target.as_deref())
            .and_then(|screen| screen.current.clone());

        // Reread only when the wallpaper under the settings changes.
        if self.settings_for != current {
            self.settings = current.as_deref().map(properties::read).unwrap_or_default();
            self.settings_for = current.clone();
        }

        let Some(dir) = current else {
            ui.label(RichText::new("Settings").small().color(theme::MUTED));
            ui.add_space(2.0);
            ui.label(
                RichText::new("Nothing is on this screen yet.")
                    .small()
                    .color(theme::MUTED),
            );
            return;
        };

        let title = self
            .items
            .iter()
            .find(|item| item.dir == dir)
            .map_or_else(|| "Current wallpaper".to_owned(), |item| item.title.clone());

        ui.label(RichText::new("Settings").small().color(theme::MUTED));
        ui.add(egui::Label::new(RichText::new(&title).size(12.0)).truncate());
        ui.add_space(4.0);

        if self.settings.is_empty() {
            ui.label(
                RichText::new("This wallpaper has no settings.")
                    .small()
                    .color(theme::MUTED),
            );
            return;
        }

        let Some(screen) = self.target.clone() else {
            return;
        };
        let mut changed: Option<(String, String)> = None;

        egui::ScrollArea::vertical()
            .id_salt("wallpaper-settings")
            .auto_shrink([false, false])
            .max_height(260.0)
            .show(ui, |ui| {
                for property in &mut self.settings {
                    if crate::widgets::property(ui, property) {
                        changed = Some((property.key.clone(), property.wire()));
                    }
                    ui.add_space(6.0);
                }
            });

        if let Some((key, value)) = changed {
            self.status = match backend {
                Some(backend) => match backend.set_property(&screen, &key, &value) {
                    Ok(()) => format!("{key} = {value}"),
                    Err(why) => why,
                },
                None => "no renderer to change it with".to_owned(),
            };
        }
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

        let columns = ((ui.available_width() / (TILE + 12.0)).floor() as usize).max(1);
        let mut apply: Option<(String, PathBuf)> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in shown.chunks(columns) {
                    ui.horizontal(|ui| {
                        for (index, item) in row {
                            let response = tile(ui, previews, item, self.selected == Some(*index));
                            if response.clicked() {
                                self.selected = Some(*index);
                            }
                            // A double click is the shortcut people try first,
                            // and it should do the obvious thing.
                            if response.double_clicked()
                                && let Some(target) = self.target.clone()
                            {
                                apply = Some((target, item.dir.clone()));
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
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(&item.title).size(17.0));
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        if ui.small_button("✕").clicked() {
                            self.selected = None;
                        }
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

                ui.add_space(10.0);
                let target = self.target.clone();
                ui.add_enabled_ui(target.is_some() && backend.is_some(), |ui| {
                    let label = target
                        .as_deref()
                        .map_or_else(|| "Apply".to_owned(), |screen| format!("Apply to {screen}"));
                    if ui
                        .add_sized([ui.available_width(), 32.0], egui::Button::new(label))
                        .clicked()
                        && let Some(screen) = target
                    {
                        self.apply(&screen, &item.dir, backend);
                    }
                });

                ui.add_space(6.0);
                if ui
                    .add_sized([ui.available_width(), 28.0], egui::Button::new("Preview & edit"))
                    .on_hover_text("Render it off-screen; nothing on your screens changes")
                    .clicked()
                {
                    self.preview_requested = Some(item.clone());
                }

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Open folder").clicked() {
                        open(&item.dir);
                        self.status = "opened the folder".to_owned();
                    }
                    if ui.button("Copy path").clicked() {
                        ui.output_mut(|out| {
                            out.copied_text = item.dir.to_string_lossy().into_owned();
                        });
                        self.status = "path copied".to_owned();
                    }
                    if ui.button("Workshop page").clicked() {
                        open(std::path::Path::new(&format!(
                            "https://steamcommunity.com/sharedfiles/filedetails/?id={}",
                            item.id
                        )));
                        self.status = "opened the Workshop page".to_owned();
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                if self.confirming.as_deref() == Some(item.id.as_str()) {
                    ui.label(
                        RichText::new("Delete the files? Steam will fetch it again unless you unsubscribe there too.")
                            .small()
                            .color(theme::MUTED),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            self.status = match std::fs::remove_dir_all(&item.dir) {
                                Ok(()) => {
                                    self.items.retain(|other| other.id != item.id);
                                    self.selected = None;
                                    format!("deleted {}", item.title)
                                }
                                Err(error) => format!("could not delete: {error}"),
                            };
                            self.confirming = None;
                        }
                        if ui.button("Keep").clicked() {
                            self.confirming = None;
                        }
                    });
                } else if ui.button("Delete from disk").clicked() {
                    self.confirming = Some(item.id.clone());
                }
            });
    }

    /// Applies one wallpaper and records what happened.
    fn apply(&mut self, screen: &str, dir: &std::path::Path, backend: Option<&dyn Backend>) {
        let Some(backend) = backend else {
            self.status = "no renderer to apply with".to_owned();
            return;
        };
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
    selected: bool,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(TILE, TILE + 40.0),
        Layout::top_down(Align::Min),
        |ui| {
            ui.set_min_width(TILE);
            ui.set_max_width(TILE);
            ui.spacing_mut().item_spacing.y = 2.0;

            let (rect, response) = ui.allocate_exact_size(Vec2::new(TILE, TILE), Sense::click());
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
            ..Library::new()
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
