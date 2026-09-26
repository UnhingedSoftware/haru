//! Names a person recognises for the screens the renderer reports.
//!
//! kirie names a Windows screen after its GDI device, `DISPLAY6`, which is
//! neither the number Windows' own settings show nor anything printed on the
//! monitor. Wayland and X11 connector names (`DP-1`) stay as they are.

use crate::Screen;

#[cfg(windows)]
pub(crate) fn label(screens: &mut [Screen]) {
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// The screens last asked about, and what they were called.
    type Asked = (Vec<String>, HashMap<String, String>);

    // Asked again only when the set of screens changes, not on every poll.
    static KNOWN: Mutex<Option<Asked>> = Mutex::new(None);

    let names: Vec<String> = screens.iter().map(|screen| screen.name.clone()).collect();
    let Ok(mut known) = KNOWN.lock() else {
        return;
    };
    let fresh = known
        .as_ref()
        .is_none_or(|(asked_for, _)| *asked_for != names);
    if fresh {
        let monitors = display_info::DisplayInfo::all()
            .unwrap_or_default()
            .into_iter()
            .map(|display| Monitor {
                device: device_tail(&display.name).to_owned(),
                model: display.friendly_name,
                builtin: display.is_builtin,
            })
            .collect::<Vec<_>>();
        *known = Some((names.clone(), labels(&names, &monitors)));
    }
    if let Some((_, labels)) = known.as_ref() {
        for screen in screens {
            if let Some(label) = labels.get(&screen.name) {
                screen.label.clone_from(label);
            }
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn label(_screens: &mut [Screen]) {}

#[cfg_attr(not(windows), allow(dead_code))]
struct Monitor {
    /// `DISPLAY6`, from `\\.\DISPLAY6`, which is how kirie names it.
    device: String,
    /// The EDID name, `DELL U2720Q`; empty or a placeholder when unknown.
    model: String,
    builtin: bool,
}

#[cfg_attr(not(windows), allow(dead_code))]
fn device_tail(device: &str) -> &str {
    device.rsplit('\\').next().unwrap_or(device)
}

/// `DISPLAY6` becomes `Display 6`, the way Windows writes it.
#[cfg_attr(not(windows), allow(dead_code))]
fn spelled(device: &str) -> String {
    match device.strip_prefix("DISPLAY") {
        Some(number) if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) => {
            format!("Display {number}")
        }
        _ => device.to_owned(),
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn labels(names: &[String], monitors: &[Monitor]) -> std::collections::HashMap<String, String> {
    let model = |name: &str| {
        let monitor = monitors
            .iter()
            .find(|monitor| monitor.device.eq_ignore_ascii_case(name))?;
        let model = monitor.model.trim();
        if !model.is_empty() && !model.starts_with("Unknown Display") {
            Some(model.to_owned())
        } else if monitor.builtin {
            Some("Built-in display".to_owned())
        } else {
            None
        }
    };

    let found: Vec<(String, Option<String>)> = names
        .iter()
        .map(|name| (name.clone(), model(name)))
        .collect();
    found
        .iter()
        .map(|(name, model)| {
            let label = match model {
                // Two of the same monitor need the number to tell them apart.
                Some(model)
                    if found
                        .iter()
                        .filter(|(_, other)| other.as_ref() == Some(model))
                        .count()
                        > 1 =>
                {
                    format!("{model} ({})", spelled(name))
                }
                Some(model) => model.clone(),
                None => spelled(name),
            };
            (name.clone(), label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(device: &str, model: &str, builtin: bool) -> Monitor {
        Monitor {
            device: device_tail(device).to_owned(),
            model: model.to_owned(),
            builtin,
        }
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_monitor_is_called_by_its_model() {
        let found = labels(
            &names(&["DISPLAY6"]),
            &[monitor(r"\\.\DISPLAY6", "DELL U2720Q", false)],
        );
        assert_eq!(
            found.get("DISPLAY6").map(String::as_str),
            Some("DELL U2720Q")
        );
    }

    #[test]
    fn two_of_the_same_model_keep_their_numbers() {
        let found = labels(
            &names(&["DISPLAY1", "DISPLAY2"]),
            &[
                monitor(r"\\.\DISPLAY1", "LG ULTRAGEAR", false),
                monitor(r"\\.\DISPLAY2", "LG ULTRAGEAR", false),
            ],
        );
        assert_eq!(
            found.get("DISPLAY2").map(String::as_str),
            Some("LG ULTRAGEAR (Display 2)")
        );
    }

    #[test]
    fn an_unnamed_monitor_is_spelled_like_windows_does() {
        let found = labels(
            &names(&["DISPLAY3", "DISPLAY4", "desktop"]),
            &[
                monitor(r"\\.\DISPLAY3", "", true),
                monitor(r"\\.\DISPLAY4", "Unknown Display 65537", false),
            ],
        );
        assert_eq!(
            found.get("DISPLAY3").map(String::as_str),
            Some("Built-in display")
        );
        assert_eq!(found.get("DISPLAY4").map(String::as_str), Some("Display 4"));
        assert_eq!(found.get("desktop").map(String::as_str), Some("desktop"));
    }
}
