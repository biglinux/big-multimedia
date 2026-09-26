//! The settings panel: one translucent card on the right, between the header
//! and the controls, in place of a menu of separate dialogs. Its first page
//! is a grid of blocks (Picture, Sound, Chapters, …) that each say, in
//! words, how that part is set now; a block opens its section in the same
//! card, with "Back". A shortcut, a chip or the right-click menu opens a
//! section directly, and then Esc closes the whole panel.
//!
//! What the controls bar already offers (volume, speed, tracks, subtitles,
//! voice) is not repeated here: one place for each thing.
//!
//! Pages are rebuilt from mpv and `Settings` each time they open, so they
//! never show a stale value.

use super::adjustments::{Adjustment, detect};
use super::*;
use crate::video::ui::menu_dialogs::chapter_title;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelPage {
    Home,
    Hdr,
    Picture,
    Framing,
    Effects,
    Sound,
    Chapters,
    SleepTimer,
}

impl PanelPage {
    fn title(self) -> String {
        match self {
            Self::Home => i18n("Settings"),
            Self::Hdr => i18n("HDR"),
            Self::Picture => i18n("Picture"),
            Self::Framing => i18n("Zoom and position"),
            Self::Effects => i18n("Effects and filters"),
            Self::Sound => i18n("Equalizer"),
            Self::Chapters => i18n("Chapters"),
            Self::SleepTimer => i18n("Stop after…"),
        }
    }

    /// Where "Back" leads.
    fn parent(self) -> Option<Self> {
        match self {
            Self::Home => None,
            _ => Some(Self::Home),
        }
    }
}

/// Below this the panel covers the controls instead of shrinking.
const PANEL_MIN_HEIGHT: i32 = 300;

/// Where the panel sits and how its home page is laid out in a window of
/// `width` × `height` whose header bar and controls take `header` and
/// `controls` pixels.
#[derive(Debug, PartialEq, Eq)]
struct PanelLayout {
    width: i32,
    columns: usize,
    top: i32,
    bottom: i32,
    /// Short window: smaller blocks and spacing.
    compact: bool,
}

fn panel_layout(width: i32, height: i32, header: i32, controls: i32) -> PanelLayout {
    // Just below the header, where the settings button is.
    let top = header + 4;
    // A short window would squeeze the card to a sliver: then it covers the
    // controls rather than shrink below a usable height.
    let bottom = (controls + 6).min(height - top - PANEL_MIN_HEIGHT).max(8);
    let room = width - 16;
    let (columns, card) = if room >= 940 {
        (3, 620)
    } else if room >= 620 {
        (2, 440)
    } else if room >= 380 {
        (2, room)
    } else {
        (1, room)
    };
    PanelLayout {
        width: card.max(0),
        columns,
        top,
        bottom,
        compact: height < 600,
    }
}

pub(super) struct SettingsPanel {
    pub(super) revealer: gtk::Revealer,
    card: gtk::Box,
    title: gtk::Label,
    back: gtk::Button,
    scroller: gtk::ScrolledWindow,
    page: Cell<PanelPage>,
    /// Block columns on the home page: three when the window is wide.
    columns: Cell<usize>,
    /// Opened straight at a section: Esc closes instead of going back.
    direct: Cell<bool>,
    effects: RefCell<Option<Controller<VideoFiltersDialog>>>,
}

impl SettingsPanel {
    pub(super) fn new(sender: &ComponentSender<VideoShell>) -> Rc<Self> {
        let back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text(i18n("Back"))
            .css_classes(["flat", "circular"])
            .build();
        back.update_property(&[gtk::accessible::Property::Label(&i18n("Back"))]);
        let title = gtk::Label::builder()
            .hexpand(true)
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        let close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(i18n("Close"))
            .css_classes(["flat", "circular"])
            .build();
        close.update_property(&[gtk::accessible::Property::Label(&i18n("Close"))]);
        let top = gtk::Box::builder()
            .spacing(6)
            .margin_top(8)
            .margin_start(8)
            .margin_end(8)
            .build();
        top.append(&back);
        top.append(&title);
        top.append(&close);

        // The card hugs its content and scrolls only when the window is too
        // short for it (`max_content_height` is set per window on open).
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .build();
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["settings-panel"])
            .build();
        card.append(&top);
        card.append(&scroller);
        card.update_property(&[gtk::accessible::Property::Label(&i18n("Settings"))]);

