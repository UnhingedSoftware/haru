use std::process::ExitCode;

use haru_ui::{Haru, Tab};

const INITIAL: [f32; 2] = [1280.0, 820.0];

struct Opened {
    tab: Tab,
    search: Option<String>,
    item: Option<String>,
}

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
                opened.search = Some(
                    rest.next()
                        .ok_or("--search needs something to search for")?
                        .clone(),
                );
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

struct App {
    haru: Haru,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.haru.ui(ctx);
    }

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
