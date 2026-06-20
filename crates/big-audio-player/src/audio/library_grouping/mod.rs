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
    fn grouping_presets_are_distinct() {
        let presets = LibraryGrouping::preset_definitions();
        assert!(presets.len() >= 5);
        for (message_id, _) in presets {
            assert!(!message_id.is_empty());
        }
    }

    #[test]
    fn default_is_artist_album() {
        assert_eq!(LibraryGrouping::default(), LibraryGrouping::artist_album());
    }

    #[test]
    fn recency_from_index_round_trip() {
        for idx in 0..6 {
            let _r = AgeFilter::from_index(idx);
            let _ = AgeFilter::all();
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
    fn group_by_column_names_are_valid() {
        use super::GroupBy::*;
        for g in [Artist, AlbumArtist, Album, Genre, Year, Format] {
            assert!(!g.column().is_empty());
        }
    }

    #[test]
    fn group_by_none_column_is_empty() {
        assert!(GroupBy::None.column().is_empty());
    }
}
