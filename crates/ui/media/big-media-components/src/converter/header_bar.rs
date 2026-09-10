//! Shared converter header bar.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::hamburger_menu::{
    BigHamburgerMenuSpec, BigMenuActionItem, build_hamburger_menu_button, set_flat_action_popover,
};
use relm4::{Component, ComponentParts, ComponentSender};

type HeaderCallback = Rc<RefCell<Option<Box<dyn Fn()>>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeaderPresentationState {
    add_sensitive: bool,
    clear_sensitive: bool,
    convert_visible: bool,
    cancel_visible: bool,
    clear_visible: bool,
    queue_label_visible: bool,
    queue_label_text: Option<String>,
}

/// Translatable labels used by [`BigConverterHeaderBar`].
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigConverterHeaderText {
    pub add: String,
    pub add_files: String,
    pub add_folder: String,
    pub add_network: String,
    pub convert: String,
    pub cancel: String,
    pub clear_queue: String,
    pub files: String,
    pub restore_defaults: String,
    pub keyboard_shortcuts: String,
    pub about: String,
    pub quit: String,
    pub main_menu: String,
}

impl BigConverterHeaderText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(text: impl Fn(&str) -> String) -> Self {
        Self {
            add: text("Add"),
            add_files: text("Add Files"),
            add_folder: text("Add Folder"),
            add_network: text("Add Network File"),
            convert: text("Convert"),
            cancel: text("Cancel"),
            clear_queue: text("Clear queue"),
            files: text("files"),
            restore_defaults: text("Restore Defaults"),
            keyboard_shortcuts: text("Keyboard Shortcuts"),
            about: text("About"),
            quit: text("Quit"),
            main_menu: text("Main menu"),
        }
    }
}

/// Header bar with queue state and converter actions.
pub struct BigConverterHeaderBar {
    container: adw::HeaderBar,
    convert_button: gtk::Button,
    add_button: gtk::MenuButton,
    cancel_button: gtk::Button,
    clear_button: gtk::Button,
    queue_label: gtk::Label,
    files_label: String,
    queue_count: Cell<usize>,
    converting: Cell<bool>,
    on_convert: HeaderCallback,
    on_add: HeaderCallback,
    on_add_folder: HeaderCallback,
    on_add_network: HeaderCallback,
    on_clear_queue: HeaderCallback,
    on_cancel: HeaderCallback,
    on_restore: HeaderCallback,
}

impl BigConverterHeaderBar {
    /// Build the header bar widget tree using the translated `text`.
    #[must_use]
    pub fn new(text: BigConverterHeaderText) -> Self {
        Self::build_into(adw::HeaderBar::new(), text)
    }

    /// Build the header into an existing [`adw::HeaderBar`] root.
    ///
    /// Lets the Relm4 wrapper [`BigConverterHeader`] reuse the component's own
    /// `Root` widget as the container; [`new`](Self::new) delegates here with a
    /// fresh header bar.
    #[must_use]
    pub fn build_into(container: adw::HeaderBar, text: BigConverterHeaderText) -> Self {
        container.add_css_class("flat");

        let left_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        left_box.set_valign(gtk::Align::Center);

        let clear_button = tooltip::icon_button(
            "user-trash-symbolic",
            &text.clear_queue,
            &["flat", "circular", "error"],
        );
        clear_button.set_visible(false);
        left_box.append(&clear_button);

        let queue_label = gtk::Label::builder()
            .label(format!("0 {}", text.files))
            .css_classes(["dim-label"])
            .visible(false)
            .build();
        left_box.append(&queue_label);

        container.pack_start(&left_box);

        let add_button = tooltip::labeled_menu_button(&text.add, &text.add, &["suggested-action"]);
        set_flat_action_popover(&add_button, &add_menu_items(&text));

        let convert_button = gtk::Button::builder()
            .label(&text.convert)
            .css_classes(["suggested-action"])
            .visible(false)
            .build();
        convert_button.update_property(&[gtk::accessible::Property::Label(&text.convert)]);

        let cancel_button = gtk::Button::builder()
            .label(&text.cancel)
            .css_classes(["destructive-action"])
            .visible(false)
            .build();
        cancel_button.update_property(&[gtk::accessible::Property::Label(&text.cancel)]);

        let center = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        center.set_halign(gtk::Align::Center);
        center.append(&add_button);
        center.append(&convert_button);
        center.append(&cancel_button);
        container.set_title_widget(Some(&center));

        let menu_button = build_hamburger_menu_button(
            &BigHamburgerMenuSpec::new(text.main_menu.clone())
                .css_classes(["flat"])
                .items(main_menu_items(&text)),
        );
        container.pack_end(&menu_button);

        let header = Self {
            container,
            convert_button,
            add_button,
            cancel_button,
            clear_button,
            queue_label,
            files_label: text.files,
            queue_count: Cell::new(0),
            converting: Cell::new(false),
            on_convert: Rc::new(RefCell::new(None)),
            on_add: Rc::new(RefCell::new(None)),
            on_add_folder: Rc::new(RefCell::new(None)),
            on_add_network: Rc::new(RefCell::new(None)),
            on_clear_queue: Rc::new(RefCell::new(None)),
            on_cancel: Rc::new(RefCell::new(None)),
            on_restore: Rc::new(RefCell::new(None)),
        };

        header.install_callbacks();
        header
    }

