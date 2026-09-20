use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scaling {
    Default,
    Fit,
    #[default]
    Fill,
    Stretch,
}

impl Scaling {
    pub const ALL: [Self; 4] = [Self::Default, Self::Fit, Self::Fill, Self::Stretch];

    #[must_use]
    pub const fn flag(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Fit => "fit",
            Self::Fill => "fill",
            Self::Stretch => "stretch",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "As the wallpaper asks",
            Self::Fit => "Fit — whole image, bars if needed",
            Self::Fill => "Fill — cover the screen, crop the rest",
            Self::Stretch => "Stretch — distort to fit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Clamp {
    #[default]
    Clamp,
    Border,
    Repeat,
}

impl Clamp {
    pub const ALL: [Self; 3] = [Self::Clamp, Self::Border, Self::Repeat];

    #[must_use]
    pub const fn flag(self) -> &'static str {
        match self {
            Self::Clamp => "clamp",
            Self::Border => "border",
            Self::Repeat => "repeat",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Clamp => "Stretch the edge pixels",
            Self::Border => "Leave the border empty",
            Self::Repeat => "Tile the wallpaper",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Renderer {
    pub fps: u32,
    pub battery_fps: u32,
    pub render_scale: f32,
    pub playback_speed: f32,
    pub volume: u32,
    pub mute: bool,
    pub scaling: Scaling,
    pub clamp: Clamp,
    pub disable_mouse: bool,
    pub interactive: bool,
    pub disable_parallax: bool,
    pub disable_particles: bool,
    pub no_automute: bool,
    pub no_audio_processing: bool,
    pub no_fullscreen_pause: bool,
    pub focus_x: f32,
    pub focus_y: f32,
    #[serde(default)]
    pub gpu: String,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            fps: 30,
            battery_fps: 30,
            render_scale: 1.0,
            playback_speed: 1.0,
            volume: 100,
            mute: false,
            scaling: Scaling::default(),
            clamp: Clamp::default(),
            disable_mouse: false,
            interactive: false,
            disable_parallax: false,
            disable_particles: false,
            no_automute: false,
            no_audio_processing: false,
            no_fullscreen_pause: false,
            focus_x: 0.0,
            focus_y: 0.0,
            gpu: String::new(),
        }
    }
}

/// The renderer's volume scale.
///
/// haru's slider is 0-100 because that is what a volume slider reads as.
/// `--volume` and the `volume` socket command are 0-128, with a default of 15.
/// Converting here rather than at each call site is what keeps the two from
/// drifting apart -- they had, in three different directions at once.
#[must_use]
pub fn kirie_volume(percent: u32) -> u32 {
    percent.min(100) * 128 / 100
}

impl Renderer {
    #[must_use]
    pub fn arguments(&self) -> Vec<String> {
        let mut out = Vec::new();
        let gpu = self.gpu.trim();
        if !gpu.is_empty() && !gpu.eq_ignore_ascii_case("auto") {
            out.push(format!("--gpu={gpu}"));
        }
        // Sent even when it is 0: the slider calls 0 "unlimited" and that is
        // what `--fps=0` means to the renderer. Leaving the flag off instead
        // silently gave the renderer's own default of 30.
        out.push(format!("--fps={}", self.fps));
        if self.battery_fps > 0 {
            out.push(format!("--battery-fps={}", self.battery_fps));
        }
        if self.render_scale != 1.0 {
            out.push(format!("--render-scale={}", self.render_scale));
        }
        if self.playback_speed != 1.0 {
            out.push(format!("--playback-speed={}", self.playback_speed));
        }
        if self.mute {
            out.push("--silent".to_owned());
        } else {
            // Always sent, and on the renderer's scale. The slider is 0-100 and
            // `--volume` is 0-128 with a default of 15, so leaving the flag off
            // at 100 used to start the wallpaper at about 12% while the slider
            // read full.
            out.push(format!("--volume={}", kirie_volume(self.volume)));
        }
        if self.scaling != Scaling::Default {
            out.push(format!("--scaling={}", self.scaling.flag()));
        }
        if self.focus_x != 0.0 || self.focus_y != 0.0 {
            out.push(format!("--focus={},{}", self.focus_x, self.focus_y));
        }
        if self.clamp != Clamp::default() {
            out.push(format!("--clamp={}", self.clamp.flag()));
        }
        for (on, flag) in [
            (self.interactive, "--interactive"),
            (self.disable_mouse, "--disable-mouse"),
            (self.disable_parallax, "--disable-parallax"),
            (self.disable_particles, "--disable-particles"),
            (self.no_automute, "--noautomute"),
            (self.no_audio_processing, "--no-audio-processing"),
            (self.no_fullscreen_pause, "--no-fullscreen-pause"),
        ] {
            if on {
                out.push(flag.to_owned());
            }
        }
        out
    }

    #[must_use]
    pub fn live_commands(&self) -> Vec<String> {
        let mut out = Vec::new();
        // `set fps 0` is not "unlimited" on the socket the way `--fps=0` is at
        // launch: the renderer floors it at 1, which freezes the wallpaper at
        // one frame a second. Leaving the line out keeps whatever it is
        // running at, which is what the slider's "unlimited" was asking for.
        if self.fps > 0 {
            out.push(format!("set fps {}", self.fps));
        }
        out.extend([
            format!("set batteryfps {}", self.battery_fps),
            format!("set renderscale {}", self.render_scale),
            format!("speed {}", self.playback_speed),
            // 0-128, the same scale as `--volume` and as docs/COMMANDS.md.
            format!("volume {}", kirie_volume(self.volume)),
            format!("mute {}", u8::from(self.mute)),
            format!("set disablemouse {}", self.disable_mouse),
            format!("set disableparallax {}", self.disable_parallax),
            format!("set nofullscreenpause {}", self.no_fullscreen_pause),
            format!("set noautomute {}", self.no_automute),
        ]);
        out
    }

