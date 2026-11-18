// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shortcut specs, override parsing, and accelerator resolution (display-free).
//!
//! The gdk keyval helpers and the GTK-backed `display_accel` label formatter
//! live in `big_app_kit::keybindings`.

use std::collections::HashMap;

/// Compile-time description of one keyboard shortcut the app declares.
///
/// Apps assemble a `&'static [ShortcutDef]` and feed it through
/// [`resolve_accels`] to combine defaults with any user overrides.
pub struct ShortcutDef {
    /// Scoped action name (`"win.play-pause"`).
    pub action: &'static str,
    /// Localized title for the shortcut dialog row.
    pub display_name: &'static str,
    /// Grouping label shown above the row in the shortcut dialog.
    pub category: &'static str,
    /// Canonical GTK accelerator (`"<Ctrl>m"`, `"space"`) used when
    /// the user has not customised it.
    pub default_accel: &'static str,
    /// `true` when the binding is a bare key (no modifiers) and must
    /// be suppressed while a text entry has focus.
    pub single_key: bool,
}

/// Pair each [`ShortcutDef`] with its effective accelerator string.
///
/// Parses `custom` via [`parse_custom`] and overlays the overrides on top
/// of each shortcut's `default_accel`. Returned vector preserves the input
/// order so it can drive a settings UI directly.
#[must_use]
pub fn resolve_accels<'a>(
    shortcuts: &'a [ShortcutDef],
    custom: &str,
) -> Vec<(&'a ShortcutDef, String)> {
    let overrides: HashMap<String, String> = parse_custom(custom).into_iter().collect();
    shortcuts
        .iter()
        .map(|shortcut| {
            let accel = overrides
                .get(shortcut.action)
                .cloned()
                .unwrap_or_else(|| shortcut.default_accel.to_string());
            (shortcut, accel)
        })
        .collect()
}

/// Build an accel-string -> action-id map for the active shortcut set.
///
/// When `only_single_key` is true, only shortcuts marked `single_key` are
/// included (used for typing-mode bindings that should not fire while a
/// text entry has focus).
#[must_use]
pub fn build_accel_map(
    shortcuts: &[ShortcutDef],
    custom: &str,
    only_single_key: bool,
) -> HashMap<String, String> {
    resolve_accels(shortcuts, custom)
        .into_iter()
        .filter(|(shortcut, _)| !only_single_key || shortcut.single_key)
        .map(|(shortcut, accel)| (canonical_accel(&accel), shortcut.action.to_string()))
        .collect()
}

/// The modifiers a shortcut lookup tells apart.
#[derive(Clone, Copy, Debug, Default)]
pub struct AccelModifiers {
    /// Control, written `<Ctrl>`.
    pub ctrl: bool,
    /// Shift, written `<Shift>`.
    pub shift: bool,
    /// Alt, written `<Alt>`.
    pub alt: bool,
}

/// The accelerator text for `key` held with `modifiers`, in the one order every
/// lookup uses: `<Ctrl>`, `<Shift>`, `<Alt>`, then the key name.
#[must_use]
pub fn accel_text(modifiers: AccelModifiers, key: &str) -> String {
    let mut text = String::new();
    if modifiers.ctrl {
        text.push_str("<Ctrl>");
    }
    if modifiers.shift {
        text.push_str("<Shift>");
    }
    if modifiers.alt {
        text.push_str("<Alt>");
    }
    text.push_str(key);
    text
}

/// `accel` in [`accel_text`]'s order. `<Control>` and `<Primary>` read as
/// `<Ctrl>` and `<Mod1>` as `<Alt>`, in any case; other modifiers follow these.
#[must_use]
pub fn canonical_accel(accel: &str) -> String {
    let mut modifiers = AccelModifiers::default();
    let mut key = String::new();
    let mut rest = accel;
    while let Some(after) = rest.strip_prefix('<')
        && let Some((name, tail)) = after.split_once('>')
    {
        match name.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "primary" => modifiers.ctrl = true,
            "shift" => modifiers.shift = true,
            "alt" | "mod1" => modifiers.alt = true,
            _ => key.push_str(&rest[..name.len() + 2]),
        }
        rest = tail;
    }
    key.push_str(rest);
    accel_text(modifiers, &key)
}

/// Decode the serialized custom-overrides string into `(action, accel)` pairs.
///
/// Expected format is `action=accel;action=accel`, each accel a GTK
/// accelerator such as `<Ctrl>m`.
#[must_use]
pub fn parse_custom(value: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for entry in value.split(';') {
        let entry = entry.trim();
        if let Some((action, accel)) = entry.split_once('=') {
            let action = action.trim().to_string();
            let accel = accel.trim().to_string();
            if !action.is_empty() && !accel.is_empty() {
                result.push((action, accel));
            }
        }
    }
    result
}