    fn install_callbacks(&self) {
        let cb = self.on_convert.clone();
        self.convert_button
            .connect_clicked(move |_| run_callback(&cb));

        let cb = self.on_clear_queue.clone();
        self.clear_button
            .connect_clicked(move |_| run_callback(&cb));

        let cb = self.on_cancel.clone();
        self.cancel_button
            .connect_clicked(move |_| run_callback(&cb));

        let action_group = gio::SimpleActionGroup::new();
        install_action(&action_group, "add-files", self.on_add.clone());
        install_action(&action_group, "add-folder", self.on_add_folder.clone());
        install_action(&action_group, "add-network", self.on_add_network.clone());
        install_action(&action_group, "restore-defaults", self.on_restore.clone());
        self.container
            .insert_action_group("header", Some(&action_group));
    }

    /// Borrow the root [`adw::HeaderBar`] widget.
    #[must_use]
    pub fn widget(&self) -> &adw::HeaderBar {
        &self.container
    }

    /// Install a callback fired when the user clicks "Convert".
    pub fn connect_convert_clicked<F: Fn() + 'static>(&self, f: F) {
        *self.on_convert.borrow_mut() = Some(Box::new(f));
    }

    /// Install a callback fired when the user clicks "Add files".
    pub fn connect_add_clicked<F: Fn() + 'static>(&self, f: F) {
        *self.on_add.borrow_mut() = Some(Box::new(f));
    }

    /// Install a callback fired when the user picks "Add folder" from the menu.
    pub fn connect_add_folder_clicked<F: Fn() + 'static>(&self, f: F) {
        *self.on_add_folder.borrow_mut() = Some(Box::new(f));
    }

    /// Install a callback fired when the user picks "Add network" from the menu.
    pub fn connect_add_network_clicked<F: Fn() + 'static>(&self, f: F) {
        *self.on_add_network.borrow_mut() = Some(Box::new(f));
    }

    /// Install a callback fired when the user clicks "Clear queue".
    pub fn connect_clear_queue_clicked<F: Fn() + 'static>(&self, f: F) {
        *self.on_clear_queue.borrow_mut() = Some(Box::new(f));
    }

    /// Install a callback fired when the user clicks "Cancel" during conversion.
    pub fn connect_cancel_clicked<F: Fn() + 'static>(&self, f: F) {
        *self.on_cancel.borrow_mut() = Some(Box::new(f));
    }

    /// Install a callback fired when the user activates "Restore defaults".
    pub fn connect_restore_defaults<F: Fn() + 'static>(&self, f: F) {
        *self.on_restore.borrow_mut() = Some(Box::new(f));
    }

    /// Update the displayed queue count and toggle the matching widgets.
    pub fn set_queue_count(&self, count: usize) {
        self.queue_count.set(count);
        self.apply_header_presentation_state();
    }

    /// Switch the header bar to the running-conversion layout.
    pub fn set_converting(&self, converting: bool) {
        self.converting.set(converting);
        self.apply_header_presentation_state();
    }

    fn apply_header_presentation_state(&self) {
        let state = resolve_header_presentation_state(
            self.queue_count.get(),
            self.converting.get(),
            &self.files_label,
        );

        self.add_button.set_sensitive(state.add_sensitive);
        self.clear_button.set_sensitive(state.clear_sensitive);
        self.convert_button.set_visible(state.convert_visible);
        self.cancel_button.set_visible(state.cancel_visible);
        self.clear_button.set_visible(state.clear_visible);
        self.queue_label.set_visible(state.queue_label_visible);
        if let Some(queue_label_text) = state.queue_label_text {
            self.queue_label.set_label(&queue_label_text);
        }
    }
}

