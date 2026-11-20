# Native library limits

The players inherit these defects from GTK, libadwaita and their Rust bindings;
each row names the version it was seen on. Project code avoids them where it
can, and `clippy.toml` bans the calls that trigger them, each ban naming its
replacement. A ban keeps a defect from spreading; it
does not remove what the libraries themselves still construct.

| Native defect | Effect | Project handling |
| --- | --- | --- |
| GTK 4.22 menu tracker does not release unmerged section links | every section a menu shows stays alive | `big_gtk_kit::menu::append_shared_section` builds each section once per process; `gio::Menu::append_section` is banned |
| libadwaita 1.9 `AdwBreakpointBin` frees its breakpoint array without an element destructor | one breakpoint per video window, three per adaptive dialog (about 1.8 KB an opening), four per preferences dialog | project breakpoints go through `big_gtk_kit::breakpoint::add`; dialogs come from `big_gtk_kit::alert::Alert` and `big_gtk_kit::dialog::Dialog`, which reuse one made already; the video sweep allows three native breakpoints per cycle |
| libadwaita 1.9 `ViewStack` keeps its transition animation after teardown, and `InlineViewSwitcher` a widget tree per page | one animation and its target per stack; one tree per switcher page | project pages use `gtk::Stack` with `big_gtk_kit::view_switcher::toggles_for`; libadwaita's own preferences windows and dialogs still build a `ViewStack` |
| libadwaita 1.9 dialog focus restoration keeps a closed sheet's subtree | closed Open URL and Go to Time dialogs retain their entry controls | none possible in project code; the video sweep reports it on stock libraries |
| gtk-rs 0.11 `update_relation` leaks its target list | one list per call | `big_gtk_kit::accessible::relate` |
| libadwaita 1.9 `PasswordEntryRow` keeps its buffer | the buffer and the password in it outlive the row | `big_gtk_kit::password::password_row` |
| libadwaita-rs 0.9 `AlertDialog::choose` and `choose_future` keep the dialog | one dialog per question | `big_gtk_kit::alert::choose` |
| GTK 4.22 `Switch::set_active` cancels a click's slide even when the value does not change | a refresh that sets the same value swallows the person's click | `big_gtk_kit::switch::set_active` |
| libadwaita 1.9 `EntryRow` exposes its title twice | a screen reader reads the field's name twice | `big_relm4_components::input::entry_row::build` |
| GTK 4.24 `PopoverMenu` built from a menu model leaves its items unnamed | a screen reader announces menu items without their labels | `popover_menu::from_model` and `popover_menu::set_menu_model` in `big-relm4-components` |
| libadwaita 1.10 `SpinRow` under GTK 4.24 hides the row and its spin button from the accessibility tree | a screen reader cannot reach the value | `spin_row::accessible_spin_row` in `big-relm4-components` |

A plain GTK window with no player also retains a small libwayland-client
allocation (`wl_display_read_events`) per open/close cycle. Compare a growth
measurement against that plain-window control before attributing it to the
players.

Closing a hosted video player logs GTK's `Invalid inhibitor cookie` warning:
the Wayland backend removes a window's idle inhibitor in `window_removed`,
before the component's `Drop` calls `uninhibit`. The inhibitor is already gone;
the warning is harmless.

## Leak sweeps against these limits

`scripts/leak-surfaces/video.toml` allows only the three native
`AdwBreakpoint` objects per cycle. Do not widen the allowance to cover retained
dialog controls: on stock libraries the Open URL and Go to Time surfaces fail
because of the focus defect above, and that failure is the expected report.
A clean run against patched libraries does not mean the installed libraries
are fixed. How to run the sweep is in `CONTRIBUTING.md`.
