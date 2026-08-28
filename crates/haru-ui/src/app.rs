//! The window, and the three things it can be showing.
//!
//! Workshop finds wallpapers, Library puts them up and holds the settings of
//! whatever is currently on each screen, and Settings is the app's own. They
//! share one preview cache, because the same artwork appears in two of them
//! and downloading it twice would be visible.

use egui::RichText;
use haru_apply::Backend;
use haru_core::{Config, Filters};
use haru_media::Previews;

use haru_workshop::Workshop;
use std::rc::Rc;

use crate::{Browser, Library, Preview, Settings, theme};

/// Which view is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Browsing the Workshop.
    Workshop,
    /// The wallpapers already installed, and what is on each screen.
    Library,
    /// One wallpaper, rendered off-screen and editable.
    Preview,
    /// The app's own settings.
    Settings,
}

impl Tab {
    /// Reads a tab from a name, for a launcher or a shortcut key.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "workshop" | "browse" => Some(Self::Workshop),
            "library" | "installed" => Some(Self::Library),
            "preview" | "studio" => Some(Self::Preview),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    /// Every name [`Tab::parse`] accepts, canonical form first.
    pub const NAMES: [&'static str; 4] = ["workshop", "library", "preview", "settings"];

    /// What the tab is called.
    const fn label(self) -> &'static str {
        match self {
            Self::Workshop => "Workshop",
            Self::Library => "Library",
            Self::Preview => "Preview",
            Self::Settings => "Settings",
        }
    }
}

/// The whole application.
pub struct Haru {
    tab: Tab,
    previews: Previews,
    browser: Browser,
    library: Library,
    preview: Preview,
    settings: Settings,
    config: Config,
    /// What renders wallpapers here, if anything does.
    ///
    /// Probed once at startup and rebuilt when the socket setting changes: the
    /// answer does not change on its own, and probing per frame would connect
    /// to a socket sixty times a second.
    backend: Option<Box<dyn Backend>>,
    /// Whether the library has been scanned since it was last invalidated.
    scanned: bool,
    /// Whether the left panel is showing.
    ///
    /// One switch for both views: it is the same edge of the same window, and
    /// someone who hides it to see more wallpapers means it in both.
    sidebar: bool,
}

impl Default for Haru {
    fn default() -> Self {
        Self::new()
    }
}

impl Haru {
    /// Opens the window on the library, which is what someone has when they
    /// already have wallpapers, and reads its settings.
    #[must_use]
    pub fn new() -> Self {
        Self::opening_on(Tab::Library, None)
    }

    /// Opens on a given tab, optionally with a search already run.
    ///
    /// What a launcher, a shortcut or a URL handler hands over: coming up
    /// showing the answer beats coming up on the front page.
    #[must_use]
    pub fn opening_on(tab: Tab, search: Option<String>) -> Self {
        Self::opening_on_item(tab, search, None)
    }

    /// The same, opening the preview on one installed wallpaper by id.
    #[must_use]
    pub fn opening_on_item(tab: Tab, search: Option<String>, item: Option<String>) -> Self {
        let config = Config::load();
        let backend = haru_apply::detect(config.socket.clone());

        let filters = Filters {
            adult: config.adult,
            per_page: config.per_page,
            text: search.unwrap_or_default(),
            ..Filters::new()
        };
        // One connection to Steam, shared: the browser searches over it and
        // the library unsubscribes over it, and a second would be a second
        // login and a second minute of connecting.
        let workshop = Rc::new(Workshop::spawn());
        let mut browser = Browser::with_filters(filters, Rc::clone(&workshop));
        browser.reconfigure(config.adult, config.per_page, config.infinite_scroll);
        browser.set_install_root(config.install_root());
        let mut settings = Settings::default();
        settings.sync(&config);

        let mut preview = Preview::new();
        if let Some(wanted) = item {
            // Scanned here rather than waiting for the Library tab: opening
            // straight into a preview should not need a detour through it.
            if let Some(found) = haru_core::library::scan(&config.libraries())
                .into_iter()
                .find(|installed| installed.id == wanted)
            {
                preview.open(found);
            }
        }

        Self {
            tab,
            previews: Previews::new(),
            browser,
            library: Library::new(workshop),
            preview,
            settings,
            config,
            backend,
            scanned: false,
            sidebar: true,
        }
    }

