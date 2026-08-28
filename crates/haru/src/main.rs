//! The `haru` binary: a window onto the Workshop.

use std::process::ExitCode;

use haru_ui::{Haru, Tab};

/// What the window opens at.
///
/// Wide enough for the sidebar, four tiles and a detail pane at once, which is
/// the layout the grid is sized around.
const INITIAL: [f32; 2] = [1280.0, 820.0];

/// What the command line asked for.
struct Opened {
    tab: Tab,
    search: Option<String>,
    /// A Workshop id to open the preview on.
    item: Option<String>,
}

/// Reads `haru [TAB] [--search TEXT] [--item ID]`.
///
/// Deliberately tiny. A picker is opened by a launcher or a shortcut, and the
/// only things either wants to say are which tab and what to search for.
fn parse(arguments: &[String]) -> Result<Opened, String> {
    let mut opened = Opened {
        tab: Tab::Library,
        search: None,
        item: None,
    };
    let mut rest = arguments.iter();

    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--item" => {
                opened.item = Some(rest.next().ok_or("--item needs a Workshop id")?.clone());
                opened.tab = Tab::Preview;
            }
            "--search" => {
                opened.search = Some(rest.next().ok_or("--search needs something to search for")?.clone());
                opened.tab = Tab::Workshop;
            }
            name => {
                opened.tab = Tab::parse(name).ok_or_else(|| {
                    format!("unknown argument {name:?}; try: {}", Tab::NAMES.join(", "))
                })?;
            }
        }
    }
    Ok(opened)
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let opened = match parse(&arguments) {
        Ok(opened) => opened,
        Err(message) => {
            eprintln!("haru: {message}");
            return ExitCode::FAILURE;
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(INITIAL)
            .with_min_inner_size([720.0, 480.0])
            .with_title("haru")
            // The window is see-through, so the wallpaper being chosen stays
            // partly in view behind the picker. A compositor with blur turns
            // the same alpha into frosted glass; one without shows a dark
            // panel, and nothing here depends on which.
            .with_transparent(true),
        ..eframe::NativeOptions::default()
    };

    match eframe::run_native(
        "haru",
        options,
        Box::new(move |cc| {
            haru_ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(App {
                haru: Haru::opening_on_item(opened.tab, opened.search, opened.item),
            }))
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("haru: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The application.
///
/// A fourth tab — the studio — slots in beside the three without any of them
/// knowing, which is why the window owns a mode rather than being one.
struct App {
    haru: Haru,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.haru.ui(ctx);
    }

    /// What the window clears to before anything is drawn.
    ///
    /// Translucent, which is what makes the desktop behind it visible; eframe
    /// takes this rather than the theme's own backdrop colour.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let backdrop = haru_ui::theme::BACKDROP;
        [
            f32::from(backdrop.r()) / 255.0,
            f32::from(backdrop.g()) / 255.0,
            f32::from(backdrop.b()) / 255.0,
            f32::from(backdrop.a()) / 255.0,
        ]
    }
}
