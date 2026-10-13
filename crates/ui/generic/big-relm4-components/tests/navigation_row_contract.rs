use big_relm4_components::list::navigation_row::BigNavigationRowSpec;

#[test]
fn navigation_row_escapes_text() {
    let resolved = BigNavigationRowSpec::new("<b>Audio</b>")
        .subtitle("A&B")
        .prefix_icon_name("audio-volume-high-symbolic")
        .resolved();

    assert_eq!(resolved.title, "&lt;b&gt;Audio&lt;/b&gt;");
    assert_eq!(resolved.subtitle.as_deref(), Some("A&amp;B"));
    assert_eq!(
        resolved.prefix_icon_name.as_deref(),
        Some("audio-volume-high-symbolic")
    );
    assert_eq!(resolved.trailing_icon_name, "go-next-symbolic");
    assert_eq!(resolved.action_label, "<b>Audio</b>");
}

#[test]
fn navigation_row_exposes_action() {
    let resolved = BigNavigationRowSpec::new("Audio Settings")
        .trailing_icon_name("document-edit-symbolic")
        .action_label("Open audio settings")
        .resolved();

    assert_eq!(resolved.trailing_icon_name, "document-edit-symbolic");
    assert_eq!(resolved.action_label, "Open audio settings");
}
