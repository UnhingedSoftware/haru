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

use crate::{Browser, Library, Settings, theme};

/// Which view is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Browsing the Workshop.
    Workshop,
    /// The wallpapers already installed, and what is on each screen.
    Library,
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
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    /// Every name [`Tab::parse`] accepts, canonical form first.
    pub const NAMES: [&'static str; 3] = ["workshop", "library", "settings"];

    /// What the tab is called.
    const fn label(self) -> &'static str {
        match self {
            Self::Workshop => "Workshop",
            Self::Library => "Library",
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
        let config = Config::load();
        let backend = haru_apply::detect(config.socket.clone());

        let filters = Filters {
            adult: config.adult,
            per_page: config.per_page,
            text: search.unwrap_or_default(),
            ..Filters::new()
        };
        let mut settings = Settings::default();
        settings.sync(&config);

        Self {
            tab,
            previews: Previews::new(),
            browser: Browser::with_filters(filters),
            library: Library::new(),
            settings,
            config,
            backend,
            scanned: false,
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

        match self.tab {
            Tab::Workshop => self.browser.ui(ctx, &mut self.previews),
            Tab::Library => self.library.ui(
                ctx,
                &mut self.previews,
                &self.config,
                self.backend.as_deref(),
            ),
            Tab::Settings => {
                if self
                    .settings
                    .ui(ctx, &mut self.config, self.backend.as_deref())
                {
                    // A changed socket means a different renderer, and a
                    // changed library means a different set of wallpapers.
                    self.backend = haru_apply::detect(self.config.socket.clone());
                    self.browser
                        .reconfigure(self.config.adult, self.config.per_page);
                    self.scanned = false;
                }
            }
        }
    }

    /// The row of tabs along the top.
    fn tabs(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs")
            .frame(theme::panel_frame(theme::Side::Left))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("haru").size(17.0).strong());
                    ui.add_space(14.0);

                    for tab in [Tab::Workshop, Tab::Library, Tab::Settings] {
                        let chosen = self.tab == tab;
                        if ui
                            .selectable_label(
                                chosen,
                                RichText::new(tab.label())
                                    .size(13.0)
                                    .color(if chosen { theme::TEXT } else { theme::MUTED }),
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