    #[must_use]
    pub fn needs_relaunch(&self, next: &Self) -> bool {
        self.interactive != next.interactive
            || self.focus_x != next.focus_x
            || self.focus_y != next.focus_y
            || self.scaling != next.scaling
            || self.clamp != next.clamp
            || self.disable_particles != next.disable_particles
            || self.no_audio_processing != next.no_audio_processing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_card_reaches_the_renderer() {
        let renderer = Renderer {
            gpu: "nvidia".to_owned(),
            ..Renderer::default()
        };
        assert!(renderer.arguments().iter().any(|arg| arg == "--gpu=nvidia"));
    }

    #[test]
    fn letting_the_driver_choose_passes_nothing() {
        for value in ["", "auto", "  Auto  "] {
            let renderer = Renderer {
                gpu: value.to_owned(),
                ..Renderer::default()
            };
            assert!(
                !renderer
                    .arguments()
                    .iter()
                    .any(|arg| arg.starts_with("--gpu")),
                "{value:?} should leave the choice alone"
            );
        }
    }

    #[test]
    fn the_default_fills_the_screen_at_thirty_frames() {
        let renderer = Renderer::default();
        assert_eq!(renderer.scaling, Scaling::Fill);
        assert_eq!(renderer.fps, 30);
        assert_eq!(renderer.battery_fps, 30);
        assert!(renderer.arguments().contains(&"--scaling=fill".to_owned()));
        assert!(renderer.arguments().contains(&"--fps=30".to_owned()));
        assert!(
            renderer
                .arguments()
                .contains(&"--battery-fps=30".to_owned())
        );
    }

    #[test]
    fn a_focus_only_travels_when_it_is_off_centre() {
        let mut renderer = Renderer::default();
        assert!(
            !renderer
                .arguments()
                .iter()
                .any(|arg| arg.starts_with("--focus"))
        );
        renderer.focus_x = -0.4;
        assert!(renderer.arguments().contains(&"--focus=-0.4,0".to_owned()));
    }

    #[test]
    fn an_uncapped_frame_rate_says_so() {
        let renderer = Renderer {
            fps: 0,
            ..Renderer::default()
        };
        // `--fps=0` is how the renderer is told to leave the rate alone.
        // Saying nothing instead gave it its own default of 30, so the slider
        // labelled "unlimited" quietly capped the wallpaper.
        assert!(renderer.arguments().contains(&"--fps=0".to_owned()));
        // The socket is the other way round: `set fps 0` is floored at 1 there,
        // which would freeze the wallpaper, so the line is left out.
        assert!(
            !renderer
                .live_commands()
                .iter()
                .any(|line| line.starts_with("set fps"))
        );
    }

    #[test]
    fn the_volume_reaches_the_renderer_on_its_own_scale() {
        let renderer = Renderer::default();
        assert_eq!(renderer.volume, 100, "the slider is 0-100");
        // 0-128 is what `--volume` and the `volume` command take. A full slider
        // used to send nothing at launch, leaving the renderer at its own
        // default of 15, and then `volume 100` on the socket.
        assert!(renderer.arguments().contains(&"--volume=128".to_owned()));
        assert!(renderer.live_commands().contains(&"volume 128".to_owned()));

        let half = Renderer {
            volume: 50,
            ..Renderer::default()
        };
        assert!(half.arguments().contains(&"--volume=64".to_owned()));
        assert!(half.live_commands().contains(&"volume 64".to_owned()));
    }

    #[test]
    fn muting_wins_over_a_volume() {
        let renderer = Renderer {
            volume: 40,
            mute: true,
            ..Renderer::default()
        };
        assert!(renderer.arguments().contains(&"--silent".to_owned()));
    }

    #[test]
    fn a_fill_and_a_repeat_are_named_the_way_kirie_names_them() {
        let renderer = Renderer {
            scaling: Scaling::Fill,
            clamp: Clamp::Repeat,
            ..Renderer::default()
        };
        assert!(renderer.arguments().contains(&"--scaling=fill".to_owned()));
        assert!(renderer.arguments().contains(&"--clamp=repeat".to_owned()));
    }

    #[test]
    fn only_launch_only_settings_ask_for_a_relaunch() {
        let base = Renderer::default();
        let live = Renderer {
            fps: 30,
            ..base.clone()
        };
        assert!(!base.needs_relaunch(&live));
        let launch = Renderer {
            scaling: Scaling::Fit,
            ..base.clone()
        };
        assert!(base.needs_relaunch(&launch));
    }

    #[test]
    fn every_setting_survives_a_round_trip() {
        let renderer = Renderer {
            fps: 24,
            battery_fps: 10,
            render_scale: 0.75,
            playback_speed: 1.5,
            volume: 20,
            mute: true,
            scaling: Scaling::Stretch,
            clamp: Clamp::Border,
            disable_mouse: true,
            interactive: true,
            disable_parallax: true,
            disable_particles: true,
            no_automute: true,
            no_audio_processing: true,
            no_fullscreen_pause: true,
            focus_x: -0.25,
            focus_y: 0.5,
            gpu: "nvidia".to_owned(),
        };
        let text = serde_json::to_string(&renderer).unwrap_or_default();
        let back: Renderer = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(back, renderer);
    }
}