fn install_action(group: &gio::SimpleActionGroup, name: &str, callback: HeaderCallback) {
    let action = gio::SimpleAction::new(name, None);
    action.connect_activate(move |_, _| run_callback(&callback));
    group.add_action(&action);
}

fn add_menu_items(text: &BigConverterHeaderText) -> [BigMenuActionItem; 3] {
    [
        BigMenuActionItem::new(text.add_files.clone(), "header.add-files"),
        BigMenuActionItem::new(text.add_folder.clone(), "header.add-folder"),
        BigMenuActionItem::new(text.add_network.clone(), "header.add-network"),
    ]
}

fn main_menu_items(text: &BigConverterHeaderText) -> [BigMenuActionItem; 4] {
    [
        BigMenuActionItem::new(text.restore_defaults.clone(), "header.restore-defaults"),
        BigMenuActionItem::new(text.keyboard_shortcuts.clone(), "win.show-help-overlay"),
        BigMenuActionItem::new(text.about.clone(), "app.about"),
        BigMenuActionItem::new(text.quit.clone(), "app.quit"),
    ]
}

fn resolve_header_presentation_state(
    queue_count: usize,
    is_converting: bool,
    files_label: &str,
) -> HeaderPresentationState {
    let has_multiple_files = queue_count >= 2;

    HeaderPresentationState {
        add_sensitive: !is_converting,
        clear_sensitive: !is_converting,
        convert_visible: !is_converting && queue_count > 0,
        cancel_visible: is_converting,
        clear_visible: has_multiple_files,
        queue_label_visible: has_multiple_files,
        queue_label_text: has_multiple_files.then(|| format!("{queue_count} {files_label}")),
    }
}

fn run_callback(callback: &HeaderCallback) {
    if let Some(f) = callback.borrow().as_ref() {
        f();
    }
}

// ---- Relm4 wrapper -------------------------------------------------------

/// Messages accepted by [`BigConverterHeader`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigConverterHeaderInput {
    /// Update the queue-count badge and toggle the dependent widgets.
    SetQueueCount(usize),
    /// Switch between the idle and running-conversion layouts.
    SetConverting(bool),
}

/// User actions emitted by [`BigConverterHeader`].
///
/// Destructive actions ([`ClearQueue`](Self::ClearQueue),
/// [`RestoreDefaults`](Self::RestoreDefaults)) carry no confirmation — the
/// receiver is responsible for confirming before acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigConverterHeaderOutput {
    /// "Convert" clicked.
    Convert,
    /// "Clear queue" clicked.
    ClearQueue,
    /// "Cancel" clicked during a conversion.
    Cancel,
    /// "Restore defaults" activated from the menu.
    RestoreDefaults,
    /// "Add files" clicked.
    AddFiles,
    /// "Add folder" activated from the menu.
    AddFolder,
    /// "Add network" activated from the menu.
    AddNetwork,
}

/// Relm4 wrapper around [`BigConverterHeaderBar`].
///
/// Translates the header's widget callbacks into typed
/// [`BigConverterHeaderOutput`] messages and applies
/// [`BigConverterHeaderInput`] to the wrapped bar. Follows the manual
/// `impl Component` idiom (no `view!` macro) shared across this crate.
pub struct BigConverterHeader {
    bar: BigConverterHeaderBar,
}

