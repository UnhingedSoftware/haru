use egui::{Align, Color32, Layout, RichText};

use crate::theme;

pub struct Account {
    open: bool,
    who: Option<String>,
    client: bool,
    code: Option<(String, Option<egui::TextureHandle>)>,
    waiting: bool,
    status: String,
}

impl Default for Account {
    fn default() -> Self {
        Self::new()
    }
}

impl Account {
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: false,
            who: None,
            client: false,
            code: None,
            waiting: false,
            status: String::new(),
        }
    }

    pub fn observed(&mut self, saved: Option<String>, client: bool) {
        let knew_nothing = self.who.is_none() && !self.client;
        self.who = saved;
        self.client = client;
        if knew_nothing && self.who.is_none() && !self.client {
            self.open = true;
        }
        if self.who.is_some() {
            self.open = false;
            self.waiting = false;
        }
    }

    pub fn show_code(&mut self, url: String) {
        self.code = Some((url, None));
        self.waiting = true;
        self.open = true;
    }

    pub fn signed_in(&mut self, account: String) {
        self.who = Some(account);
        self.code = None;
        self.waiting = false;
        self.open = false;
    }

    pub fn signed_out(&mut self) {
        self.who = None;
        self.code = None;
        self.waiting = false;
        self.status.clear();
        self.open = !self.client;
    }

    pub fn failed(&mut self, why: String) {
        self.waiting = false;
        self.code = None;
        self.status = why;
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    #[must_use]
    pub const fn waiting(&self) -> bool {
        self.waiting
    }

    #[must_use]
    pub const fn can_download(&self) -> bool {
        self.who.is_some() || self.client
    }

    #[must_use]
    pub fn who(&self) -> Option<&str> {
        self.who.as_deref()
    }

    #[must_use]
    pub const fn has_client(&self) -> bool {
        self.client
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> bool {
        if !self.open {
            return false;
        }

        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("account-shade"))
            .order(egui::Order::Background)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(180));
            });

        let mut asked = false;
        let mut close = false;

        egui::Window::new("sign in")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(380.0)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(theme::MODAL)
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.horizontal(|ui| {
                    ui.heading("Sign in to download");
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        if crate::icons::button(ui, crate::icons::Icon::Close, false).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Browsing needs nothing. Downloading needs an account that owns \
                         Wallpaper Engine — either way below works.",
                    )
                    .small()
                    .color(theme::MUTED),
                );

                ui.add_space(14.0);

                asked |= self.routes(ui);

                if self.waiting && self.code.is_none() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("asking Steam…").small().color(theme::MUTED));
                    });
                }

                if !self.status.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(&self.status).small().color(theme::DANGER));
                }

                ui.add_space(12.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::new(RichText::new("Browse without signing in").small()),
                    )
                    .clicked()
                {
                    close = true;
                }
            });

        if close {
            self.open = false;
            self.code = None;
        }
        asked
    }
}

impl Account {
    fn routes(&mut self, ui: &mut egui::Ui) -> bool {
        let mut asked = false;
        match self.code.as_mut() {
            Some((url, texture)) => {
                ui.vertical_centered(|ui| {
                    let picture = texture.get_or_insert_with(|| {
                        ui.ctx()
                            .load_texture("qr", render(url), egui::TextureOptions::NEAREST)
                    });
                    ui.add(egui::Image::new(&*picture).fit_to_exact_size(egui::vec2(220.0, 220.0)));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Scan it with the Steam mobile app")
                            .small()
                            .color(theme::MUTED),
                    );
                    ui.add_space(4.0);
                    ui.hyperlink_to(RichText::new("or open the link").small(), url.clone());
                });
            }
            None => {
                if ui
                    .add_sized(
                        [ui.available_width(), 34.0],
                        egui::Button::new("Sign in with a QR code"),
                    )
                    .clicked()
                {
                    asked = true;
                    self.waiting = true;
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new("No password is typed; approve it on your phone.")
                        .small()
                        .color(theme::MUTED),
                );

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(10.0);

                if self.client {
                    ui.label(
                        RichText::new("Steam is running — downloads can go through it.")
                            .color(theme::ACCENT),
                    );
                } else {
                    ui.label(RichText::new("Or use the Steam client").strong());
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "With Steam open and signed in, haru can subscribe through \
                     it and skip signing in here.",
                        )
                        .small()
                        .color(theme::MUTED),
                    );
                    ui.add_space(6.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), 30.0],
                            egui::Button::new("Open Steam"),
                        )
                        .clicked()
                    {
                        let _ = std::process::Command::new("xdg-open")
                            .arg("steam://open/main")
                            .spawn();
                    }
                }
            }
        }
        asked
    }
}

fn render(url: &str) -> egui::ColorImage {
    const QUIET: usize = 4;

    let Ok(code) = qrcode::QrCode::new(url.as_bytes()) else {
        return egui::ColorImage::new([1, 1], Color32::BLACK);
    };

    let modules = code.to_colors();
    let side = code.width();
    let full = side + QUIET * 2;
    let mut pixels = vec![Color32::WHITE; full * full];

    for (index, module) in modules.iter().enumerate() {
        if *module != qrcode::Color::Dark {
            continue;
        }
        let (x, y) = (index % side + QUIET, index / side + QUIET);
        if let Some(pixel) = pixels.get_mut(y * full + x) {
            *pixel = Color32::BLACK;
        }
    }

    egui::ColorImage {
        size: [full, full],
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_square_and_has_its_quiet_border() {
        let image = render("https://s.team/q/1/2");
        assert_eq!(image.size[0], image.size[1]);
        assert_eq!(image.pixels.first(), Some(&Color32::WHITE));
        assert!(image.pixels.contains(&Color32::BLACK));
    }

    #[test]
    fn it_asks_only_when_there_is_no_way_to_download() {
        let mut account = Account::new();
        account.observed(None, true);
        assert!(!account.open, "a running client is a way to download");

        let mut account = Account::new();
        account.observed(Some("someone".to_owned()), false);
        assert!(!account.open, "a saved token is a way to download");

        let mut account = Account::new();
        account.observed(None, false);
        assert!(account.open, "neither: worth asking");
    }

    #[test]
    fn signing_in_closes_it_and_is_remembered() {
        let mut account = Account::new();
        account.observed(None, false);
        account.show_code("https://s.team/q/1".to_owned());
        assert!(account.open);

        account.signed_in("someone".to_owned());
        assert!(!account.open);
        assert_eq!(account.who(), Some("someone"));
        assert!(account.can_download());
    }

    #[test]
    fn a_rotated_code_replaces_the_picture_of_the_old_one() {
        let mut account = Account::new();
        account.show_code("first".to_owned());
        account.show_code("second".to_owned());
        let held = account.code.as_ref();
        assert_eq!(held.map(|(url, _)| url.as_str()), Some("second"));
        assert!(
            held.is_some_and(|(_, texture)| texture.is_none()),
            "the old picture must not be kept"
        );
    }
}
