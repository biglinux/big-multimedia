//! Layout subsection of the appearance dialog: buttons page (the shared
//! `BigZoneLayoutEditor`) and layout page.

mod buttons_page;
mod layout_page;

pub(in crate::video) use buttons_page::create_buttons_page;
pub(in crate::video) use layout_page::create_layout_page;
