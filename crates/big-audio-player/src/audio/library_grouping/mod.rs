//! Clementine-style 3-level hierarchical grouping for the library browser.
//!
//! Split by axis:
//! - `group_by`   — `GroupBy` enum (single column).
//! - `grouping`   — `LibraryGrouping` triple of levels + serde + presets.
//! - `age_filter` — `AgeFilter` "since" window.
//!
//! All three types are re-exported here so callers keep `use crate::audio::library_grouping::*`.

mod age_filter;
mod group_by;
mod grouping;

pub use age_filter::AgeFilter;
pub use group_by::GroupBy;
pub use grouping::LibraryGrouping;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::browser_node::BrowserNodeKind;

    #[test]
    fn group_by_round_trip() {
        let variants = [
            GroupBy::None,
            GroupBy::Artist,
            GroupBy::AlbumArtist,
            GroupBy::Album,
            GroupBy::Genre,
            GroupBy::Year,
            GroupBy::Format,
        ];
        for g in variants {
            assert_eq!(GroupBy::from_str_token(g.as_str()), g);
        }
    }

    #[test]
    fn group_by_unknown_token() {
        assert_eq!(GroupBy::from_str_token("unknown"), GroupBy::None);
    }

    #[test]
    fn grouping_preset_definitions_match_expected_order_and_levels() {
        let preset_definitions = LibraryGrouping::preset_definitions();
        let expected_presets = [
            ("Songs", [GroupBy::None, GroupBy::None, GroupBy::None]),
            (
                "Artist → Album",
                [GroupBy::Artist, GroupBy::Album, GroupBy::None],
            ),
            ("Album", [GroupBy::Album, GroupBy::None, GroupBy::None]),
            (
                "Genre → Artist → Album",
                [GroupBy::Genre, GroupBy::Artist, GroupBy::Album],
            ),
            (
                "Genre → Artist",
                [GroupBy::Genre, GroupBy::Artist, GroupBy::None],
            ),
            (
                "Year → Album",
                [GroupBy::Year, GroupBy::Album, GroupBy::None],
            ),
            ("Genre", [GroupBy::Genre, GroupBy::None, GroupBy::None]),
            ("Year", [GroupBy::Year, GroupBy::None, GroupBy::None]),
        ];

        assert_eq!(preset_definitions.len(), expected_presets.len());
        for ((message_id, grouping), (expected_message_id, expected_levels)) in
            preset_definitions.iter().zip(expected_presets)
        {
            assert_eq!(*message_id, expected_message_id);
            assert_eq!(grouping.levels, expected_levels);
        }
    }

    #[test]
    fn default_is_artist_album() {
        assert_eq!(LibraryGrouping::default(), LibraryGrouping::artist_album());
    }

    #[test]
    fn age_filter_index_text_and_seconds_contract() {
        let expected_filters = [
            (0, AgeFilter::AllTime, "All Time", None),
            (1, AgeFilter::Today, "Today", Some(86_400)),
            (2, AgeFilter::ThisWeek, "This Week", Some(604_800)),
            (3, AgeFilter::ThisMonth, "This Month", Some(2_592_000)),
            (4, AgeFilter::ThreeMonths, "3 Months", Some(7_776_000)),
            (5, AgeFilter::ThisYear, "This Year", Some(31_536_000)),
        ];

        assert_eq!(AgeFilter::all().len(), expected_filters.len());
        for (index, expected_filter, expected_message_id, expected_seconds) in expected_filters {
            assert_eq!(AgeFilter::from_index(index), expected_filter);
            assert_eq!(AgeFilter::all()[index as usize], expected_filter);
            assert_eq!(expected_filter.display_name_msgid(), expected_message_id);
            assert_eq!(expected_filter.display_name(), expected_message_id);
            assert_eq!(expected_filter.seconds_ago(), expected_seconds);
        }
    }

    #[test]
    fn grouping_serialize_round_trip() {
        let g = LibraryGrouping::artist_album();
        let s = g.serialize();
        let g2 = LibraryGrouping::deserialize(&s);
        assert_eq!(g, g2);
    }

    #[test]
    fn grouping_deserialize_partial_input() {
        let g = LibraryGrouping::deserialize("artist");
        assert_eq!(g.level(0), GroupBy::Artist);
        assert_eq!(g.level(1), GroupBy::None);
        assert_eq!(g.level(2), GroupBy::None);
    }

    #[test]
    fn grouping_deserialize_empty() {
        let g = LibraryGrouping::deserialize("");
        assert_eq!(g.depth(), 0);
    }

    #[test]
    fn grouping_depth() {
        assert_eq!(LibraryGrouping::artist_album().depth(), 2);
        assert_eq!(LibraryGrouping::songs().depth(), 0);
        assert_eq!(LibraryGrouping::genre_artist_album().depth(), 3);
    }

    #[test]
    fn grouping_level_out_of_bounds() {
        let g = LibraryGrouping::artist_album();
        assert_eq!(g.level(3), GroupBy::None);
    }

    #[test]
    fn age_filter_all_time_no_threshold() {
        assert!(AgeFilter::AllTime.seconds_ago().is_none());
    }

    #[test]
    fn age_filter_today_is_one_day() {
        assert_eq!(AgeFilter::Today.seconds_ago(), Some(86_400));
    }

    #[test]
    fn age_filter_from_index_default() {
        assert!(matches!(AgeFilter::from_index(99), AgeFilter::AllTime));
    }

    #[test]
    fn age_filter_all_variants_covered() {
        let all = AgeFilter::all();
        assert_eq!(all.len(), 6);
        for a in all {
            assert!(!a.display_name_msgid().is_empty());
        }
    }

    #[test]
    fn group_by_metadata_contract_covers_every_variant() {
        let expected_groups = [
            (GroupBy::None, "", "None", "", BrowserNodeKind::All, "none"),
            (
                GroupBy::Artist,
                "artist",
                "Artist",
                "avatar-default-symbolic",
                BrowserNodeKind::Artist,
                "artist",
            ),
            (
                GroupBy::AlbumArtist,
                "album_artist",
                "Album Artist",
                "avatar-default-symbolic",
                BrowserNodeKind::Artist,
                "album_artist",
            ),
            (
                GroupBy::Album,
                "album",
                "Album",
                "media-optical-symbolic",
                BrowserNodeKind::Album,
                "album",
            ),
            (
                GroupBy::Genre,
                "genre",
                "Genre",
                "audio-x-generic-symbolic",
                BrowserNodeKind::Genre,
                "genre",
            ),
            (
                GroupBy::Year,
                "year",
                "Year",
                "x-office-calendar-symbolic",
                BrowserNodeKind::Year,
                "year",
            ),
            (
                GroupBy::Format,
                "format",
                "Format",
                "document-properties-symbolic",
                BrowserNodeKind::File,
                "format",
            ),
        ];

        for (
            group,
            expected_column,
            expected_label,
            expected_icon,
            expected_node,
            expected_token,
        ) in expected_groups
        {
            assert_eq!(group.column(), expected_column);
            assert_eq!(group.display_name(), expected_label);
            assert_eq!(group.icon_name(), expected_icon);
            assert_eq!(group.node_kind(), expected_node);
            assert_eq!(group.as_str(), expected_token);
            assert_eq!(GroupBy::from_str_token(expected_token), group);
        }
    }

    #[test]
    fn translated_presets_keep_definition_order_and_grouping() {
        let preset_definitions = LibraryGrouping::preset_definitions();
        let translated_presets = LibraryGrouping::presets();

        assert_eq!(translated_presets.len(), preset_definitions.len());
        for ((translated_label, translated_grouping), (message_id, expected_grouping)) in
            translated_presets.iter().zip(preset_definitions)
        {
            assert_eq!(translated_label, message_id);
            assert_eq!(*translated_grouping, expected_grouping);
        }
    }
}
