//! Grouped IPTV channel list (Relm4 port).
//!
//! Groups channels by `group-title` into collapsible `AdwExpanderRow`; each
//! channel is an `AdwActionRow` with an optional logo (lazy `logos::fetch`)
//! and a favorite-toggle suffix. The view is a plain GTK `ListBox` driven by
//! the [`VideoIptvBrowser`] component: the component
//! pushes channels in and listens for play / favorite-toggle intents via the
//! relm4 sender, so this stays a thin, video-local widget builder (relm4's
//! `factory` feature is off in this workspace).
//!
//! Mirrors the legacy `iptv_view::channel_list` until the V6 cutover deletes it,
//! but reports user actions back through a relm4 [`ComponentSender`] instead of
//! `Rc<RefCell<Option<Box<dyn Fn>>>>` callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use big_relm4_components::list::button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec};
use big_relm4_components::list::info_row::{BigInfoRow, BigInfoRowSpec};
use relm4::ComponentSender;

use crate::i18n::{i18n, ni18n};
use crate::media::iptv::IptvChannel;
use crate::video::ui::iptv::favorites::IptvFavorites;
use crate::video::ui::iptv::logos;
use crate::video::ui::iptv::{VideoIptvBrowser, VideoIptvBrowserInput};
use big_app_kit::http_client::RequestHeaders;

/// Grouped channel list widget plus the state needed to rebuild it on filter or
/// channel-set changes. Owned by the component; not a relm4 component itself.
pub struct ChannelListView {
    /// Root `ListBox` mounted by the browser component.
    pub root: gtk::ListBox,
    all_channels: Rc<RefCell<Vec<IptvChannel>>>,
    favorites: Rc<RefCell<IptvFavorites>>,
    user_agent: Rc<RefCell<String>>,
    referer: Rc<RefCell<String>>,
    sender: ComponentSender<VideoIptvBrowser>,
}

impl ChannelListView {
    #[must_use]
    pub fn new(sender: ComponentSender<VideoIptvBrowser>) -> Self {
        let root = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        Self {
            root,
            all_channels: Rc::new(RefCell::new(Vec::new())),
            favorites: Rc::new(RefCell::new(IptvFavorites::load())),
            user_agent: Rc::new(RefCell::new(String::new())),
            referer: Rc::new(RefCell::new(String::new())),
            sender,
        }
    }

    /// Set the per-provider HTTP headers used for logo fetches.
    pub fn set_headers(&self, user_agent: &str, referer: &str) {
        self.user_agent.replace(user_agent.to_string());
        self.referer.replace(referer.to_string());
    }

    /// Replace the channel set and rebuild the (unfiltered) list.
    pub fn set_channels(&self, channels: Vec<IptvChannel>) {
        self.all_channels.replace(channels);
        self.rebuild("");
    }

    /// Re-render the list filtered by `query` (matches name or group).
    pub fn apply_filter(&self, query: &str) {
        self.rebuild(query);
    }

    /// Re-read the favorites file → mirror cross-instance star toggles.
    pub fn reload_favorites(&self) {
        self.favorites.replace(IptvFavorites::load());
    }

    fn rebuild(&self, query: &str) {
        while let Some(child) = self.root.first_child() {
            self.root.remove(&child);
        }

        let query_lower = query.trim().to_lowercase();
        let channels = self.all_channels.borrow();
        let filtered: Vec<&IptvChannel> = channels
            .iter()
            .filter(|c| {
                query_lower.is_empty()
                    || c.name.to_lowercase().contains(&query_lower)
                    || c.group.to_lowercase().contains(&query_lower)
            })
            .collect();

        if filtered.is_empty() {
            let row = BigInfoRow::new(
                BigInfoRowSpec::new(i18n("No channels"))
                    .subtitle(i18n("Try a different search or refresh the list")),
            )
            .into_root();
            self.root.append(&row);
            return;
        }

        for (group_name, list) in group_channels(&filtered) {
            let subtitle = ni18n("{} channel", "{} channels", list.len() as u64)
                .replace("{}", &list.len().to_string());
            let expander = adw::ExpanderRow::builder()
                .title(&group_name)
                .subtitle(&subtitle)
                .build();
            for ch in list {
                expander.add_row(&self.build_channel_row(ch));
            }
            self.root.append(&expander);
        }
    }

