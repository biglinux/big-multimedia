# Native library limits

The players inherit these defects from GTK 4.22.4 and libadwaita 1.9.3. Project
code avoids them where it can, and `clippy.toml` bans the calls that trigger
them, each ban naming its replacement. A ban keeps a defect from spreading; it
does not remove what the libraries themselves still construct.

| Native defect | Effect | Project handling |
| --- | --- | --- |
| GTK menu tracker does not release unmerged section links | every section a menu shows stays alive | `big_gtk_kit::menu::append_shared_section` builds each section once per process; `gio::Menu::append_section` is banned |
| libadwaita `AdwBreakpointBin` frees its breakpoint array without an element destructor | one breakpoint per video window, three per adaptive dialog, four per preferences dialog | project breakpoints go through `big_gtk_kit::breakpoint::add`; the video sweep allows three native breakpoints per cycle |
| libadwaita `ViewStack` keeps its transition animation after teardown | one animation and its target per stack | project pages use `gtk::Stack`; libadwaita's own preferences windows and dialogs still build a `ViewStack` |
| libadwaita dialog focus restoration keeps a closed sheet's subtree | closed Open URL and Go to Time dialogs retain their entry controls | none possible in project code; the video sweep reports it on stock libraries |
| gtk-rs 0.11 `update_relation` leaks its target list | one list per call | `big_gtk_kit::accessible::relate` |
| libadwaita `PasswordEntryRow` keeps its buffer | the buffer and the password in it outlive the row | `big_gtk_kit::password::password_row` |

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