impl Component for BigConverterHeader {
    type Init = BigConverterHeaderText;
    type Input = BigConverterHeaderInput;
    type Output = BigConverterHeaderOutput;
    type CommandOutput = ();
    type Root = adw::HeaderBar;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::HeaderBar::new()
    }

    fn init(
        text: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let bar = BigConverterHeaderBar::build_into(root.clone(), text);

        let s = sender.clone();
        bar.connect_convert_clicked(move || {
            let _ = s.output(BigConverterHeaderOutput::Convert);
        });
        let s = sender.clone();
        bar.connect_clear_queue_clicked(move || {
            let _ = s.output(BigConverterHeaderOutput::ClearQueue);
        });
        let s = sender.clone();
        bar.connect_cancel_clicked(move || {
            let _ = s.output(BigConverterHeaderOutput::Cancel);
        });
        let s = sender.clone();
        bar.connect_restore_defaults(move || {
            let _ = s.output(BigConverterHeaderOutput::RestoreDefaults);
        });
        let s = sender.clone();
        bar.connect_add_clicked(move || {
            let _ = s.output(BigConverterHeaderOutput::AddFiles);
        });
        let s = sender.clone();
        bar.connect_add_folder_clicked(move || {
            let _ = s.output(BigConverterHeaderOutput::AddFolder);
        });
        let s = sender;
        bar.connect_add_network_clicked(move || {
            let _ = s.output(BigConverterHeaderOutput::AddNetwork);
        });

        ComponentParts {
            model: Self { bar },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            BigConverterHeaderInput::SetQueueCount(count) => self.bar.set_queue_count(count),
            BigConverterHeaderInput::SetConverting(converting) => {
                self.bar.set_converting(converting);
            }
        }
    }
}

#[cfg(test)]
mod relm4_tests {
    use super::*;
    use big_relm4_components::layout::hamburger_menu::{build_flat_menu_model, menu_action_names};
    use std::cell::RefCell;

    #[test]
    fn translated_text_routes_every_visible_message_through_callback() {
        let translated_messages = RefCell::new(Vec::new());
        let text = BigConverterHeaderText::translated(|message| {
            translated_messages.borrow_mut().push(message.to_string());
            format!("translated:{message}")
        });
        let translated_messages = translated_messages.into_inner();

        assert_eq!(text.add, "translated:Add");
        assert_eq!(text.add_files, "translated:Add Files");
        assert_eq!(text.add_folder, "translated:Add Folder");
        assert_eq!(text.add_network, "translated:Add Network File");
        assert_eq!(text.convert, "translated:Convert");
        assert_eq!(text.cancel, "translated:Cancel");
        assert_eq!(text.clear_queue, "translated:Clear queue");
        assert_eq!(text.files, "translated:files");
        assert_eq!(text.restore_defaults, "translated:Restore Defaults");
        assert_eq!(text.keyboard_shortcuts, "translated:Keyboard Shortcuts");
        assert_eq!(text.about, "translated:About");
        assert_eq!(text.quit, "translated:Quit");
        assert_eq!(text.main_menu, "translated:Main menu");
        assert_eq!(translated_messages.len(), 13);
        assert!(translated_messages.contains(&"Add Network File".to_string()));
        assert!(translated_messages.contains(&"Keyboard Shortcuts".to_string()));
    }

    #[test]
    fn header_messages_distinct() {
        assert_ne!(
            BigConverterHeaderOutput::Convert,
            BigConverterHeaderOutput::Cancel
        );
        assert_eq!(
            BigConverterHeaderInput::SetQueueCount(3),
            BigConverterHeaderInput::SetQueueCount(3)
        );
    }

    // GMenu is a GLib object model; ordinary tests cover the contract while
    // Miri stays focused on Rust-owned invariants.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn add_menu_is_flat_and_exposes_all_sources() {
        let text = BigConverterHeaderText::translated(str::to_owned);
        let menu = build_flat_menu_model(&add_menu_items(&text));