    fn build_channel_row(&self, channel: &IptvChannel) -> adw::ActionRow {
        let mut spec = BigButtonStripRowSpec::new(&channel.name).activatable(true);
        if !channel.group.is_empty() {
            spec = spec.subtitle(&channel.group);
        }
        let row_builder = BigButtonStripRow::new(spec);

        self.attach_logo(&row_builder, channel);
        self.attach_favorite_button(&row_builder, channel);

        let url = channel.url.clone();
        let name = channel.name.clone();
        let sender = self.sender.clone();
        let row = row_builder.into_root();
        row.connect_activated(move |_| {
            sender.input(VideoIptvBrowserInput::PlayChannel {
                url: url.clone(),
                title: name.clone(),
            });
        });

        row
    }

    /// Show the cached logo immediately, else a placeholder icon plus a lazy
    /// fetch that swaps the placeholder once the logo lands.
    fn attach_logo(&self, row_builder: &BigButtonStripRow, channel: &IptvChannel) {
        if let Some(path) = logos::cached(&channel.logo) {
            row_builder
                .root()
                .add_prefix(&thumbnail_image_from_path(&path));
            return;
        }
        let placeholder = gtk::Image::builder()
            .icon_name("video-x-generic-symbolic")
            .pixel_size(32)
            .build();
        row_builder.root().add_prefix(&placeholder);
        if channel.logo.is_empty() {
            return;
        }
        let placeholder_weak = placeholder.downgrade();
        let row_weak = row_builder.root().downgrade();
        let headers = RequestHeaders {
            user_agent: &self.user_agent.borrow(),
            referer: &self.referer.borrow(),
        };
        logos::fetch(&channel.logo, &headers, move |file| {
            if let Some(row) = row_weak.upgrade() {
                if let Some(placeholder) = placeholder_weak.upgrade() {
                    row.remove(&placeholder);
                }
                row.add_prefix(&thumbnail_image_from_file(&file));
            }
        });
    }

    /// Append the favorite-toggle star; toggling persists and notifies the
    /// component so other views can re-sync.
    fn attach_favorite_button(&self, row_builder: &BigButtonStripRow, channel: &IptvChannel) {
        let is_favorite = self.favorites.borrow().contains(&channel.url);
        let fav_btn = tooltip::icon_toggle_button(
            star_icon(is_favorite),
            &i18n("Toggle favorite"),
            is_favorite,
            &["flat", "circular"],
        );
        fav_btn.set_valign(gtk::Align::Center);
        let url = channel.url.clone();
        let favorites = self.favorites.clone();
        let sender = self.sender.clone();
        fav_btn.connect_toggled(move |btn| {
            let now_starred = favorites.borrow_mut().toggle(&url);
            btn.set_icon_name(star_icon(now_starred));
            sender.input(VideoIptvBrowserInput::FavoriteToggled);
        });
        row_builder.append(&fav_btn);
    }
}

/// Group filtered channels by `group-title`, preserving first-seen order;
/// empty groups fold into a single translated "Ungrouped" bucket.
fn group_channels<'a>(filtered: &[&'a IptvChannel]) -> Vec<(String, Vec<&'a IptvChannel>)> {
    let mut grouped: Vec<(String, Vec<&IptvChannel>)> = Vec::new();
    for ch in filtered {
        let key = if ch.group.is_empty() {
            i18n("Ungrouped")
        } else {
            ch.group.clone()
        };
        match grouped.iter_mut().find(|(k, _)| k == &key) {
            Some((_, list)) => list.push(ch),
            None => grouped.push((key, vec![ch])),
        }
    }
    grouped
}

fn star_icon(is_favorite: bool) -> &'static str {
    if is_favorite {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    }
}

fn thumbnail_image_from_path(path: &std::path::Path) -> gtk::Image {
    let img = gtk::Image::builder().pixel_size(40).build();
    img.set_from_file(Some(path));
    img
}

fn thumbnail_image_from_file(file: &gtk::gio::File) -> gtk::Image {
    let img = gtk::Image::builder().pixel_size(40).build();
    if let Some(path) = file.path() {
        img.set_from_file(Some(&path));
    }
    img
}
