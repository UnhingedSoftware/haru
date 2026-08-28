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

use haru_workshop::{Reply, Request};

use crate::{Account, Browser, Library, Preview, Settings, theme};

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
    /// The sign-in overlay, and what it knows.
    account: Account,
    /// The offer to install a renderer, on a machine with none.
    installer: crate::renderer::Installer,
    /// A renderer being started, until it answers or gives up.
    ///
    /// On a thread because starting one means waiting for it to build the
    /// first scene, which is seconds, and the window keeps drawing meanwhile.
    starting: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    /// The connection everything Steam-shaped goes over.
    workshop: Rc<Workshop>,
    /// The request that asks who is signed in, while it is outstanding.
    who_request: Option<haru_workshop::RequestId>,
    /// The sign-in in flight, so its answers come here and not elsewhere.
    sign_in_request: Option<haru_workshop::RequestId>,
    /// The sign-out in flight.
    sign_out_request: Option<haru_workshop::RequestId>,
    /// Whether the account has been asked about yet.
    asked_who: bool,
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

        // Offered on a machine that has never had a renderer, and only where
        // one is published: browsing works without kirie, applying does not,
        // and a picker that can never put anything up should say so on the way
        // in rather than when the first wallpaper is clicked.
        let mut installer = crate::renderer::Installer::new();
        if config.offer_renderer
            && haru_apply::install::supported()
            && haru_apply::install::installed().is_none()
        {
            installer.offer();
        }

        Self {
            tab,
            previews: Previews::new(),
            browser,
            library: Library::new(Rc::clone(&workshop)),
            preview,
            settings,
            config,
            backend,
            scanned: false,
            sidebar: true,
            account: Account::new(),
            installer,
            starting: None,
            workshop,
            asked_who: false,
            who_request: None,
            sign_in_request: None,
            sign_out_request: None,
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

        // Asked once, on the first frame: the answer decides whether the
        // overlay has anything to interrupt for.
        if !self.asked_who {
            self.who_request = Some(self.workshop.send(Request::WhoAmI));
            self.asked_who = true;
        }
        self.collect_account();

        // egui draws on events, and an answer arriving on a channel is not
        // one. Without this the window sleeps with a reply sitting unread —
        // which is exactly what happened on a static tab: the account state
        // was answered in milliseconds and collected never.
        if self.who_request.is_some()
            || self.sign_in_request.is_some()
            || self.sign_out_request.is_some()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
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
                let asked = self.settings.ui(
                    ctx,
                    &mut self.config,
                    self.backend.as_deref(),
                    self.account.who(),
                    self.account.has_client(),
                );
                if asked.sign_in {
                    self.account.open();
                }
                if asked.install {
                    self.installer.offer();
                }
                if asked.sign_out {
                    self.sign_out_request = Some(self.workshop.send(Request::SignOut));
                }
                if asked.changed {
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

        // A wallpaper picked while nothing was rendering is a request to start
        // something, not a failure.
        if let Some((screen, dir)) = self.library.take_pending() {
            self.start_renderer(ctx, &screen, &dir);
        }
        self.collect_renderer(ctx);

        // Last, so they sit over whatever was drawn. One at a time: two
        // modals over each other is a window nobody can answer.
        match self.installer.ui(ctx) {
            crate::renderer::Outcome::Installed(web) => {
                self.config.renderer_web = Some(web.key().to_owned());
                let _ = self.config.save();
                // It is on disk now; whether it is *running* is another
                // question, and the answer is the one the backend probes for.
                self.backend = haru_apply::detect(self.config.socket.clone());
            }
            crate::renderer::Outcome::Dismissed => {
                self.config.offer_renderer = false;
                let _ = self.config.save();
            }
            crate::renderer::Outcome::Nothing => {}
        }
        if !self.installer.is_open() && self.account.ui(ctx) {
            self.sign_in_request = Some(self.workshop.send(Request::SignIn));
        }

        self.finish_frame();
    }

    /// Starts an engine on this wallpaper, if there is one to start.
    ///
    /// One engine serves every screen, so an engine somebody else started is
    /// left alone: a second would fight the first for the same socket.
    fn start_renderer(&mut self, ctx: &egui::Context, screen: &str, dir: &std::path::Path) {
        if self.starting.is_some() {
            return;
        }

        let Some(binary) = haru_apply::install::installed() else {
            self.library.say("no renderer installed yet");
            self.installer.offer();
            return;
        };
        let socket = haru_apply::Kirie::new(self.config.socket.clone())
            .socket()
            .to_path_buf();
        if haru_apply::launch::running() {
            self.library.say(format!(
                "a renderer is running but not answering on {}",
                socket.display()
            ));
            return;
        }

        let (answer, heard) = std::sync::mpsc::channel();
        let (screen, dir) = (screen.to_owned(), dir.to_owned());
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("haru-start-renderer".to_owned())
            .spawn(move || {
                let _ = answer.send(haru_apply::launch::start(&binary, &socket, &screen, &dir));
                ctx.request_repaint();
            });

        if spawned.is_ok() {
            self.starting = Some(heard);
            self.library.say("starting the renderer\u{2026}");
        } else {
            self.library.say("could not start the renderer");
        }
    }

    /// Takes the answer from a renderer that was being started.
    fn collect_renderer(&mut self, ctx: &egui::Context) {
        let Some(heard) = self.starting.as_ref() else {
            return;
        };
        match heard.try_recv() {
            Ok(Ok(())) => {
                self.starting = None;
                // It came up on the wallpaper it was started for, so there is
                // nothing left to apply — only a backend to notice.
                self.backend = haru_apply::detect(self.config.socket.clone());
                self.library.refresh(&self.config, self.backend.as_deref());
                self.library.say("the renderer is up");
            }
            Ok(Err(why)) => {
                self.starting = None;
                self.library.say(why);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.starting = None;
                self.library.say("the renderer stopped without answering");
            }
            // Still coming up, and egui only draws when something happens.
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
    }

    /// Takes the connection's answers about the account.
    ///
    /// Everything else on that channel belongs to the tab that asked for it,
    /// and is left alone.
    fn collect_account(&mut self) {
        if let Some(id) = self.who_request
            && let Some(reply) = self.workshop.take(id)
        {
            self.who_request = None;
            match reply {
                Reply::Account { saved, client } => {
                    self.browser.set_client(client);
                    self.account.observed(saved, client);
                }
                // Nothing to sign in with and no client to ask is exactly the
                // state worth interrupting for.
                _ => self.account.observed(None, false),
            }
        }

        if let Some(id) = self.sign_out_request
            && let Some(reply) = self.workshop.take(id)
        {
            self.sign_out_request = None;
            match reply {
                Reply::SignedOut => self.account.signed_out(),
                Reply::Failed(why) => self.account.failed(why),
                _ => {}
            }
        }

        // A sign-in answers more than once: Steam rotates the code, so each
        // one arrives under the same request until the login resolves.
        while let Some(id) = self.sign_in_request
            && let Some(reply) = self.workshop.take(id)
        {
            match reply {
                Reply::QrCode(url) => self.account.show_code(url),
                Reply::SignedIn(who) => {
                    self.sign_in_request = None;
                    self.account.signed_in(who);
                }
                Reply::Failed(why) => {
                    self.sign_in_request = None;
                    self.account.failed(why);
                }
                _ => {}
            }
        }
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
                    if crate::icons::button(ui, crate::icons::Icon::Menu, self.sidebar)
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

                    for tab in [Tab::Library, Tab::Workshop, Tab::Preview, Tab::Settings] {
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
                    // see it. One picker rather than a row of cards in one tab:
                    // it is the same choice from either, and it was previously
                    // reachable from only one of them.
                    let screens = self.library.screens().to_vec();
                    if !screens.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let chosen = self
                                .library
                                .target()
                                .map_or_else(|| "Screen".to_owned(), str::to_owned);
                            egui::ComboBox::from_id_salt("screen")
                                .selected_text(chosen)
                                .width(160.0)
                                .show_ui(ui, |ui| {
                                    for screen in screens {
                                        let picked =
                                            self.library.target() == Some(screen.name.as_str());
                                        // The name first, then what is on it —
                                        // two screens showing wallpapers are
                                        // told apart by the wallpaper, and by
                                        // the name when both are empty.
                                        let showing = screen
                                            .current
                                            .as_ref()
                                            .and_then(|dir| self.library.title_of(dir))
                                            .unwrap_or_else(|| "nothing".to_owned());
                                        let response = ui.selectable_label(
                                            picked,
                                            format!("{}  ·  {showing}", screen.name),
                                        );
                                        if response.clicked() {
                                            self.library.set_target(screen.name.clone());
                                        }
                                    }
                                });
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
