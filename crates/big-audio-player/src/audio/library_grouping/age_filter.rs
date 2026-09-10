//! `AgeFilter`: "since" window used by the library browser to constrain rows.
//! Independent of [`super::GroupBy`]; lives next to it because both feed the same
//! query builder.

use crate::i18n::{gettext_noop, i18n};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgeFilter {
    #[default]
    AllTime,
    Today,
    ThisWeek,
    ThisMonth,
    ThreeMonths,
    ThisYear,
}

impl AgeFilter {
    #[must_use]
    pub fn display_name(self) -> String {
        i18n(self.display_name_msgid())
    }

    #[must_use]
    pub(super) const fn display_name_msgid(self) -> &'static str {
        match self {
            Self::AllTime => gettext_noop("All Time"),
            Self::Today => gettext_noop("Today"),
            Self::ThisWeek => gettext_noop("This Week"),
            Self::ThisMonth => gettext_noop("This Month"),
            Self::ThreeMonths => gettext_noop("3 Months"),
            Self::ThisYear => gettext_noop("This Year"),
        }
    }

    /// `None` = no filter.
    #[must_use]
    pub fn seconds_ago(self) -> Option<i64> {
        match self {
            Self::AllTime => None,
            Self::Today => Some(86_400),
            Self::ThisWeek => Some(604_800),
            Self::ThisMonth => Some(2_592_000),
            Self::ThreeMonths => Some(7_776_000),
            Self::ThisYear => Some(31_536_000),
        }
    }

    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::AllTime,
            Self::Today,
            Self::ThisWeek,
            Self::ThisMonth,
            Self::ThreeMonths,
            Self::ThisYear,
        ]
    }

    #[must_use]
    pub fn from_index(idx: u32) -> Self {
        match idx {
            1 => Self::Today,
            2 => Self::ThisWeek,
            3 => Self::ThisMonth,
            4 => Self::ThreeMonths,
            5 => Self::ThisYear,
            _ => Self::AllTime,
        }
    }
}