        let revealer = gtk::Revealer::builder()
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            .transition_duration(150)
            .margin_end(8)
            .child(&card)
            .build();

        back.connect_clicked({
            let sender = sender.clone();
            move |_| sender.input(VideoShellInput::PanelBack)
        });
        close.connect_clicked({
            let sender = sender.clone();
            move |_| sender.input(VideoShellInput::ClosePanel)
        });
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let sender = sender.clone();
            move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    sender.input(VideoShellInput::PanelEscape);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        });
        card.add_controller(keys);

        Rc::new(Self {
            revealer,
            card,
            title,
            back,
            scroller,
            page: Cell::new(PanelPage::Home),
            columns: Cell::new(2),
            direct: Cell::new(false),
            effects: RefCell::new(None),
        })
    }

    pub(super) fn is_open(&self) -> bool {
        self.revealer.reveals_child()
    }
}

/// One home block: what it is, how it is set now, and whether that is a
/// change from normal (then a dot and bold text say so, not colour alone).
struct Block {
    icon: &'static str,
    name: String,
    state: String,
    changed: bool,
    target: BlockTarget,
}

enum BlockTarget {
    Page(PanelPage),
    Action(&'static str),
}

/// The home block (by page) an adjustment belongs to.
fn page_of(adjustment: &Adjustment) -> Option<PanelPage> {
    match adjustment {
        Adjustment::SourceColours(_) => Some(PanelPage::Hdr),
        Adjustment::Picture(_) => Some(PanelPage::Picture),
        Adjustment::Zoom { .. }
        | Adjustment::Rotation(_)
        | Adjustment::Flip { .. }
        | Adjustment::Crop { .. } => Some(PanelPage::Framing),
        Adjustment::VideoEffects(_) | Adjustment::ImageEnhancement(_) => Some(PanelPage::Effects),
        Adjustment::Equalizer { .. } | Adjustment::AudioDelay(_) => Some(PanelPage::Sound),
        _ => None,
    }
}

/// "Normal", the one change by name, or how many there are.
fn summary(changes: &[&Adjustment], normal: String) -> (String, bool) {
    match changes {
        [] => (normal, false),
        [only] => (only.label(), true),
        many => (
            ni18n("{n} change", "{n} changes", many.len() as u64)
                .replace("{n}", &many.len().to_string()),
            true,
        ),
    }
}

fn group(title: &str, description: Option<&str>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title(title).build();
    if let Some(description) = description {
        group.set_description(Some(description));
    }
    group
}

fn page_box() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .build()
}

impl VideoShell {
    fn panel_mpv(&self) -> Option<Arc<MpvBackend>> {
        self.mpv.borrow().clone()
    }

    /// Open (or switch) the panel at `page`; `direct` when a shortcut, chip or
    /// menu asked for that section itself.
    pub(super) fn show_panel(
        &mut self,
        page: PanelPage,
        direct: bool,
        sender: &ComponentSender<Self>,
    ) {
        let Some(mpv) = self.panel_mpv() else {
            return;
        };
        let panel = self.panel.clone();
        if !panel.is_open() {
            // The header bar itself: in the gradient look its box also holds
            // 80 px of fade, and so does the top of the controls.
            let natural = |widget: Option<gtk::Widget>| {
                widget.map_or(0, |widget| widget.measure(gtk::Orientation::Vertical, -1).1)
            };
            let fade = if self.settings.get_string(keys::UI_COLOR_SOURCE) == "gradient" {
                80
            } else {
                0
            };
            let layout = panel_layout(
                self.main_overlay.width(),
                self.main_overlay.height(),
                natural(
                    self.osd
                        .header_revealer
                        .child()
                        .and_then(|b| b.first_child()),
                ),
                natural(self.osd.controls_revealer.child()) - fade,
            );
            panel.columns.set(layout.columns);
            panel.card.set_width_request(layout.width);
            panel.revealer.set_margin_top(layout.top);
            // Room below the card's own title row.
            panel.scroller.set_max_content_height(
                (self.main_overlay.height() - layout.top - layout.bottom - 52).max(120),
            );
            if layout.compact {
                panel.card.add_css_class("compact");
            } else {
                panel.card.remove_css_class("compact");
            }
        }
        let content = match page {
            PanelPage::Home => self.home_page(&mpv, sender),
            PanelPage::Hdr => media_adjust::build_hdr_section(&mpv, &self.video_transform),
            PanelPage::Picture => {
                media_adjust::build_picture_section(&mpv, panel.columns.get() >= 3)
            }
            PanelPage::Framing => media_adjust::build_framing_section(&mpv, &self.video_transform),
            PanelPage::Effects => self.effects_page(&mpv),
            PanelPage::Sound => self.sound_page(&mpv),
            PanelPage::Chapters => chapters_page(&mpv, sender),
            PanelPage::SleepTimer => self.sleep_timer_page(sender),
        };
        if page != PanelPage::Effects {
            panel.effects.take();
        }
        let wrapper = page_box();
        wrapper.append(&content);
        panel.scroller.set_child(Some(&wrapper));
        panel.title.set_label(&page.title());
        panel.back.set_visible(page != PanelPage::Home);
        if !panel.is_open() {
            panel.direct.set(direct);
            self.osd.show();
            panel.revealer.set_reveal_child(true);
        }
        panel.page.set(page);
        panel.scroller.vadjustment().set_value(0.0);
        glib::idle_add_local_once(move || {
            if !panel.scroller.child_focus(gtk::DirectionType::TabForward) {
                panel.card.grab_focus();
            }
        });
    }

