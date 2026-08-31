use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Bool(bool),
    Slider {
        value: f64,
        min: f64,
        max: f64,
        step: f64,
    },
    Color([f32; 3]),
    Combo {
        value: String,
        options: Vec<(String, String)>,
    },
    Text(String),
    Caption,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub key: String,
    pub label: String,
    pub kind: Kind,
    pub order: i64,
}

impl Property {
    pub fn set_from_wire(&mut self, raw: &str) {
        match &mut self.kind {
            Kind::Bool(on) => *on = raw == "true" || raw == "1",
            Kind::Slider {
                value, min, max, ..
            } => {
                if let Ok(parsed) = raw.trim().parse::<f64>() {
                    *value = parsed.clamp(*min, *max);
                }
            }
            Kind::Color(rgb) => *rgb = triple(raw),
            Kind::Combo { value, options } => {
                if options.iter().any(|(_, option)| option == raw) {
                    *value = raw.to_owned();
                }
            }
            Kind::Text(text) => *text = raw.to_owned(),
            Kind::Caption => {}
        }
    }

    #[must_use]
    pub fn wire(&self) -> String {
        match &self.kind {
            Kind::Bool(on) => on.to_string(),
            Kind::Slider { value, .. } => format!("{value}"),
            Kind::Color([r, g, b]) => format!("{r} {g} {b}"),
            Kind::Combo { value, .. } => value.clone(),
            Kind::Text(text) => text.clone(),
            Kind::Caption => String::new(),
        }
    }
}

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

fn property(key: &str, raw: &serde_json::Value) -> Option<Property> {
    let object = raw.as_object()?;
    let declared = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    if declared == "text" {
        return None;
    }

    let value = object.get("value");
    let kind = match declared {
        "bool" => Kind::Bool(match value {
            Some(serde_json::Value::Bool(on)) => *on,
            Some(serde_json::Value::String(text)) => text == "true",
            _ => false,
        }),
        "slider" => Kind::Slider {
            value: number(value).unwrap_or(0.0),
            min: number(object.get("min")).unwrap_or(0.0),
            max: number(object.get("max")).unwrap_or(1.0),
            step: number(object.get("step")).unwrap_or(0.01),
        },
        "color" => Kind::Color(triple(
            value.and_then(serde_json::Value::as_str).unwrap_or(""),
        )),
        "combo" => Kind::Combo {
            value: value.map(scalar_to_string).unwrap_or_default(),
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
        "group" => Kind::Caption,
        _ => Kind::Text(value.map(scalar_to_string).unwrap_or_default()),
    };

    let label = object
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(crate::plain_text)
        .filter(|label| !label.is_empty())
        .map_or_else(|| clip(&pretty(key)), |label| readable(&label));

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

fn scalar_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn number(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn triple(raw: &str) -> [f32; 3] {
    let mut channels = raw.split_whitespace().filter_map(|part| part.parse().ok());
    [
        channels.next().unwrap_or(1.0),
        channels.next().unwrap_or(1.0),
        channels.next().unwrap_or(1.0),
    ]
}

fn readable(label: &str) -> String {
    let looks_like_a_key = label.starts_with("ui_")
        || (!label.contains(' ') && label.contains('_') && label.is_ascii());
    if looks_like_a_key {
        clip(&pretty(label))
    } else {
        clip(label)
    }
}

fn clip(label: &str) -> String {
    const LONGEST: usize = 48;

    if label.chars().count() <= LONGEST {
        return label.to_owned();
    }
    let clipped: String = label.chars().take(LONGEST).collect();
    format!("{}…", clipped.trim_end())
}

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
        assert_eq!(
            found.first().map(|p| p.kind.clone()),
            Some(Kind::Bool(true))
        );
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
    fn a_group_reads_as_a_heading_with_no_value() {
        let dir = write(
            r#"{"general":{"properties":{
                "scene":{"type":"group","text":"Scene","order":0},
                "bloom":{"type":"bool","value":true,"order":1,"text":"Bloom"}
            }}}"#,
        );
        let found = read(&dir);
        assert_eq!(found.len(), 2);
        let Some(first) = found.first() else { return };
        assert_eq!(first.kind, Kind::Caption);
        assert_eq!(first.label, "Scene");
        assert!(first.wire().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_heading_is_not_a_control() {
        let dir =
            write(r#"{"general":{"properties":{"banner":{"type":"text","value":"Colours"}}}}"#);
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
    fn a_saved_value_goes_back_into_the_property_it_came_from() {
        let mut slider = Property {
            key: "speed".to_owned(),
            label: "Speed".to_owned(),
            kind: Kind::Slider {
                value: 1.0,
                min: 0.0,
                max: 2.0,
                step: 0.1,
            },
            order: 0,
        };
        slider.set_from_wire("1.5");
        assert_eq!(slider.wire(), "1.5");

        slider.set_from_wire("99");
        assert_eq!(slider.wire(), "2");

        slider.set_from_wire("fast");
        assert_eq!(slider.wire(), "2");
    }

    #[test]
    fn a_saved_option_the_wallpaper_no_longer_offers_is_ignored() {
        let mut combo = Property {
            key: "mode".to_owned(),
            label: "Mode".to_owned(),
            kind: Kind::Combo {
                value: "a".to_owned(),
                options: vec![("A".to_owned(), "a".to_owned())],
            },
            order: 0,
        };
        combo.set_from_wire("gone");
        assert_eq!(combo.wire(), "a");
        combo.set_from_wire("a");
        assert_eq!(combo.wire(), "a");
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
        assert_eq!(
            pretty("ui_editor_effect_local_contrast_title"),
            "Local contrast"
        );
    }

    #[test]
    fn a_label_that_is_itself_a_key_is_read_the_same_way() {
        assert_eq!(readable("ui_browse_properties_brightness"), "Brightness");
        assert_eq!(readable("Cursor | 光标"), "Cursor | 光标");
    }

    #[test]
    fn a_key_that_is_really_the_value_is_clipped_too() {
        let long = "img_src_http_example_invalid_".repeat(9);
        let shown = property(
            &long,
            &serde_json::json!({"type": "textinput", "value": ""}),
        )
        .map(|found| found.label)
        .unwrap_or_default();
        assert!(shown.chars().count() <= 49, "{shown}");
    }

    #[test]
    fn a_label_that_is_really_the_value_is_clipped() {
        let long = "img src=http://example.invalid/".repeat(9);
        let shown = readable(&long);
        assert!(shown.chars().count() <= 49, "{shown}");
        assert!(shown.ends_with('…'));
    }
}
