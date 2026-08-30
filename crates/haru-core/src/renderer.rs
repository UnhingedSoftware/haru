use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scaling {
    #[default]
    Default,
    Fit,
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
    pub disable_parallax: bool,
    pub disable_particles: bool,
    pub no_automute: bool,
    pub no_audio_processing: bool,
    pub no_fullscreen_pause: bool,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            fps: 0,
            battery_fps: 0,
            render_scale: 1.0,
            playback_speed: 1.0,
            volume: 100,
            mute: false,
            scaling: Scaling::default(),
            clamp: Clamp::default(),
            disable_mouse: false,
            disable_parallax: false,
            disable_particles: false,
            no_automute: false,
            no_audio_processing: false,
            no_fullscreen_pause: false,
        }
    }
}

impl Renderer {
    #[must_use]
    pub fn arguments(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.fps > 0 {
            out.push(format!("--fps={}", self.fps));
        }
        if self.render_scale != 1.0 {
            out.push(format!("--render-scale={}", self.render_scale));
        }
        if self.playback_speed != 1.0 {
            out.push(format!("--playback-speed={}", self.playback_speed));
        }
        if self.mute {
            out.push("--silent".to_owned());
        } else if self.volume != 100 {
            out.push(format!("--volume={}", self.volume));
        }
        if self.scaling != Scaling::Default {
            out.push(format!("--scaling={}", self.scaling.flag()));
        }
        if self.clamp != Clamp::default() {
            out.push(format!("--clamp={}", self.clamp.flag()));
        }
        for (on, flag) in [
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
        vec![
            format!("set fps {}", self.fps),
            format!("set batteryfps {}", self.battery_fps),
            format!("set renderscale {}", self.render_scale),
            format!("speed {}", self.playback_speed),
            format!("volume {}", self.volume),
            format!("mute {}", u8::from(self.mute)),
            format!("set disablemouse {}", self.disable_mouse),
            format!("set disableparallax {}", self.disable_parallax),
            format!("set nofullscreenpause {}", self.no_fullscreen_pause),
            format!("set noautomute {}", self.no_automute),
        ]
    }

    #[must_use]
    pub fn needs_relaunch(&self, next: &Self) -> bool {
        self.scaling != next.scaling
            || self.clamp != next.clamp
            || self.disable_particles != next.disable_particles
            || self.no_audio_processing != next.no_audio_processing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_ask_the_renderer_for_nothing() {
        assert!(Renderer::default().arguments().is_empty());
    }

    #[test]
    fn a_capped_frame_rate_reaches_the_command_line() {
        let renderer = Renderer {
            fps: 30,
            ..Renderer::default()
        };
        assert_eq!(renderer.arguments(), vec!["--fps=30".to_owned()]);
    }

    #[test]
    fn muting_wins_over_a_volume() {
        let renderer = Renderer {
            volume: 40,
            mute: true,
            ..Renderer::default()
        };
        assert_eq!(renderer.arguments(), vec!["--silent".to_owned()]);
    }

    #[test]
    fn a_fill_and_a_repeat_are_named_the_way_kirie_names_them() {
        let renderer = Renderer {
            scaling: Scaling::Fill,
            clamp: Clamp::Repeat,
            ..Renderer::default()
        };
        assert_eq!(
            renderer.arguments(),
            vec!["--scaling=fill".to_owned(), "--clamp=repeat".to_owned()]
        );
    }

    #[test]
    fn only_launch_only_settings_ask_for_a_relaunch() {
        let base = Renderer::default();
        let live = Renderer { fps: 30, ..base };
        assert!(!base.needs_relaunch(&live));
        let launch = Renderer {
            scaling: Scaling::Fit,
            ..base
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
            disable_parallax: true,
            disable_particles: true,
            no_automute: true,
            no_audio_processing: true,
            no_fullscreen_pause: true,
        };
        let text = serde_json::to_string(&renderer).unwrap_or_default();
        let back: Renderer = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(back, renderer);
    }
}
