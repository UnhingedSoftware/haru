use egui::RichText;
use haru_apply::Backend;
use haru_core::{Config, Filters};
use haru_media::Previews;

use haru_workshop::Workshop;
use std::rc::Rc;

use haru_workshop::{Reply, Request};

use crate::{Account, Browser, Library, Preview, Settings, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Workshop,
    Library,
    Preview,
    Settings,
}

impl Tab {
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

    pub const NAMES: [&'static str; 4] = ["workshop", "library", "preview", "settings"];

    const fn label(self) -> &'static str {
        match self {
            Self::Workshop => "Workshop",
            Self::Library => "Library",
            Self::Preview => "Preview",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Job {
    Start,
    Stop,
}

pub struct Haru {
    tab: Tab,
    previews: Previews,
    browser: Browser,
    library: Library,
    preview: Preview,
    settings: Settings,
    config: Config,
    backend: Option<Box<dyn Backend>>,
    scanned: bool,
    account: Account,
    installer: crate::renderer::Installer,
    starting: Option<(Job, std::sync::mpsc::Receiver<Result<(), String>>)>,
    workshop: Rc<Workshop>,
    who_request: Option<haru_workshop::RequestId>,
    sign_in_request: Option<haru_workshop::RequestId>,
    sign_out_request: Option<haru_workshop::RequestId>,
    asked_who: bool,
    sidebar: bool,
}

impl Default for Haru {
    fn default() -> Self {
        Self::new()
    }
}

impl Haru {
    #[must_use]
    pub fn new() -> Self {
        Self::opening_on(Tab::Library, None)
    }

    #[must_use]
    pub fn opening_on(tab: Tab, search: Option<String>) -> Self {
        Self::opening_on_item(tab, search, None)
    }

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
        let workshop = Rc::new(Workshop::spawn());
        let mut browser = Browser::with_filters(filters, Rc::clone(&workshop));
        browser.reconfigure(
            config.adult,
            config.per_page,
            config.infinite_scroll,
            config.fit_per_page,
        );
        browser.set_install_root(config.install_root());
        let mut settings = Settings::default();
        settings.sync(&config);

        let mut preview = Preview::new();
        if let Some(wanted) = item {
            if let Some(found) = haru_core::library::scan(&config.libraries())
                .into_iter()
                .find(|installed| installed.id == wanted)
            {
                preview.open(found);
            }
        }

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

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.before_drawing(ctx);
        self.tabs(ctx);

        if self.tab != Tab::Preview {
            self.preview.suspend();
        }

        match self.tab {
            Tab::Workshop => self.workshop_tab(ctx),
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
            Tab::Settings => self.settings_tab(ctx),
        }

        if let Some((screen, dir)) = self.library.take_pending() {
            self.start_renderer(ctx, &screen, &dir);
        }
        if let Some((screen, dir)) = self.library.take_applied() {
            self.config.screens.insert(screen, dir);
            let _ = self.config.save();
        }
        self.collect_renderer(ctx);

        self.overlays(ctx);

        self.finish_frame();
    }

    fn screen_picker(&mut self, ui: &mut egui::Ui) {
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
                            let picked = self.library.target() == Some(screen.name.as_str());
                            let showing = screen
                                .current
                                .as_ref()
                                .and_then(|dir| self.library.title_of(dir))
                                .unwrap_or_else(|| "nothing".to_owned());
                            let response = ui
                                .selectable_label(picked, format!("{}  ·  {showing}", screen.name));
                            if response.clicked() {
                                self.library.set_target(screen.name.clone());
                            }
                        }
                    });
            });
        }
    }

    fn before_drawing(&mut self, ctx: &egui::Context) {
        if !self.scanned {
            self.library.refresh(&self.config, self.backend.as_deref());
            self.scanned = true;
        }

        if !self.asked_who {
            self.who_request = Some(self.workshop.send(Request::WhoAmI));
            self.asked_who = true;
        }
        self.collect_account();

        if self.who_request.is_some()
            || self.sign_in_request.is_some()
            || self.sign_out_request.is_some()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        match self.installer.ui(ctx) {
            crate::renderer::Outcome::Installed(web) => {
                self.config.renderer_web = Some(web.key().to_owned());
                let _ = self.config.save();
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
    }

    fn workshop_tab(&mut self, ctx: &egui::Context) {
        self.browser.ui(ctx, &mut self.previews, self.sidebar);
        if let Some(dir) = self.browser.take_landed() {
            self.library.refresh(&self.config, self.backend.as_deref());
            self.library.apply_to_target(&dir, self.backend.as_deref());
            self.scanned = true;
        }
    }

    fn settings_tab(&mut self, ctx: &egui::Context) {
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
        if let Some(what) = asked.renderer {
            self.manage_renderer(ctx, what);
        }
        if asked.sign_out {
            self.sign_out_request = Some(self.workshop.send(Request::SignOut));
        }
        if asked.changed {
            self.backend = haru_apply::detect(self.config.socket.clone());
            self.browser.reconfigure(
                self.config.adult,
                self.config.per_page,
                self.config.infinite_scroll,
                self.config.fit_per_page,
            );
            self.browser.set_install_root(self.config.install_root());
            self.scanned = false;
        }
    }

    fn plan_for(&self, screen: &str, dir: &std::path::Path) -> Vec<haru_apply::launch::Plan> {
        let mut names: Vec<String> = Vec::new();
        let mut showing: Vec<(String, std::path::PathBuf)> = Vec::new();

        if let Some(backend) = self.backend.as_deref()
            && let Ok(live) = backend.screens()
        {
            for found in live {
                names.push(found.name.clone());
                if let Some(current) = found.current {
                    showing.push((found.name, current));
                }
            }
        }
        for name in haru_apply::launch::connectors() {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        if !names.iter().any(|name| name == screen) {
            names.push(screen.to_owned());
        }

        names
            .into_iter()
            .map(|name| {
                if name == screen {
                    return haru_apply::launch::Plan::showing(name, dir);
                }
                let wallpaper = showing
                    .iter()
                    .find(|(had, _)| *had == name)
                    .map(|(_, wallpaper)| wallpaper.clone())
                    .or_else(|| self.config.screens.get(&name).cloned())
                    .filter(|wallpaper| wallpaper.is_dir());
                haru_apply::launch::Plan {
                    screen: name,
                    wallpaper,
                }
            })
            .collect()
    }

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

        let plan = self.plan_for(screen, dir);
        let replacing = haru_apply::launch::running();
        let (answer, heard) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("haru-start-renderer".to_owned())
            .spawn(move || {
                let outcome = if replacing {
                    haru_apply::launch::restart(&binary, &socket, &plan)
                } else {
                    haru_apply::launch::start(&binary, &socket, &plan)
                };
                let _ = answer.send(outcome);
                ctx.request_repaint();
            });

        if spawned.is_ok() {
            self.starting = Some((Job::Start, heard));
            self.config
                .screens
                .insert(screen.to_owned(), dir.to_owned());
            let _ = self.config.save();
            self.library.say(if replacing {
                "restarting the renderer for that screen\u{2026}"
            } else {
                "starting the renderer\u{2026}"
            });
        } else {
            self.library.say("could not start the renderer");
        }
    }

    fn manage_renderer(&mut self, ctx: &egui::Context, asked: crate::settings::Renderer) {
        use crate::settings::Renderer;

        match asked {
            Renderer::Start | Renderer::Restart => {
                let last = self
                    .config
                    .screens
                    .iter()
                    .find(|(_, wallpaper)| wallpaper.is_dir())
                    .map(|(screen, wallpaper)| (screen.clone(), wallpaper.clone()));
                match last {
                    Some((screen, wallpaper)) => self.start_renderer(ctx, &screen, &wallpaper),
                    None => self
                        .library
                        .say("pick a wallpaper — an engine cannot start without one"),
                }
            }
            Renderer::Stop => self.stop_renderer(ctx),
        }
    }

    fn stop_renderer(&mut self, ctx: &egui::Context) {
        if self.starting.is_some() {
            return;
        }
        let (answer, heard) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("haru-stop-renderer".to_owned())
            .spawn(move || {
                let _ = answer.send(haru_apply::launch::stop());
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.starting = Some((Job::Stop, heard));
            self.library.say("stopping the renderer\u{2026}");
        }
    }

    fn collect_renderer(&mut self, ctx: &egui::Context) {
        let Some((job, heard)) = self.starting.as_ref() else {
            return;
        };
        let job = *job;
        match heard.try_recv() {
            Ok(Ok(())) => {
                self.starting = None;
                self.backend = haru_apply::detect(self.config.socket.clone());
                self.library.refresh(&self.config, self.backend.as_deref());
                self.library.say(match job {
                    Job::Start => "the renderer is up",
                    Job::Stop => "the renderer is stopped",
                });
            }
            Ok(Err(why)) => {
                self.starting = None;
                self.library.say(why);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.starting = None;
                self.library.say("the renderer stopped without answering");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
    }

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

    fn finish_frame(&mut self) {
        self.previews.sweep();
    }

    fn tabs(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs")
            .frame(theme::panel_frame(theme::Side::Left))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
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
                            if tab == Tab::Library {
                                self.scanned = false;
                            }
                        }
                    }

                    self.screen_picker(ui);
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
