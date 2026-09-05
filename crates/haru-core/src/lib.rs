pub mod config;
pub mod engine;
pub mod library;
pub mod overrides;
pub mod properties;
pub mod renderer;

pub use config::Config;
pub use library::Installed;
pub use properties::Property;

use tapline::{BrowseQuery, BrowseSort, ContentDescriptor, TextTarget, TimeRange};

pub const WALLPAPER_ENGINE: tapline_ids::AppId = tapline_ids::AppId(431_960);

pub struct TagGroup {
    pub label: &'static str,
    pub tags: &'static [&'static str],
}

pub const TAG_GROUPS: &[TagGroup] = &[
    TagGroup {
        label: "Type",
        tags: &["Scene", "Video", "Web", "Application"],
    },
    TagGroup {
        label: "Age rating",
        tags: &["Everyone", "Questionable", "Mature"],
    },
    TagGroup {
        label: "Genre",
        tags: &[
            "Abstract",
            "Animal",
            "Anime",
            "Cartoon",
            "CGI",
            "Cyberpunk",
            "Fantasy",
            "Game",
            "Girls",
            "Guys",
            "Landscape",
            "Medieval",
            "Memes",
            "MMD",
            "Music",
            "Nature",
            "Pixel art",
            "Relaxing",
            "Retro",
            "Sci-Fi",
            "Sports",
            "Technology",
            "Television",
            "Vehicle",
            "Unspecified",
        ],
    },
    TagGroup {
        label: "Resolution",
        tags: &[
            "Standard Definition",
            "1280 x 720",
            "1920 x 1080",
            "2560 x 1440",
            "3840 x 2160",
            "Ultrawide Standard",
            "Ultrawide 2560 x 1080",
            "Ultrawide 3440 x 1440",
            "Dual Standard",
            "Dual 3840 x 1080",
            "Dual 5120 x 1440",
            "Triple Standard",
            "Triple 5760 x 1080",
            "Triple 7680 x 1440",
            "Portrait Standard",
            "Portrait 720 x 1280",
            "Portrait 1080 x 1920",
            "Portrait 1440 x 2560",
            "Portrait 2160 x 3840",
            "Other resolution",
            "Dynamic resolution",
        ],
    },
    TagGroup {
        label: "Category",
        tags: &["Wallpaper", "Preset", "Asset"],
    },
    TagGroup {
        label: "Features",
        tags: &[
            "Approved",
            "Audio responsive",
            "3D",
            "Customizable",
            "Puppet Warp",
            "HDR",
            "Media Integration",
            "User Shortcut",
            "Video Texture",
            "Asset Pack",
        ],
    },
];

pub const TREND_PERIODS: &[(&str, u32)] = &[
    ("Today", 1),
    ("This week", 7),
    ("Three months", 90),
    ("Six months", 180),
    ("This year", 365),
];

#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub text: String,
    pub search_in: TextTarget,
    pub chosen: Vec<Vec<String>>,
    pub sort: BrowseSort,
    pub trend_days: Option<u32>,
    pub updated_since: Option<u32>,
    pub adult: bool,
    pub page: u32,
    pub per_page: u32,
}

impl Filters {
    #[must_use]
    pub fn new() -> Self {
        Self {
            chosen: vec![Vec::new(); TAG_GROUPS.len()],
            per_page: 24,
            page: 1,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn is_narrowed(&self) -> bool {
        !self.text.is_empty()
            || self.chosen.iter().any(|group| !group.is_empty())
            || self.updated_since.is_some()
    }

    #[must_use]
    pub const fn pages(&self, total: u32) -> u32 {
        let per_page = if self.per_page == 0 { 1 } else { self.per_page };
        total.div_ceil(per_page)
    }

    pub fn clear(&mut self) {
        let sort = self.sort;
        let adult = self.adult;
        *self = Self::new();
        self.sort = sort;
        self.adult = adult;
    }

    pub fn to_query(&self) -> BrowseQuery {
        let text = self.text.trim();

        BrowseQuery {
            app: WALLPAPER_ENGINE,
            text: (!text.is_empty()).then(|| text.to_owned()),
            search_in: if text.is_empty() {
                TextTarget::Everything
            } else {
                self.search_in
            },
            tag_groups: self
                .chosen
                .iter()
                .filter(|group| !group.is_empty())
                .cloned()
                .collect(),
            excluded_descriptors: if self.adult {
                Vec::new()
            } else {
                vec![ContentDescriptor::AnyMature]
            },
            sort: self.sort,
            trend_days: (self.sort == BrowseSort::Trend)
                .then_some(self.trend_days)
                .flatten(),
            updated: self.updated_since.map(|start| TimeRange {
                start: Some(start),
                end: None,
            }),
            per_page: self.per_page,
            page: (self.page > 1).then_some(self.page),
            ..BrowseQuery::default()
        }
    }
}

#[must_use]
pub fn human_size(bytes: u64) -> String {
    const UNITS: [(u64, &str); 3] = [
        (1024 * 1024 * 1024, "GB"),
        (1024 * 1024, "MB"),
        (1024, "KB"),
    ];
    for (scale, unit) in UNITS {
        if bytes >= scale {
            #[expect(
                clippy::cast_precision_loss,
                reason = "one decimal place of a file size"
            )]
            return format!("{:.1} {unit}", bytes as f64 / scale as f64);
        }
    }
    format!("{bytes} B")
}

