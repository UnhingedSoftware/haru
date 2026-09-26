// haru is a window, not a command: a release build on Windows should not drag a
// console along behind it. Debug builds keep theirs, so `eprintln!` still lands
// somewhere while developing.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::process::ExitCode;

use haru_ui::{Haru, Tab};

// A 1080p laptop at Windows' default 150% scaling has 1280x720 logical pixels,
// less the taskbar, so 820 tall put the paging bar and the bottom of the grid
// off the screen before the user had touched anything.
const INITIAL: [f32; 2] = if cfg!(windows) {
    [1120.0, 660.0]
} else {
    [1280.0, 820.0]
};

// Only a compositor that blends the window's alpha makes the translucent
// backdrop worth asking for. On Windows the DX12 and Vulkan swapchains only
// offer an opaque surface, so winit's blur-behind is all the request buys, and
// that is what left black or white flashes and garbage along the edges while
// resizing.
const TRANSPARENT: bool = !cfg!(windows);

const APP_ID: &str = "haru";

const ICON: &[u8] = include_bytes!("../../../packaging/haru-256.png");

fn icon() -> Option<egui::IconData> {
    let decoded = image::load_from_memory(ICON).ok()?.into_rgba8();
    let (width, height) = decoded.dimensions();
    Some(egui::IconData {
        rgba: decoded.into_raw(),
        width,
        height,
    })
}

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

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(INITIAL)
        .with_min_inner_size([720.0, 480.0])
        .with_title("haru")
        .with_app_id(APP_ID)
        .with_transparent(TRANSPARENT);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            present_mode: present_mode(),
            desired_maximum_frame_latency: Some(2),
            supported_backends: backends(),
            ..eframe::egui_wgpu::WgpuConfiguration::default()
        },
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

// Without vsync, a Windows swapchain presents immediately and tears whenever
// the window goes full-screen-ish and DWM hands it the flip. Wayland and macOS
// compositors sync regardless, so they keep the lower latency.
fn present_mode() -> eframe::wgpu::PresentMode {
    if cfg!(windows) {
        eframe::wgpu::PresentMode::AutoVsync
    } else {
        eframe::wgpu::PresentMode::AutoNoVsync
    }
}

// Left to itself, wgpu takes the first adapter that answers among Vulkan, DX12
// and WGL, so which renderer haru got depended on the driver, and the Vulkan
// and GL paths are where egui draws wrong on Windows. DX12 is on every Windows
// 10 machine haru supports. `WGPU_BACKEND` still overrides this for testing.
fn backends() -> eframe::wgpu::Backends {
    if cfg!(windows) {
        eframe::wgpu::util::backend_bits_from_env().unwrap_or(eframe::wgpu::Backends::DX12)
    } else {
        eframe::egui_wgpu::WgpuConfiguration::default().supported_backends
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
            if TRANSPARENT {
                f32::from(backdrop.a()) / 255.0
            } else {
                1.0
            },
        ]
    }
}
