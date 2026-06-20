//! `LibraryGrouping`: ordered triple of [`GroupBy`] levels driving the 3-level
//! Clementine-style browser tree. Carries preset constructors + serde helpers
//! used by the GSettings string key.

use crate::i18n::i18n;

use super::group_by::GroupBy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryGrouping {
    pub levels: [GroupBy; 3],
}

impl Default for LibraryGrouping {
    fn default() -> Self {
        Self::artist_album()
    }
}

impl LibraryGrouping {
    #[must_use]
    pub fn artist_album() -> Self {
        Self {
            levels: [GroupBy::Artist, GroupBy::Album, GroupBy::None],
        }
    }

    #[must_use]
    pub fn album() -> Self {
        Self {
            levels: [GroupBy::Album, GroupBy::None, GroupBy::None],
        }
    }

    #[must_use]
    pub fn genre_artist_album() -> Self {
        Self {
            levels: [GroupBy::Genre, GroupBy::Artist, GroupBy::Album],
        }
    }

    #[must_use]
    pub fn year_album() -> Self {
        Self {
            levels: [GroupBy::Year, GroupBy::Album, GroupBy::None],
        }
    }

    #[must_use]
    pub fn genre() -> Self {
        Self {
            levels: [GroupBy::Genre, GroupBy::None, GroupBy::None],
        }
    }

    #[must_use]
    pub fn year() -> Self {
        Self {
            levels: [GroupBy::Year, GroupBy::None, GroupBy::None],
        }
    }

    #[must_use]
    pub fn genre_artist() -> Self {
        Self {
            levels: [GroupBy::Genre, GroupBy::Artist, GroupBy::None],
        }
    }

    #[must_use]
    pub fn songs() -> Self {
        Self {
            levels: [GroupBy::None, GroupBy::None, GroupBy::None],
        }
    }

    #[must_use]
    pub fn preset_definitions() -> Vec<(&'static str, Self)> {
        vec![
            ("Songs", Self::songs()),
            ("Artist → Album", Self::artist_album()),
            ("Album", Self::album()),
            ("Genre → Artist → Album", Self::genre_artist_album()),
            ("Genre → Artist", Self::genre_artist()),
            ("Year → Album", Self::year_album()),
            ("Genre", Self::genre()),
            ("Year", Self::year()),
        ]
    }

    #[must_use]
    pub fn presets() -> Vec<(String, Self)> {
        Self::preset_definitions()
            .into_iter()
            .map(|(message_id, grouping)| (i18n(message_id), grouping))
            .collect()
    }

    #[must_use]
    pub fn depth(&self) -> usize {
        self.levels.iter().filter(|l| **l != GroupBy::None).count()
    }

    #[must_use]
    pub fn level(&self, idx: usize) -> GroupBy {
        if idx < 3 {
            self.levels[idx]
        } else {
            GroupBy::None
        }
    }

    /// Format: `"artist/album/none"`.
    #[must_use]
    pub fn serialize(&self) -> String {
        format!(
            "{}/{}/{}",
            self.levels[0].as_str(),
            self.levels[1].as_str(),
            self.levels[2].as_str(),
        )
    }

    #[must_use]
    pub fn deserialize(s: &str) -> Self {
        let parts: Vec<&str> = s.split('/').collect();
        let l0 = parts
            .first()
            .map_or(GroupBy::None, |s| GroupBy::from_str_token(s));
        let l1 = parts
            .get(1)
            .map_or(GroupBy::None, |s| GroupBy::from_str_token(s));
        let l2 = parts
            .get(2)
            .map_or(GroupBy::None, |s| GroupBy::from_str_token(s));
        Self {
            levels: [l0, l1, l2],
        }
    }
}