    pub(super) fn close_panel(&mut self, sender: &ComponentSender<Self>) {
        if !self.panel.is_open() {
            return;
        }
        self.panel.revealer.set_reveal_child(false);
        self.panel.scroller.set_child(None::<&gtk::Widget>);
        self.panel.effects.take();
        self.last_reset = None;
        self.osd.show_and_schedule_hide();
        self.refresh_adjustments(sender);
    }

    /// Redraw the home page if it is showing (a reset or undo changed it).
    pub(super) fn refresh_panel_home(&mut self, sender: &ComponentSender<Self>) {
        if self.panel.is_open() && self.panel.page.get() == PanelPage::Home {
            self.show_panel(PanelPage::Home, false, sender);
        }
    }

    pub(super) fn panel_back(&mut self, escape: bool, sender: &ComponentSender<Self>) {
        let parent = self.panel.page.get().parent();
        match parent {
            Some(page) if !(escape && self.panel.direct.get()) => {
                self.show_panel(page, false, sender);
            }
            _ => self.close_panel(sender),
        }
    }

    pub(super) fn toggle_panel(&mut self, sender: &ComponentSender<Self>) {
        if self.panel.is_open() {
            self.close_panel(sender);
        } else {
            self.show_panel(PanelPage::Home, false, sender);
        }
    }