/// Encode `(action, accel)` overrides as the on-disk `action=accel;...` form.
///
/// Inverse of [`parse_custom`]; safe to feed back through it.
#[must_use]
pub fn serialize_custom(overrides: &[(String, String)]) -> String {
    overrides
        .iter()
        .map(|(action, key)| format!("{action}={key}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// Drop any override for `action_id` from the in-memory list.
///
/// No-op when the action is not currently customised. Paired with
/// [`set_custom_override`] to implement the "reset to default" path.
pub fn remove_custom_override(overrides: &mut Vec<(String, String)>, action_id: &str) {
    overrides.retain(|(action, _)| action != action_id);
}

/// Update the custom override field in place.
pub fn set_custom_override(
    overrides: &mut Vec<(String, String)>,
    action_id: impl Into<String>,
    accel: impl Into<String>,
    default_accel: &str,
) {
    let action_id = action_id.into();
    let accel = accel.into();
    remove_custom_override(overrides, &action_id);
    if accel != default_accel {
        overrides.push((action_id, accel));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHORTCUTS: &[ShortcutDef] = &[
        ShortcutDef {
            action: "play-pause",
            display_name: "Play / Pause",
            category: "Playback",
            default_accel: "space",
            single_key: true,
        },
        ShortcutDef {
            action: "mute",
            display_name: "Mute",
            category: "Playback",
            default_accel: "<Ctrl>m",
            single_key: false,
        },
    ];

    #[test]
    fn parse_custom_rejects_empty_action_or_accel() {
        let result = parse_custom("=p;mute=;play-pause=space");

        assert_eq!(result, vec![("play-pause".into(), "space".into())]);
    }

    #[test]
    fn serialize_custom_roundtrips() {
        let original = vec![
            ("play-pause".into(), "p".into()),
            ("volume-up".into(), "Up".into()),
        ];
        assert_eq!(parse_custom(&serialize_custom(&original)), original);
    }

    #[test]
    fn build_accel_map_only_single_key_filters() {
        let map = build_accel_map(SHORTCUTS, "", true);
        assert_eq!(map.get("space"), Some(&"play-pause".to_string()));
        assert_eq!(map.get("<Ctrl>m"), None);
    }

    /// GTK accepts modifiers in any order and under several names; a press is
    /// looked up as `<Ctrl><Shift><Alt>key`.
    #[test]
    fn build_accel_map_orders_modifiers() {
        let map = build_accel_map(
            SHORTCUTS,
            "mute=<Shift><Control>m;play-pause=<alt><Primary>p",
            false,
        );
        assert_eq!(map.get("<Ctrl><Shift>m"), Some(&"mute".to_string()));
        assert_eq!(map.get("<Ctrl><Alt>p"), Some(&"play-pause".to_string()));
    }

    #[test]
    fn canonical_accel_keeps_unknown_text() {
        for (accel, canonical) in [
            ("", ""),
            ("space", "space"),
            ("<Ctrl><Shift>z", "<Ctrl><Shift>z"),
            ("<Mod1><SHIFT>F9", "<Shift><Alt>F9"),
            ("<Super><Shift>k", "<Shift><Super>k"),
            ("<Ctrl", "<Ctrl"),
            ("<>x", "<>x"),
        ] {
            assert_eq!(canonical_accel(accel), canonical, "{accel}");
        }
    }

    #[test]
    fn custom_override_applies_to_matching_action() {
        let resolved = resolve_accels(SHORTCUTS, "play-pause=p");
        assert_eq!(resolved[0].1, "p");
        assert_eq!(resolved[1].1, "<Ctrl>m");
    }

    #[test]
    fn set_custom_override_replaces_and_removes_default() {
        let mut entries = vec![("play-pause".to_string(), "space".to_string())];

        set_custom_override(&mut entries, "play-pause", "p", "space");
        assert_eq!(entries, vec![("play-pause".to_string(), "p".to_string())]);

        set_custom_override(&mut entries, "play-pause", "space", "space");
        assert!(entries.is_empty());
    }

    #[test]
    fn remove_custom_override_keeps_other_actions() {
        let mut entries = vec![
            ("play-pause".to_string(), "p".to_string()),
            ("mute".to_string(), "m".to_string()),
        ];

        remove_custom_override(&mut entries, "play-pause");
        assert_eq!(entries, vec![("mute".to_string(), "m".to_string())]);
    }
}
