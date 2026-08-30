use egui::RichText;
use haru_apply::Engine;
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

pub struct Haru {
    tab: Tab,
    previews: Previews,
    browser: Browser,
    library: Library,
    preview: Preview,
    settings: Settings,
    config: Config,
    engine: Engine,
    scanned: bool,
    account: Account,
    installer: crate::renderer::Installer,
    updates: crate::updates::Updates,
    workshop: Rc<Workshop>,
    who_request: Option<haru_workshop::RequestId>,
    sign_in_request: Option<haru_workshop::RequestId>,
    sign_out_request: Option<haru_workshop::RequestId>,
    assets_request: Option<haru_workshop::RequestId>,
    assets_note: String,
    assets_tried: bool,
    assets_wait_account: bool,
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
        let engine = Engine::spawn(config.socket.clone());

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
            engine,
            scanned: false,
            sidebar: true,
            account: Account::new(),
            installer,
            updates: crate::updates::Updates::default(),
            workshop,
            asked_who: false,
            who_request: None,
            sign_in_request: None,
            sign_out_request: None,
            assets_request: None,
            assets_note: String::new(),
            assets_tried: false,
            assets_wait_account: false,
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
                    &self.engine,
                    self.sidebar,
                );
            }
            Tab::Preview => self.preview.ui(ctx, self.sidebar),
            Tab::Settings => self.settings_tab(ctx),
        }

        if let Some(item) = self.library.take_preview() {
            self.preview.open(item);
            self.tab = Tab::Preview;
        }
        if let Some((screen, dir)) = self.library.take_pending() {
            self.start_renderer(&screen, &dir);
        }
        if self.browser.take_needs_account() {
            self.account.open();
        }
        if let Some((screen, dir)) = self.library.take_applied() {
            self.config.screens.insert(screen, dir);
            let _ = self.config.save();
            self.refresh_startup();
        }
        self.updates.tick(self.config.auto_update);
        self.engine_notes(ctx);

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
            self.library.refresh(&self.config, &self.engine);
            self.scanned = true;
        }

        if !self.asked_who {
            self.who_request = Some(self.workshop.send(Request::WhoAmI));
            self.asked_who = true;
        }
        self.collect_account();

        self.fetch_assets_if_missing();
        self.collect_assets();

        if self.who_request.is_some()
            || self.sign_in_request.is_some()
            || self.sign_out_request.is_some()
            || self.assets_request.is_some()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn fetch_assets_if_missing(&mut self) {
        if self.assets_wait_account && self.account.who().is_some() {
            self.assets_wait_account = false;
            self.assets_tried = false;
        }
        if self.assets_tried
            || self.assets_request.is_some()
            || haru_core::engine::found().is_some()
        {
            return;
        }
        self.assets_tried = true;
        let Some(into) = haru_core::engine::install_root() else {
            self.assets_note = "no data directory to install into".to_owned();
            return;
        };
        self.assets_note = "fetching Wallpaper Engine's assets…".to_owned();
        self.assets_request = Some(self.workshop.send(Request::EngineAssets { into }));
    }

    fn collect_assets(&mut self) {
        let Some(id) = self.assets_request else {
            return;
        };
        let Some(reply) = self.workshop.take(id) else {
            return;
        };
        match reply {
            haru_workshop::Reply::Progress { done, total, .. } => {
                let share = done.saturating_mul(100).checked_div(total).unwrap_or(0);
                self.assets_note = format!("fetching Wallpaper Engine's assets… {share}%");
                return;
            }
            haru_workshop::Reply::EngineAssets { dir } => {
                self.assets_note = format!("engine assets ready: {}", dir.display());
            }
            haru_workshop::Reply::NeedsAccount => {
                self.assets_note = "sign in with an account that owns Wallpaper Engine".to_owned();
                self.assets_wait_account = true;
                self.account.open();
            }
            haru_workshop::Reply::Failed(why) => self.assets_note = why,
            _ => return,
        }
        self.assets_request = None;
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        match self.installer.ui(ctx) {
            crate::renderer::Outcome::Installed(web) => {
                self.config.renderer_web = Some(web.key().to_owned());
                let _ = self.config.save();
                self.engine = Engine::spawn(self.config.socket.clone());
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
        self.browser
            .ui(ctx, &mut self.previews, self.sidebar, &self.engine);
        if let Some(dir) = self.browser.take_landed() {
            self.library.refresh(&self.config, &self.engine);
            self.library.apply_to_target(&dir, &self.engine);
            self.scanned = true;
        }
    }

    fn settings_tab(&mut self, ctx: &egui::Context) {
        let asked = self.settings.ui(
            ctx,
            &mut self.config,
            &self.engine,
            self.account.who(),
            self.account.has_client(),
            &self.assets_note,
            self.updates.note(),
        );
        if asked.sign_in {
            self.account.open();
        }
        if asked.install {
            self.installer.offer();
        }
        if asked.fetch_assets && self.assets_request.is_none() {
            self.assets_tried = true;
            match haru_core::engine::install_root() {
                Some(into) => {
                    self.assets_note = "fetching Wallpaper Engine's assets…".to_owned();
                    self.assets_request = Some(self.workshop.send(Request::EngineAssets { into }));
                }
                None => self.assets_note = "no data directory to install into".to_owned(),
            }
        }
        if let Some(what) = asked.renderer {
            self.manage_renderer(what);
        }
        if asked.sign_out {
            self.sign_out_request = Some(self.workshop.send(Request::SignOut));
        }
        if let Some(on) = asked.register {
            self.settings.note(if on {
                match haru_apply::desktop::install() {
                    Ok(path) => Some(format!("added {}", path.display())),
                    Err(why) => Some(why),
                }
            } else {
                haru_apply::desktop::uninstall()
                    .err()
                    .or_else(|| Some("removed".to_owned()))
            });
        }
        if let Some(on) = asked.startup {
            self.settings.note(if on {
                self.register_startup()
            } else {
                haru_apply::startup::disable().err()
            });
        }
        if asked.tune {
            let _ = self.config.save();
            self.engine.tune(self.config.renderer.live_commands());
            self.refresh_startup();
        }
        if asked.relaunch && haru_apply::launch::running() {
            let plan = self.current_plan();
            if !plan.is_empty() {
                self.engine.restart(plan);
            }
        }
        if asked.changed {
            self.engine = Engine::spawn(self.config.socket.clone());
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

    fn refresh_startup(&self) {
        if haru_apply::startup::enabled() {
            let _ = self.register_startup();
        }
    }

    fn register_startup(&self) -> Option<String> {
        let binary = haru_apply::install::installed()?;
        let socket = self
            .config
            .socket
            .clone()
            .unwrap_or_else(haru_apply::default_socket);
        let plan = self.current_plan();
        if plan.is_empty() {
            return Some("put a wallpaper up first, then it can be restored at login".to_owned());
        }

        let mut command = vec![binary.to_string_lossy().into_owned()];
        command.extend(haru_apply::launch::arguments_for(&socket, &plan));
        let environment: Vec<(String, String)> = haru_apply::renderer_env()
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_string_lossy().into_owned()))
            .collect();

        haru_apply::startup::enable(&command, &environment).err()
    }

    fn current_plan(&self) -> Vec<haru_apply::launch::Plan> {
        let seen = self.engine.snapshot();
        if !cfg!(target_os = "linux") {
            return seen
                .screens
                .into_iter()
                .filter_map(|found| {
                    let wallpaper = found.current?;
                    wallpaper
                        .is_dir()
                        .then(|| haru_apply::launch::Plan::showing(found.name, wallpaper))
                })
                .take(1)
                .collect();
        }
        seen.screens
            .into_iter()
            .filter_map(|found| {
                let wallpaper = found
                    .current
                    .or_else(|| self.config.screens.get(&found.name).cloned())?;
                wallpaper
                    .is_dir()
                    .then(|| haru_apply::launch::Plan::showing(found.name, wallpaper))
            })
            .collect()
    }

    fn plan_for(&self, screen: &str, dir: &std::path::Path) -> Vec<haru_apply::launch::Plan> {
        if !cfg!(target_os = "linux") {
            return vec![haru_apply::launch::Plan::showing(screen, dir)];
        }
        let mut names: Vec<String> = Vec::new();
        let mut showing: Vec<(String, std::path::PathBuf)> = Vec::new();

        let seen = self.engine.snapshot();
        for found in seen.screens {
            names.push(found.name.clone());
            if let Some(current) = found.current {
                showing.push((found.name, current));
            }
        }
        for name in seen.connectors {
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

    fn start_renderer(&mut self, screen: &str, dir: &std::path::Path) {
        if self.engine.snapshot().binary.is_none() {
            self.library.say("no renderer installed yet");
            self.installer.offer();
            return;
        }

        let replacing = self.engine.snapshot().pid.is_some();
        self.engine.start(self.plan_for(screen, dir));
        self.config
            .screens
            .insert(screen.to_owned(), dir.to_owned());
        let _ = self.config.save();
        self.library.say(if replacing {
            "restarting the renderer for that screen\u{2026}"
        } else {
            "starting the renderer\u{2026}"
        });
    }

    fn manage_renderer(&mut self, asked: crate::settings::Renderer) {
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
                    Some((screen, wallpaper)) => self.start_renderer(&screen, &wallpaper),
                    None => self
                        .library
                        .say("pick a wallpaper — an engine cannot start without one"),
                }
            }
            Renderer::Stop => {
                self.engine.stop();
                self.library.say("stopping the renderer\u{2026}");
            }
        }
    }

    fn engine_notes(&mut self, ctx: &egui::Context) {
        while let Some(note) = self.engine.take_note() {
            match note {
                Ok(said) => self.library.say(said),
                Err(why) => self.library.say(why),
            }
        }
        if self.engine.snapshot().working {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
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
                    ui.label(
                        RichText::new(env!("CARGO_PKG_VERSION"))
                            .small()
                            .color(crate::theme::MUTED),
                    )
                    .on_hover_text(
                        "Settings lists this, the renderer's version and where both live.",
                    );
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
