use big_relm4_components::list::action_nav_row::BigActionNavRowSpec;

#[test]
fn action_nav_row_escapes_text() {
    let resolved = BigActionNavRowSpec::new("<b>Audio</b>")
        .subtitle("A&B")
        .prefix_icon_name("audio-volume-high-symbolic")
        .resolved();

    assert_eq!(resolved.title, "&lt;b&gt;Audio&lt;/b&gt;");
    assert_eq!(resolved.subtitle.as_deref(), Some("A&amp;B"));
    assert_eq!(
        resolved.prefix_icon_name.as_deref(),
        Some("audio-volume-high-symbolic")
    );
    assert_eq!(resolved.action_icon_name, "go-next-symbolic");
    assert_eq!(resolved.action_label, "<b>Audio</b>");
}