    fn home_page(&self, mpv: &MpvBackend, sender: &ComponentSender<Self>) -> gtk::Widget {
        let adjustments = detect(mpv, &self.settings, &self.video_transform);
        let block = |icon, name: String, page| {
            let changes: Vec<&Adjustment> = adjustments
                .iter()
                .filter(|a| page_of(a) == Some(page))
                .collect();
            let (state, changed) = summary(&changes, i18n("Normal"));
            Block {
                icon,
                name,
                state,
                changed,
                target: BlockTarget::Page(page),
            }
        };
        let hdr = block("weather-clear-symbolic", i18n("HDR"), PanelPage::Hdr);
        let hdr = Block {
            // Say which reading is in use even when it is the automatic one.
            state: if hdr.changed {
                hdr.state
            } else {
                media_adjust::source_colour_names()[0].clone()
            },
            ..hdr
        };
        let mut blocks = vec![
            hdr,
            block(
                "image-x-generic-symbolic",
                i18n("Picture"),
                PanelPage::Picture,
            ),
            block(
                "zoom-in-symbolic",
                i18n("Zoom and position"),
                PanelPage::Framing,
            ),
            block(
                "applications-graphics-symbolic",
                i18n("Effects and filters"),
                PanelPage::Effects,
            ),
            block(
                "audio-speakers-symbolic",
                i18n("Equalizer"),
                PanelPage::Sound,
            ),
        ];
        let chapters = mpv.get_chapter_count();
        if chapters > 0 {
            blocks.push(Block {
                icon: "view-list-bullet-symbolic",
                name: i18n("Chapters"),
                state: i18n("{current} of {total}")
                    .replace("{current}", &(mpv.get_current_chapter() + 1).to_string())
                    .replace("{total}", &chapters.to_string()),
                changed: false,
                target: BlockTarget::Page(PanelPage::Chapters),
            });
        }
        let timer = self
            .sleep_timer_is_armed()
            .then(|| Adjustment::SleepTimer(self.sleep_timer_remaining_secs()));
        blocks.push(Block {
            icon: "alarm-symbolic",
            name: i18n("Stop after…"),
            state: timer
                .as_ref()
                .map_or_else(|| i18n("Off"), Adjustment::label),
            changed: timer.is_some(),
            target: BlockTarget::Page(PanelPage::SleepTimer),
        });
        blocks.push(Block {
            icon: "tv-symbolic",
            name: i18n("Play on a TV"),
            state: i18n("Chromecast or DLNA"),
            changed: false,
            target: BlockTarget::Action("win.cast"),
        });
        if self
            .win_actions
            .lookup_action("ytdl-quality")
            .is_some_and(|action| action.is_enabled())
        {
            blocks.push(Block {
                icon: "network-wireless-symbolic",
                name: i18n("Online quality"),
                state: i18n("Choose"),
                changed: false,
                target: BlockTarget::Action("win.ytdl-quality"),
            });
        }
        let (width, height) = (
            mpv.get_property_f64("width"),
            mpv.get_property_f64("height"),
        );
        blocks.push(Block {
            icon: "dialog-information-symbolic",
            name: i18n("About this video"),
            state: if width > 0.0 {
                format!("{width:.0} × {height:.0}")
            } else {
                String::new()
            },
            changed: false,
            target: BlockTarget::Action("win.media-info"),
        });

        let grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .column_homogeneous(true)
            .build();
        for (index, block) in blocks.into_iter().enumerate() {
            let columns = self.panel.columns.get();
            let column = i32::try_from(index % columns).unwrap_or(0);
            let row = i32::try_from(index / columns).unwrap_or(0);
            grid.attach(&block_button(block, sender), column, row, 1, 1);
        }

        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        page.append(&grid);
        if !adjustments.is_empty() {
            page.prepend(&Self::changes_group(&adjustments, sender));
        }
        if let Some(undone) = self.last_reset.clone() {
            let undo = gtk::Button::builder()
                .label(i18n("Undo"))
                .tooltip_text(i18n("Put back what was just set to normal"))
                .css_classes(["adjustment-reset"])
                .halign(gtk::Align::Center)
                .build();
            let sender = sender.clone();
            undo.connect_clicked(move |_| {
                sender.input(VideoShellInput::RestoreAdjustments(undone.clone()));
            });
            page.append(&undo);
        }

        // The player's own settings: one compact row of links, so the
        // video's blocks stay the first thing seen.
        let app = gtk::Grid::builder()
            .column_spacing(6)
            .row_spacing(6)
            .column_homogeneous(true)
            .build();
        let per_row = if self.panel.columns.get() >= 3 { 4 } else { 2 };
        for (index, (icon, label, action)) in [
            (
                "preferences-system-symbolic",
                i18n("Preferences"),
                "win.preferences",
            ),
            (
                "preferences-desktop-appearance-symbolic",
                i18n("Appearance"),
                "win.appearance",
            ),
            (
                "input-keyboard-symbolic",
                i18n("Shortcuts"),
                "win.keyboard-shortcuts",
            ),
            ("help-about-symbolic", i18n("About"), "app.about"),
        ]
        .into_iter()
        .enumerate()
        {
            let content = gtk::Box::builder()
                .spacing(6)
                .halign(gtk::Align::Center)
                .build();
            content.append(&gtk::Image::from_icon_name(icon));
            content.append(
                &gtk::Label::builder()
                    .label(&label)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build(),
            );
            let link = gtk::Button::builder()
                .child(&content)
                .tooltip_text(&label)
                .css_classes(["settings-link"])
                .build();
            link.update_property(&[gtk::accessible::Property::Label(&label)]);
            let sender = sender.clone();
            link.connect_clicked(move |link| {
                let _ = link.activate_action(action, None);
                sender.input(VideoShellInput::ClosePanel);
            });
            let column = i32::try_from(index % per_row).unwrap_or(0);
            let row = i32::try_from(index / per_row).unwrap_or(0);
            app.attach(&link, column, row, 1, 1);
        }
        page.append(&app);
        page.upcast()
    }