    /// Draws a frame.
    pub fn ui(&mut self, ctx: &egui::Context) {
        // Scanned on first sight rather than at startup: a library on a slow
        // disk should not hold the window closed.
        if !self.scanned {
            self.library.refresh(&self.config, self.backend.as_deref());
            self.scanned = true;
        }

        self.tabs(ctx);

        // The preview holds a renderer, and a renderer holds a wallpaper.
        // Leaving the tab gives both back.
        if self.tab != Tab::Preview {
            self.preview.suspend();
        }

        match self.tab {
            Tab::Workshop => {
                self.browser.ui(ctx, &mut self.previews, self.sidebar);
                // A wallpaper that just downloaded goes straight up: asking for
                // it was the decision, and a second trip through the Library to
                // see it is a step nobody wants.
                if let Some(dir) = self.browser.take_landed() {
                    self.library.refresh(&self.config, self.backend.as_deref());
                    self.library.apply_to_target(&dir, self.backend.as_deref());
                    self.scanned = true;
                }
            }
            Tab::Library => {
                self.library.ui(
                    ctx,
                    &mut self.previews,
                    &self.config,
                    self.backend.as_deref(),
                    self.sidebar,
                );
            }
            Tab::Preview => self.preview.ui(ctx, self.sidebar),
            Tab::Settings => {
                if self
                    .settings
                    .ui(ctx, &mut self.config, self.backend.as_deref())
                {
                    // A changed socket means a different renderer, and a
                    // changed library means a different set of wallpapers.
                    self.backend = haru_apply::detect(self.config.socket.clone());
                    self.browser.reconfigure(
                        self.config.adult,
                        self.config.per_page,
                        self.config.infinite_scroll,
                    );
                    self.browser.set_install_root(self.config.install_root());
                    self.scanned = false;
                }
            }
        }

        // Everything the frame did not draw is dropped here.
        self.finish_frame();
    }

    /// Ends the frame.
    ///
    /// Pictures the frame did not draw are dropped here, which is what keeps a
    /// long browse from growing: leave the Workshop tab and its tiles go with
    /// it, rather than being held against a return that may never come.
    fn finish_frame(&mut self) {
        self.previews.sweep();
    }

    /// The row of tabs along the top.
    fn tabs(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs")
            .frame(theme::panel_frame(theme::Side::Left))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // The panel is worth more than the button that hides it,
                    // so the button is small and lives before the name.
                    if ui
                        .selectable_label(self.sidebar, "☰")
                        .on_hover_text(if self.sidebar {
                            "Hide the panel"
                        } else {
                            "Show the panel"
                        })
                        .clicked()
                    {
                        self.sidebar = !self.sidebar;
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("haru").size(17.0).strong());
                    ui.add_space(14.0);

                    for tab in [Tab::Workshop, Tab::Library, Tab::Preview, Tab::Settings] {
                        let chosen = self.tab == tab;
                        if ui
                            .selectable_label(
                                chosen,
                                RichText::new(tab.label()).size(13.0).color(if chosen {
                                    theme::TEXT
                                } else {
                                    theme::MUTED
                                }),
                            )
                            .clicked()
                        {
                            self.tab = tab;
                            // Re-entering the library should show what is
                            // actually on the screens now, which anything else
                            // may have changed while another tab was open.
                            if tab == Tab::Library {
                                self.scanned = false;
                            }
                        }
                    }

                    // Which screen everything applies to, where both tabs can
                    // see it: the Workshop downloads onto it and the Library
                    // puts wallpapers on it, and it was only reachable from
                    // one of them.
                    let screens: Vec<String> = self
                        .library
                        .screens()
                        .iter()
                        .map(|screen| screen.name.clone())
                        .collect();
                    if !screens.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let chosen = self
                                .library
                                .target()
                                .map_or_else(|| "Screen".to_owned(), str::to_owned);
                            egui::ComboBox::from_id_salt("screen")
                                .selected_text(chosen)
                                .show_ui(ui, |ui| {
                                    for screen in screens {
                                        let picked = self.library.target() == Some(screen.as_str());
                                        if ui.selectable_label(picked, &screen).clicked() {
                                            self.library.set_target(screen);
                                        }
                                    }
                                });
                            ui.label(RichText::new("Applying to").small().color(theme::MUTED));
                        });
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tab_has_a_name() {
        for tab in [Tab::Workshop, Tab::Library, Tab::Settings] {
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn every_listed_tab_name_parses() {
        for name in Tab::NAMES {
            assert!(Tab::parse(name).is_some(), "{name} is listed but unknown");
        }
        assert_eq!(Tab::parse("nonsense"), None);
    }
}