        assert_eq!(
            menu_action_names(&menu),
            [
                "header.add-files",
                "header.add-folder",
                "header.add-network"
            ]
        );
        for index in 0..menu.n_items() {
            assert!(menu.item_link(index, "section").is_none());
            assert!(menu.item_link(index, "submenu").is_none());
        }
    }

    // GMenu is a GLib object model; ordinary tests cover the contract while
    // Miri stays focused on Rust-owned invariants.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn main_menu_is_flat_and_exposes_window_and_app_actions() {
        let text = BigConverterHeaderText::translated(str::to_owned);
        let menu = build_flat_menu_model(&main_menu_items(&text));

        assert_eq!(
            menu_action_names(&menu),
            [
                "header.restore-defaults",
                "win.show-help-overlay",
                "app.about",
                "app.quit"
            ]
        );
        for index in 0..menu.n_items() {
            assert!(menu.item_link(index, "section").is_none());
            assert!(menu.item_link(index, "submenu").is_none());
        }
    }

    #[test]
    fn presentation_state_keeps_idle_empty_queue_minimal() {
        let state = resolve_header_presentation_state(0, false, "files");

        assert!(state.add_sensitive);
        assert!(state.clear_sensitive);
        assert!(!state.convert_visible);
        assert!(!state.cancel_visible);
        assert!(!state.clear_visible);
        assert!(!state.queue_label_visible);
        assert_eq!(state.queue_label_text, None);
    }

    #[test]
    fn presentation_state_shows_convert_for_single_idle_file() {
        let state = resolve_header_presentation_state(1, false, "files");

        assert!(state.add_sensitive);
        assert!(state.clear_sensitive);
        assert!(state.convert_visible);
        assert!(!state.cancel_visible);
        assert!(!state.clear_visible);
        assert!(!state.queue_label_visible);
        assert_eq!(state.queue_label_text, None);
    }

    #[test]
    fn presentation_state_shows_queue_controls_for_multiple_idle_files() {
        let state = resolve_header_presentation_state(3, false, "files");

        assert!(state.add_sensitive);
        assert!(state.clear_sensitive);
        assert!(state.convert_visible);
        assert!(!state.cancel_visible);
        assert!(state.clear_visible);
        assert!(state.queue_label_visible);
        assert_eq!(state.queue_label_text.as_deref(), Some("3 files"));
    }

    #[test]
    fn presentation_state_disables_add_and_clear_while_converting() {
        let state = resolve_header_presentation_state(3, true, "files");

        assert!(!state.add_sensitive);
        assert!(!state.clear_sensitive);
        assert!(!state.convert_visible);
        assert!(state.cancel_visible);
        assert!(state.clear_visible);
        assert!(state.queue_label_visible);
        assert_eq!(state.queue_label_text.as_deref(), Some("3 files"));
    }

    #[test]
    fn run_callback_invokes_present_callback_and_ignores_empty_slot() {
        let call_count = Rc::new(Cell::new(0));
        let callback: HeaderCallback = Rc::new(RefCell::new(Some(Box::new({
            let call_count = call_count.clone();
            move || call_count.set(call_count.get() + 1)
        }))));

        run_callback(&callback);
        assert_eq!(call_count.get(), 1);

        let empty_callback: HeaderCallback = Rc::new(RefCell::new(None));
        run_callback(&empty_callback);
        assert_eq!(call_count.get(), 1);
    }

    // SimpleActionGroup construction crosses GLib FFI; ordinary tests cover
    // action installation while Miri stays on the Rust-owned callback reducer.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn install_action_routes_activation_to_callback_slot() {
        let call_count = Rc::new(Cell::new(0));
        let callback: HeaderCallback = Rc::new(RefCell::new(Some(Box::new({
            let call_count = call_count.clone();
            move || call_count.set(call_count.get() + 1)
        }))));
        let action_group = gio::SimpleActionGroup::new();

        install_action(&action_group, "probe", callback);
        let action = action_group
            .lookup_action("probe")
            .expect("probe action installed");
        action.activate(None);

        assert_eq!(call_count.get(), 1);
    }
}