    /// "Changed": every adjustment by name; a row opens its section, its ×
    /// undoes it, and "Back to normal" undoes them all.
    fn changes_group(adjustments: &[Adjustment], sender: &ComponentSender<Self>) -> gtk::Widget {
        let changes = group(&i18n("Changed"), None);
        for adjustment in adjustments {
            let label = adjustment.label();
            let row = adw::ActionRow::builder().title(&label).build();
            if let Some(action) = adjustment.section_action() {
                row.set_activatable(true);
                row.connect_activated(move |row| {
                    let _ = row.activate_action(action, None);
                });
            }
            let undo = gtk::Button::builder()
                .icon_name("window-close-symbolic")
                .tooltip_text(i18n("Back to normal"))
                .valign(gtk::Align::Center)
                .css_classes(["flat", "circular"])
                .build();
            undo.update_property(&[gtk::accessible::Property::Label(
                &i18n("Undo: {setting}").replace("{setting}", &label),
            )]);
            let sender = sender.clone();
            let adjustment = adjustment.clone();
            undo.connect_clicked(move |_| {
                sender.input(match &adjustment {
                    Adjustment::SleepTimer(_) => VideoShellInput::SleepTimerCancel,
                    other => VideoShellInput::ResetAdjustments(vec![other.clone()]),
                });
            });
            row.add_suffix(&undo);
            changes.add(&row);
        }
        if adjustments
            .iter()
            .any(|a| !matches!(a, Adjustment::SleepTimer(_)))
        {
            let reset = gtk::Button::builder()
                .label(i18n("Back to normal"))
                .tooltip_text(i18n("Undo every change to the picture and sound"))
                .css_classes(["adjustment-reset"])
                .halign(gtk::Align::Center)
                .margin_top(8)
                .build();
            let sender = sender.clone();
            reset.connect_clicked(move |_| sender.input(VideoShellInput::ResetAllAdjustments));
            changes.add(&reset);
        }
        changes.upcast()
    }

    fn effects_page(&self, mpv: &Arc<MpvBackend>) -> gtk::Widget {
        let controller = VideoFiltersDialog::builder()
            .launch(VideoFiltersDialogInit {
                mpv: Arc::clone(mpv),
                settings: self.settings.clone(),
            })
            .detach();
        let widget: gtk::Widget = controller.widget().clone().upcast();
        self.panel.effects.replace(Some(controller));
        widget
    }

    fn sound_page(&self, mpv: &Arc<MpvBackend>) -> gtk::Widget {
        let page = page_box();
        page.set_margin_start(0);
        page.set_margin_end(0);
        page.append(&crate::shared::audio_effects::create_equalizer_content(
            &self.settings,
            Some(Arc::clone(mpv)),
            Some(Rc::new(|| {
                super::send_to_shell(VideoShellInput::RefreshAdjustments);
            })),
        ));
        page.append(&media_adjust::build_delay_row(mpv, false));
        page.upcast()
    }

    fn sleep_timer_page(&self, sender: &ComponentSender<Self>) -> gtk::Widget {
        use big_relm4_components::display::sleep_timer::{
            BigSleepTimerActiveLabels, BigSleepTimerActivePage, BigSleepTimerActiveSpec,
            BigSleepTimerSetupPage, BigSleepTimerSetupSpec, connect_sleep_timer_start_with_close,
        };
        let icon = || gtk::Image::from_icon_name("alarm-symbolic");
        if self.sleep_timer_is_armed() {
            let remaining = self.sleep_timer_remaining_secs();
            let active = BigSleepTimerActivePage::new(
                BigSleepTimerActiveSpec {
                    labels: BigSleepTimerActiveLabels {
                        title: i18n("Timer Active"),
                        cancel_button: i18n("Cancel Timer"),
                    },
                    remaining_seconds: remaining,
                    icon_size: 64,
                },
                Some(icon().upcast()),
            );
            active.start_preview_countdown(remaining);
            let sender = sender.clone();
            active.cancel_button().connect_clicked(move |_| {
                sender.input(VideoShellInput::SleepTimerCancel);
                sender.input(VideoShellInput::ClosePanel);
            });
            return active.into_page().upcast();
        }
        let (minutes, action) =
            crate::shared::sleep_timer::load_sleep_timer_preferences(Some(&self.settings));
        let setup = BigSleepTimerSetupPage::new(
            BigSleepTimerSetupSpec {
                labels: crate::shared::sleep_timer::sleep_timer_setup_labels(),
                actions: crate::shared::sleep_timer::sleep_timer_actions(),
                saved_minutes: minutes,
                saved_action_id: action,
                icon_size: 64,
            },
            Some(icon().upcast()),
            None,
            None,
        );
        let settings = self.settings.clone();
        let input = sender.input_sender().clone();
        let close = sender.input_sender().clone();
        connect_sleep_timer_start_with_close(
            &setup,
            &self.panel.card,
            move |_, minutes, action| {
                crate::shared::sleep_timer::save_sleep_timer_preferences(
                    Some(&settings),
                    minutes,
                    &action,
                );
                let _ = input.send(VideoShellInput::SleepTimerStart { minutes, action });
            },
            move || {
                let _ = close.send(VideoShellInput::ClosePanel);
            },
        );
        setup.into_page().upcast()
    }
}

