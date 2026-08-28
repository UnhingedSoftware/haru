//! What haru knows with no window open.
//!
//! The filter vocabulary and the state of a search live here rather than in
//! the UI, so the same query can be built by a window, a command line or the
//! studio later. Nothing in this crate draws anything or touches a platform.

pub mod config;
pub mod library;
pub mod properties;

pub use config::Config;
pub use library::Installed;
pub use properties::Property;

use tapline::{BrowseQuery, BrowseSort, ContentDescriptor, TextTarget, TimeRange};

/// Wallpaper Engine.
pub const WALLPAPER_ENGINE: tapline_ids::AppId = tapline_ids::AppId(431_960);

/// One axis of Steam's filter sidebar.
///
/// Steam's own semantics are one tag from each axis: ticking Scene and Video
/// under Type and Anime under Genre means *(Scene or Video) and Anime*. That
/// is what [`Filters::to_query`] builds, and it is why the axes are separate
/// values rather than one list of tags.
pub struct TagGroup {
    /// What the axis is called.
    pub label: &'static str,
    /// Every tag on it.
    pub tags: &'static [&'static str],
}

/// Wallpaper Engine's filter axes, in the order Steam shows them.
///
/// Hardcoded, and honestly so: PICS `app_info` does not carry the Workshop tag
/// list — it reports branches and depots and nothing about tags — so there is
/// nowhere to read this from. Results carry the tags they have, which is how a
/// missing one gets noticed.
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

/// How long a trend ranking looks back, as Steam offers it.
pub const TREND_PERIODS: &[(&str, u32)] = &[
    ("Today", 1),
    ("This week", 7),
    ("Three months", 90),
    ("Six months", 180),
    ("This year", 365),
];

/// Everything a search is currently asking for.
///
/// Held as the UI's own state and turned into a [`BrowseQuery`] on the way
/// out, so the widgets never assemble a wire request by hand.
#[derive(Debug, Clone, Default)]
pub struct Filters {
    /// The text in the search box.
    pub text: String,
    /// Where that text is matched.
    pub search_in: TextTarget,
    /// One chosen tag per axis, indexed the same as [`TAG_GROUPS`].
    pub chosen: Vec<Option<String>>,
    /// How to order results.
    pub sort: BrowseSort,
    /// The trend window, when sorting by trend.
    pub trend_days: Option<u32>,
    /// Only items revised since this moment.
    pub updated_since: Option<u32>,
    /// Whether to let adult content through.
    pub adult: bool,
    /// Which page, as Steam's cursor.
    pub cursor: Option<String>,
    /// How many per page.
    pub per_page: u32,
}

impl Filters {
    /// The state a picker opens with.
    #[must_use]
    pub fn new() -> Self {
        Self {
            chosen: vec![None; TAG_GROUPS.len()],
            per_page: 24,
            ..Self::default()
        }
    }

    /// Whether anything is being filtered on beyond the defaults.
    #[must_use]
    pub fn is_narrowed(&self) -> bool {
        !self.text.is_empty()
            || self.chosen.iter().any(Option::is_some)
            || self.updated_since.is_some()
    }

    /// Clears every filter, keeping the sort.
    pub fn clear(&mut self) {
        let sort = self.sort;
        let adult = self.adult;
        *self = Self::new();
        self.sort = sort;
        self.adult = adult;
    }

    /// Builds the query these filters describe.
    pub fn to_query(&self) -> BrowseQuery {
        let text = self.text.trim();

        BrowseQuery {
            app: WALLPAPER_ENGINE,
            text: (!text.is_empty()).then(|| text.to_owned()),
            // Narrowing with no text to narrow is refused by tapline, and the
            // box being empty is the ordinary state of a filter sidebar.
            search_in: if text.is_empty() {
                TextTarget::Everything
            } else {
                self.search_in
            },
            tag_groups: self
                .chosen
                .iter()
                .filter_map(|chosen| chosen.clone())
                .map(|tag| vec![tag])
                .collect(),
            // Steam's own labels rather than the Mature tag: an author who
            // never ticked the tag is still covered by the descriptor.
            excluded_descriptors: if self.adult {
                Vec::new()
            } else {
                vec![ContentDescriptor::AnyMature]
            },
            sort: self.sort,
            // A window on any other sort is refused, since Steam ignores it.
            trend_days: (self.sort == BrowseSort::Trend)
                .then_some(self.trend_days)
                .flatten(),
            updated: self.updated_since.map(|start| TimeRange {
                start: Some(start),
                end: None,
            }),
            per_page: self.per_page,
            cursor: self.cursor.clone(),
            ..BrowseQuery::default()
        }
    }
}

/// Renders a byte count the way a person reads one.
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

/// Strips the HTML that Workshop titles and descriptions carry.
///
/// Steam stores what authors typed, which includes entities and the occasional
/// tag. Decoding and stripping have to alternate: an entity can decode *into* a
/// tag, so one pass of each in either order leaves visible markup behind.
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
        // Flattening them would ask for all-or-any across axes, which is a
        // different search that still returns a plausible page.
        let mut filters = Filters::new();
        filters.chosen[0] = Some("Scene".to_owned());
        filters.chosen[2] = Some("Anime".to_owned());

        let query = filters.to_query();
        assert_eq!(
            query.tag_groups,
            vec![vec!["Scene".to_owned()], vec!["Anime".to_owned()]]
        );
        assert!(query.required_tags.is_empty());
    }

    #[test]
    fn a_trend_window_is_dropped_when_the_sort_is_not_trend() {
        // tapline refuses the combination, and a sidebar that leaves the
        // period set while switching sort would make the search fail.
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
        // Both are preferences about how to browse, not part of a search, and
        // resetting them on Clear is the kind of thing that gets sworn at.
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
        // &lt;b&gt; decodes into a tag, so decode-then-strip once leaves it.
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