#[must_use]
pub fn plain_text(raw: &str) -> String {
    let mut text = raw.to_owned();
    for _ in 0..3 {
        text = text
            .replace("&nbsp;", " ")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#039;", "'")
            .replace("&amp;", "&");
        let mut stripped = String::with_capacity(text.len());
        let mut depth = 0_u32;
        for character in text.chars() {
            match character {
                '<' => depth = depth.saturating_add(1),
                '>' => depth = depth.saturating_sub(1),
                _ if depth == 0 => stripped.push(character),
                _ => {}
            }
        }
        text = stripped;
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_tag_per_axis_becomes_one_group_per_axis() {
        let mut filters = Filters::new();
        if let Some(slot) = filters.chosen.get_mut(0) {
            *slot = vec!["Scene".to_owned()];
        }
        if let Some(slot) = filters.chosen.get_mut(2) {
            *slot = vec!["Anime".to_owned()];
        }

        let query = filters.to_query();
        assert_eq!(
            query.tag_groups,
            vec![vec!["Scene".to_owned()], vec!["Anime".to_owned()]]
        );
        assert!(query.required_tags.is_empty());
    }

    #[test]
    fn several_tags_on_one_axis_stay_one_group() {
        let mut filters = Filters::new();
        if let Some(slot) = filters.chosen.get_mut(2) {
            *slot = vec!["Anime".to_owned(), "Game".to_owned()];
        }

        let query = filters.to_query();
        assert_eq!(
            query.tag_groups,
            vec![vec!["Anime".to_owned(), "Game".to_owned()]]
        );
    }

    #[test]
    fn an_empty_axis_asks_for_nothing() {
        let filters = Filters::new();
        assert!(filters.to_query().tag_groups.is_empty());
        assert!(!filters.is_narrowed());
    }

    #[test]
    fn a_trend_window_is_dropped_when_the_sort_is_not_trend() {
        let filters = Filters {
            sort: BrowseSort::Vote,
            trend_days: Some(180),
            ..Filters::new()
        };
        assert_eq!(filters.to_query().trend_days, None);
        assert_eq!(filters.to_query().validate(), Ok(()));
    }

    #[test]
    fn narrowing_the_text_target_is_dropped_with_an_empty_box() {
        let filters = Filters {
            search_in: TextTarget::Title,
            ..Filters::new()
        };
        assert_eq!(filters.to_query().validate(), Ok(()));
    }

    #[test]
    fn adult_content_is_excluded_by_label_unless_asked_for() {
        assert_eq!(
            Filters::new().to_query().excluded_descriptors,
            vec![ContentDescriptor::AnyMature]
        );
        let allowed = Filters {
            adult: true,
            ..Filters::new()
        };
        assert!(allowed.to_query().excluded_descriptors.is_empty());
    }

    #[test]
    fn clearing_keeps_the_sort_and_the_adult_choice() {
        let mut filters = Filters {
            sort: BrowseSort::Recent,
            adult: true,
            text: "miku".to_owned(),
            ..Filters::new()
        };
        filters.clear();
        assert_eq!(filters.sort, BrowseSort::Recent);
        assert!(filters.adult);
        assert!(filters.text.is_empty());
    }

    #[test]
    fn markup_survives_neither_decoding_nor_stripping_alone() {
        assert_eq!(plain_text("&lt;b&gt;Neon&lt;/b&gt; nights"), "Neon nights");
        assert_eq!(plain_text("<i>Rain</i>&nbsp;&amp;&nbsp;fog"), "Rain & fog");
    }

    #[test]
    fn sizes_read_the_way_a_person_says_them() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(25_179_527), "24.0 MB");
    }
}

/// Where kirie puts its control socket when the session has no
/// `XDG_RUNTIME_DIR` (macOS, or a bare login). The temp dir is shared between
/// accounts there, so a fixed name collides and the second user cannot even
/// unlink the first one's socket under the sticky bit. This must stay in step
/// with kirie's own `default_control_socket`.
#[must_use]
pub fn runtime_dir() -> std::path::PathBuf {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return std::path::PathBuf::from(runtime);
    }
    let dir = std::env::temp_dir().join(format!("kirie-{}", user_tag()));
    if std::fs::create_dir_all(&dir).is_ok() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir
}

fn user_tag() -> String {
    use std::os::unix::fs::MetadataExt;
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(meta) = std::fs::metadata(&home)
    {
        return meta.uid().to_string();
    }
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "shared".to_owned())
}

#[cfg(test)]
mod runtime_dir_tests {
    #[test]
    fn the_fallback_is_per_user() {
        let tag = super::user_tag();
        assert!(!tag.is_empty());
        assert!(!tag.contains('/'), "used as a directory name: {tag}");
    }
}