fn block_button(block: Block, sender: &ComponentSender<VideoShell>) -> gtk::Button {
    let content = gtk::Box::builder().spacing(10).build();
    content.append(
        &gtk::Image::builder()
            .icon_name(block.icon)
            .pixel_size(22)
            .valign(gtk::Align::Center)
            .build(),
    );
    let texts = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .valign(gtk::Align::Center)
        .build();
    content.append(&texts);
    texts.append(
        &gtk::Label::builder()
            .label(&block.name)
            .xalign(0.0)
            .css_classes(["heading"])
            .build(),
    );
    let state = if block.changed {
        format!("● {}", block.state)
    } else {
        block.state.clone()
    };
    let state_label = gtk::Label::builder()
        .label(&state)
        .xalign(0.0)
        .wrap(true)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["block-state"])
        .build();
    if block.changed {
        state_label.add_css_class("block-changed");
    }
    texts.append(&state_label);
    let button = gtk::Button::builder()
        .child(&content)
        .css_classes(["settings-block"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&format!(
        "{}: {}",
        block.name, block.state
    ))]);
    let sender = sender.clone();
    match block.target {
        BlockTarget::Page(page) => {
            button.connect_clicked(move |_| {
                sender.input(VideoShellInput::PanelNavigate(page));
            });
        }
        BlockTarget::Action(action) => {
            button.connect_clicked(move |button| {
                let _ = button.activate_action(action, None);
                sender.input(VideoShellInput::ClosePanel);
            });
        }
    }
    button
}

fn chapters_page(mpv: &MpvBackend, sender: &ComponentSender<VideoShell>) -> gtk::Widget {
    let chapters = group(&i18n("Jump to a chapter"), None);
    let current = mpv.get_current_chapter();
    for index in 0..mpv.get_chapter_count() {
        let time = mpv.get_property_f64(&format!("chapter-list/{index}/time"));
        let title = chapter_title(
            mpv.get_property_string(&format!("chapter-list/{index}/title")),
            index,
        );
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(big_media_components::time::playback_time(time))
            .activatable(true)
            .build();
        if index == current {
            row.add_prefix(&gtk::Image::from_icon_name("media-playback-start-symbolic"));
        }
        let sender = sender.clone();
        row.connect_activated(move |_| {
            sender.input(VideoShellInput::SetChapter(index));
            sender.input(VideoShellInput::PanelNavigate(PanelPage::Chapters));
        });
        chapters.add(&row);
    }
    chapters.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_window_gets_three_columns_right_under_the_header() {
        let layout = panel_layout(1920, 1080, 46, 110);
        assert_eq!((layout.columns, layout.width, layout.top), (3, 620, 50));
        assert_eq!(layout.bottom, 116);
        assert!(!layout.compact);
    }

    #[test]
    fn short_window_covers_the_controls_instead_of_shrinking() {
        let layout = panel_layout(1174, 420, 46, 110);
        assert_eq!(layout.bottom, 420 - 50 - PANEL_MIN_HEIGHT);
        assert!(layout.compact);
    }

    #[test]
    fn narrow_windows_fit_the_card_and_drop_columns() {
        assert_eq!(panel_layout(800, 600, 46, 110).columns, 2);
        assert_eq!(panel_layout(520, 600, 46, 110).width, 504);
        assert_eq!(panel_layout(360, 600, 46, 110).columns, 1);
    }
}
