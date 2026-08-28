//! The knobs a wallpaper exposes.
//!
//! A wallpaper's `project.json` carries a `general.properties` object: the
//! sliders, switches, colours and dropdowns its author wanted people to have.
//! Wallpaper Engine shows them in a panel beside the wallpaper, and a picker
//! that cannot is only half of one — most of what makes a scene *yours* is in
//! there.
//!
//! Reading them needs no renderer, so the panel works with nothing running;
//! changing one needs a renderer, because it is the renderer that redraws.

use std::path::Path;

/// What kind of control a property wants.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A switch.
    Bool(bool),
    /// A number with a range.
    Slider {
        /// Where it is now.
        value: f64,
        /// The lowest it goes.
        min: f64,
        /// The highest it goes.
        max: f64,
        /// How far one notch moves it.
        step: f64,
    },
    /// A colour, as the `r g b` triple the renderer speaks.
    Color([f32; 3]),
    /// One of a fixed set.
    Combo {
        /// The chosen value.
        value: String,
        /// Every option, as label and value.
        options: Vec<(String, String)>,
    },
    /// Free text, and anything unrecognised.
    Text(String),
}

/// One property, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// The name the renderer knows it by.
    pub key: String,
    /// What to call it in the panel.
    pub label: String,
    /// What it is.
    pub kind: Kind,
    /// Where the author wanted it in the list.
    pub order: i64,
}

impl Property {
    /// The value as the control socket wants it.
    ///
    /// Colours are a space-separated triple, which is why they cannot be sent
    /// as a bare string: the renderer parses three floats.
    #[must_use]
    pub fn wire(&self) -> String {
        match &self.kind {
            Kind::Bool(on) => on.to_string(),
            Kind::Slider { value, .. } => format!("{value}"),
            Kind::Color([r, g, b]) => format!("{r} {g} {b}"),
            Kind::Combo { value, .. } => value.clone(),
            Kind::Text(text) => text.clone(),
        }
    }
}

/// Reads the properties a wallpaper directory exposes, in the author's order.
///
/// A wallpaper with none — most videos — gives an empty list rather than an
/// error, because having no settings is an ordinary state and not a failure.
#[must_use]
pub fn read(dir: &Path) -> Vec<Property> {
    let Ok(text) = std::fs::read_to_string(dir.join("project.json")) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(properties) = parsed
        .get("general")
        .and_then(|general| general.get("properties"))
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };

    let mut found: Vec<Property> = properties
        .iter()
        .filter_map(|(key, value)| property(key, value))
        .collect();
    found.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.key.cmp(&right.key))
    });
    found
}

