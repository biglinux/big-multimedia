//! Canonical IPTV channel model.

/// Metadata for one IPTV channel parsed from an M3U `#EXTINF` line.
///
/// All fields default to empty strings when the corresponding EXTINF
/// attribute is missing — IPTV playlists in the wild routinely omit
/// attributes, so callers should tolerate empty values rather than
/// treat them as errors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IptvChannel {
    /// Display name (text after the last comma on the `#EXTINF` line).
    pub name: String,
    /// Stream URL (or local path for on-disk playlists).
    pub url: String,
    /// Group/category (`group-title="..."`) for sidebar grouping.
    pub group: String,
    /// Channel logo URL (`tvg-logo="..."`) for thumbnail fetching.
    pub logo: String,
    /// Channel ID (`tvg-id="..."`), used to correlate with XMLTV EPG.
    pub tvg_id: String,
}