/// Reads one entry of the properties object.
fn property(key: &str, raw: &serde_json::Value) -> Option<Property> {
    let object = raw.as_object()?;
    let declared = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    // A `text` property is a heading the author put between controls, not a
    // control. Drawing it as a text box is the tag-soup this used to be.
    if declared == "text" {
        return None;
    }

    let value = object.get("value");
    let kind = match declared {
        "bool" => Kind::Bool(match value {
            Some(serde_json::Value::Bool(on)) => *on,
            // Authors write both `true` and `"true"`, and a string that is not
            // "true" is off.
            Some(serde_json::Value::String(text)) => text == "true",
            _ => false,
        }),
        "slider" => Kind::Slider {
            value: number(value).unwrap_or(0.0),
            min: number(object.get("min")).unwrap_or(0.0),
            max: number(object.get("max")).unwrap_or(1.0),
            step: number(object.get("step")).unwrap_or(0.01),
        },
        "color" => Kind::Color(triple(value.and_then(serde_json::Value::as_str).unwrap_or(""))),
        "combo" => Kind::Combo {
            value: value
                .map(scalar_to_string)
                .unwrap_or_default(),
            options: object
                .get("options")
                .and_then(serde_json::Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter_map(|option| {
                            let value = option.get("value").map(scalar_to_string)?;
                            let label = option
                                .get("label")
                                .and_then(serde_json::Value::as_str)
                                .map_or_else(|| value.clone(), crate::plain_text);
                            Some((label, value))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        _ => Kind::Text(value.map(scalar_to_string).unwrap_or_default()),
    };

    let label = object
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(crate::plain_text)
        .filter(|label| !label.is_empty())
        .map_or_else(|| pretty(key), |label| readable(&label));

    Some(Property {
        key: key.to_owned(),
        label,
        kind,
        order: object
            .get("order")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(i64::MAX),
    })
}

/// A JSON scalar as the string the renderer would take.
fn scalar_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A number, whether it arrived as one or as a string.
fn number(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// Reads an `r g b` triple, each channel 0..=1.
fn triple(raw: &str) -> [f32; 3] {
    let mut channels = raw.split_whitespace().filter_map(|part| part.parse().ok());
    [
        channels.next().unwrap_or(1.0),
        channels.next().unwrap_or(1.0),
        channels.next().unwrap_or(1.0),
    ]
}

/// A label fit to put beside a control.
///
/// Two things authors leave in one: Wallpaper Engine's own localisation keys,
/// which the editor resolves and a reader does not, and occasionally the whole
/// contents of the property as its own label — one observed here is a 200-byte
/// image URL, which pushes every control below it off the panel.
fn readable(label: &str) -> String {
    /// Past this a label is not a label.
    const LONGEST: usize = 48;

    let looks_like_a_key = label.starts_with("ui_")
        || (!label.contains(' ') && label.contains('_') && label.is_ascii());
    let text = if looks_like_a_key {
        pretty(label)
    } else {
        label.to_owned()
    };

    if text.chars().count() <= LONGEST {
        return text;
    }
    let clipped: String = text.chars().take(LONGEST).collect();
    format!("{}…", clipped.trim_end())
}

/// A readable name for a property with no label of its own.
///
/// Wallpaper Engine's own localisation keys look like
/// `ui_browse_properties_scheme_color`; showing that verbatim is worse than
/// showing nothing.
fn pretty(key: &str) -> String {
    let trimmed = key
        .strip_prefix("ui_browse_properties_")
        .or_else(|| key.strip_prefix("ui_editor_properties_"))
        .or_else(|| key.strip_prefix("ui_editor_effect_"))
        .unwrap_or(key)
        .trim_end_matches("_title")
        .replace(['_', '-'], " ");
    let mut characters = trimmed.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + characters.as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(project: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("haru-props-{:x}", project.len()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("project.json"), project);
        dir
    }

    #[test]
    fn every_control_kind_reads_back_as_itself() {
        let dir = write(
            r#"{"general":{"properties":{
                "glow":{"type":"bool","value":"true","order":1,"text":"Glow"},
                "speed":{"type":"slider","value":0.5,"min":0,"max":2,"step":0.1,"order":2},
                "tint":{"type":"color","value":"0.5 0.25 1","order":3},
                "mode":{"type":"combo","value":"b","order":4,
                        "options":[{"label":"A","value":"a"},{"label":"B","value":"b"}]}
            }}}"#,
        );

        let found = read(&dir);
        assert_eq!(found.len(), 4);
        assert_eq!(found.first().map(|p| p.kind.clone()), Some(Kind::Bool(true)));
        assert!(matches!(
            found.get(1).map(|p| p.kind.clone()),
            Some(Kind::Slider { max, .. }) if (max - 2.0).abs() < f64::EPSILON
        ));
        assert_eq!(
            found.get(2).map(|p| p.kind.clone()),
            Some(Kind::Color([0.5, 0.25, 1.0]))
        );
        assert_eq!(
            found.get(3).map(Property::wire),
            Some("b".to_owned()),
            "a combo sends its value, not its label"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_heading_is_not_a_control() {
        // Authors put `text` entries between controls as section titles, and
        // drawing them as editable fields is the tag soup this avoids.
        let dir = write(r#"{"general":{"properties":{"banner":{"type":"text","value":"Colours"}}}}"#);
        assert!(read(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn properties_keep_the_authors_order() {
        let dir = write(
            r#"{"general":{"properties":{
                "zeta":{"type":"bool","value":true,"order":1},
                "alpha":{"type":"bool","value":true,"order":9}
            }}}"#,
        );
        let keys: Vec<String> = read(&dir).into_iter().map(|p| p.key).collect();
        assert_eq!(keys, vec!["zeta", "alpha"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wallpaper_with_no_properties_is_not_an_error() {
        let dir = write(r#"{"title":"Just a video","type":"video"}"#);
        assert!(read(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_colour_travels_as_the_triple_the_renderer_parses() {
        let property = Property {
            key: "tint".to_owned(),
            label: "Tint".to_owned(),
            kind: Kind::Color([0.5, 0.25, 1.0]),
            order: 0,
        };
        assert_eq!(property.wire(), "0.5 0.25 1");
    }

    #[test]
    fn a_key_with_no_label_is_made_readable() {
        assert_eq!(pretty("ui_browse_properties_scheme_color"), "Scheme color");
        assert_eq!(pretty("glow_amount"), "Glow amount");
        assert_eq!(pretty("ui_editor_effect_local_contrast_title"), "Local contrast");
    }

    #[test]
    fn a_label_that_is_itself_a_key_is_read_the_same_way() {
        // Authors put the localisation key in the label field as often as they
        // leave it out, and the editor resolves it where a reader cannot.
        assert_eq!(readable("ui_browse_properties_brightness"), "Brightness");
        assert_eq!(readable("Cursor | 光标"), "Cursor | 光标");
    }

    #[test]
    fn a_label_that_is_really_the_value_is_clipped() {
        // One observed wallpaper labels a property with a 200-byte image URL,
        // which pushes every control under it off the panel.
        let long = "img src=http://example.invalid/".repeat(9);
        let shown = readable(&long);
        assert!(shown.chars().count() <= 49, "{shown}");
        assert!(shown.ends_with('…'));
    }
}
