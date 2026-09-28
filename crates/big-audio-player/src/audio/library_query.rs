//! Pure (GTK-free) library node → track resolution for the Relm4 `AudioShell`.
//!
//! The shell runs these off the read-only DB connection and pushes the result
//! into the library panel as `PopulateDetail`.

use crate::audio::browser_node::BrowserNodeKind;
use crate::audio::library_grouping::{GroupBy, LibraryGrouping};
use crate::audio::ui::library_tree::{LibraryTreeNode, LibraryTreeSnapshot};
use crate::db::models::Track;
use crate::db::queries;
use crate::i18n::i18n;

/// Map a `BrowserNode::kind() as u32` back to the enum (the `NodeSelected`
/// intent carries the discriminant so the panel needn't expose the GObject).
#[must_use]
pub(crate) fn kind_from_u32(value: u32) -> BrowserNodeKind {
    match value {
        1 => BrowserNodeKind::Artist,
        2 => BrowserNodeKind::Album,
        3 => BrowserNodeKind::Genre,
        4 => BrowserNodeKind::Year,
        5 => BrowserNodeKind::Folder,
        6 => BrowserNodeKind::Station,
        7 => BrowserNodeKind::Playlist,
        8 => BrowserNodeKind::SmartPlaylist,
        9 => BrowserNodeKind::File,
        10 => BrowserNodeKind::Divider,
        11 => BrowserNodeKind::VariousArtists,
        _ => BrowserNodeKind::All,
    }
}

/// Resolve a selected library node into its tracks (in DB order).
pub(crate) fn query_tracks_for_node(
    conn: &rusqlite::Connection,
    node_kind: BrowserNodeKind,
    node_key: &str,
    node_secondary: &str,
    filter_text: &str,
    age_secs: Option<i64>,
    grouping: &LibraryGrouping,
) -> Vec<Track> {
    match node_kind {
        BrowserNodeKind::All => {
            queries::get_library_tracks(conn, &[], filter_text, age_secs).unwrap_or_default()
        }
        BrowserNodeKind::VariousArtists => {
            let compilations = queries::get_compilation_albums(conn, filter_text, &[], age_secs)
                .unwrap_or_default();
            let mut tracks = Vec::new();
            for (album, _) in &compilations {
                let filters = [("album", album.as_str())];
                if let Ok(mut resolved) =
                    queries::get_library_tracks(conn, &filters, filter_text, age_secs)
                {
                    tracks.append(&mut resolved);
                }
            }
            tracks
        }
        BrowserNodeKind::Divider | BrowserNodeKind::Station | BrowserNodeKind::Folder => Vec::new(),
        BrowserNodeKind::File => queries::get_track_by_path(conn, node_key)
            .ok()
            .flatten()
            .map_or_else(Vec::new, |track| vec![track]),
        BrowserNodeKind::Artist
        | BrowserNodeKind::Album
        | BrowserNodeKind::Genre
        | BrowserNodeKind::Year
        | BrowserNodeKind::Playlist
        | BrowserNodeKind::SmartPlaylist => {
            let mut filters: Vec<(String, String)> = Vec::new();
            let column = match node_kind {
                BrowserNodeKind::Artist
                    if grouping.level(0) == GroupBy::AlbumArtist
                        || grouping.level(1) == GroupBy::AlbumArtist =>
                {
                    "album_artist"
                }
                BrowserNodeKind::Album => "album",
                BrowserNodeKind::Genre => "genre",
                BrowserNodeKind::Year => "year",
                _ => "artist",
            };
            filters.push((column.to_string(), node_key.to_string()));

            if !node_secondary.is_empty()
                && let Some((parent_column, parent_value)) = node_secondary.split_once(':')
            {
                filters.push((parent_column.to_string(), parent_value.to_string()));
            }

            let filter_refs: Vec<(&str, &str)> = filters
                .iter()
                .map(|(column, value)| (column.as_str(), value.as_str()))
                .collect();
            queries::get_library_tracks(conn, &filter_refs, filter_text, age_secs)
                .unwrap_or_default()
        }
    }
}

/// Build the full nested library tree (the `SetLibraryTree` payload) from the
/// group + recursive child queries — the producer side of the atomic preload.
pub(crate) fn try_build_library_tree_snapshot(
    conn: &rusqlite::Connection,
    grouping: &LibraryGrouping,
    filter: &str,
    age_secs: Option<i64>,
) -> Result<LibraryTreeSnapshot, &'static str> {
    let total = queries::get_library_track_count(conn, filter, age_secs)
        .map_err(|_| "library count failed")?;
    if total == 0 {
        return Ok(LibraryTreeSnapshot::default());
    }

    let level0 = grouping.level(0);
    if level0 == GroupBy::None {
        // Songs view: flat file leaves, no grouping.
        let tracks = queries::get_library_tracks(conn, &[], filter, age_secs)
            .map_err(|_| "library query failed")?;
        return Ok(LibraryTreeSnapshot {
            level0: tracks.iter().map(file_node).collect(),
        });
    }

    let mut level0_nodes = vec![LibraryTreeNode {
        kind: BrowserNodeKind::All,
        label: i18n("All Tracks"),
        key: String::new(),
        count: total as i32,
        secondary: String::new(),
        children: Vec::new(),
        track: None,
    }];

    // One query for every matching track, grouped here. Asking SQLite once per
    // group scanned the whole table per group — COALESCE keeps the column
    // indexes out — which took a core for 15 s on a 30 000-track library.
    // `get_library_tracks` returns the leaf order, and grouping keeps it.
    let tracks = queries::get_library_tracks(conn, &[], filter, age_secs)
        .map_err(|_| "library query failed")?;

    // "Various Artists" bucket (compilations) — only under an artist top level.
    let compilations = if matches!(level0, GroupBy::Artist | GroupBy::AlbumArtist) {
        queries::get_compilation_albums(conn, filter, &[], age_secs)
            .map_err(|_| "library query failed")?
    } else {
        Vec::new()
    };
    if !compilations.is_empty() {
        let va_count: i64 = compilations.iter().map(|(_, count)| count).sum();
        let va_children = compilations
            .iter()
            .map(|(album, count)| LibraryTreeNode {
                kind: BrowserNodeKind::Album,
                label: album.clone(),
                key: album.clone(),
                count: *count as i32,
                secondary: "Various Artists".to_string(),
                children: tracks
                    .iter()
                    .filter(|track| group_value(track, GroupBy::Album) == *album)
                    .map(file_node)
                    .collect(),
                track: None,
            })
            .collect();
        level0_nodes.push(LibraryTreeNode {
            kind: BrowserNodeKind::VariousArtists,
            label: i18n("Various Artists"),
            key: "__various_artists__".to_string(),
            count: va_count as i32,
            secondary: String::new(),
            children: va_children,
            track: None,
        });
    }

    let all: Vec<&Track> = tracks.iter().collect();
    level0_nodes.extend(group_nodes(grouping, 0, &all));
    Ok(LibraryTreeSnapshot {
        level0: level0_nodes,
    })
}

/// The text a track groups under for `group_by`, as the SQL grouping spells it:
/// `COALESCE(column, '')`, with the integer columns cast to text.
fn group_value(track: &Track, group_by: GroupBy) -> String {
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let number = |value: Option<i32>| value.map(|n| n.to_string()).unwrap_or_default();
    match group_by {
        GroupBy::None => String::new(),
        GroupBy::Artist => text(&track.artist),
        GroupBy::AlbumArtist => text(&track.album_artist),
        GroupBy::Album => text(&track.album),
        GroupBy::Genre => text(&track.genre),
        GroupBy::Year => number(track.year),
        GroupBy::Format => text(&track.format),
    }
}

/// The group nodes of `tracks` at `level_idx`: distinct values in
/// `COLLATE NOCASE` order (ASCII case folding, the SQL grouping's order),
/// empty values left out, each counting all of its tracks. Past the last level
/// the tracks become file leaves in their incoming order.
fn group_nodes(
    grouping: &LibraryGrouping,
    level_idx: usize,
    tracks: &[&Track],
) -> Vec<LibraryTreeNode> {
    let group_by = grouping.level(level_idx);
    let mut groups: std::collections::HashMap<String, Vec<&Track>> =
        std::collections::HashMap::new();
    for track in tracks {
        groups
            .entry(group_value(track, group_by))
            .or_default()
            .push(track);
    }
    let mut groups: Vec<(String, Vec<&Track>)> = groups
        .into_iter()
        .filter(|(value, _)| !value.is_empty())
        .collect();
    groups.sort_by(|(a, _), (b, _)| {
        a.to_ascii_lowercase()
            .cmp(&b.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
    let next_idx = level_idx + 1;
    groups
        .into_iter()
        .map(|(value, members)| LibraryTreeNode {
            kind: group_by.node_kind(),
            count: members.len() as i32,
            children: if next_idx >= grouping.depth() {
                members.iter().map(|track| file_node(track)).collect()
            } else {
                group_nodes(grouping, next_idx, &members)
            },
            label: value.clone(),
            key: value,
            secondary: String::new(),
            track: None,
        })
        .collect()
}

/// A file-leaf node carrying the full track (drag/export) + the tab-joined
/// metadata the library detail binder expects (`album\tduration\tartist\thash`).
fn file_node(track: &Track) -> LibraryTreeNode {
    let secondary = format!(
        "{}\t{}\t{}\t{}",
        track.display_album(),
        track.duration_display(),
        track.display_artist(),
        track.art_hash.as_deref().unwrap_or(""),
    );
    LibraryTreeNode {
        kind: BrowserNodeKind::File,
        label: track.display_title().to_string(),
        key: track.file_path.clone(),
        count: 0,
        secondary,
        children: Vec::new(),
        track: Some(track.clone()),
    }
}

#[cfg(test)]
fn build_library_tree_snapshot(
    conn: &rusqlite::Connection,
    grouping: &LibraryGrouping,
    filter: &str,
    age: Option<i64>,
) -> LibraryTreeSnapshot {
    try_build_library_tree_snapshot(conn, grouping, filter, age).expect("valid test fixture")
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use crate::db::queries::insert_track;
    use crate::db::schema;
    use big_media_runtime::Result as MediaResult;
    use rusqlite::Connection;

    fn setup_db() -> MediaResult<Connection> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        schema::run_migrations(&conn)?;
        Ok(conn)
    }

    fn track(
        path: &str,
        artist: &str,
        album_artist: &str,
        album: &str,
        genre: &str,
        year: i32,
    ) -> Track {
        Track {
            id: 0,
            file_path: path.to_string(),
            title: Some(format!("title {path}")),
            artist: Some(artist.to_string()),
            album: Some(album.to_string()),
            album_artist: Some(album_artist.to_string()),
            track_number: Some(1),
            disc_number: None,
            year: Some(year),
            genre: Some(genre.to_string()),
            duration_secs: 100.0,
            file_size: None,
            bitrate: None,
            sample_rate: None,
            channels: None,
            format: Some("MP3".into()),
            art_hash: None,
            date_added: 0,
            last_played: None,
            play_count: 0,
            rating: 0,
            modified_at: None,
        }
    }

    /// Two "Debut" tracks by Alice, one "Solo" by Bob, and a two-artist
    /// "Comp" compilation (Carol + Dave under album_artist "VA").
    fn seed_library(conn: &Connection) -> MediaResult<()> {
        insert_track(
            conn,
            &track("/m/a1.mp3", "Alice", "Alice", "Debut", "Rock", 2000),
        )?;
        insert_track(
            conn,
            &track("/m/a2.mp3", "Alice", "Alice", "Debut", "Rock", 2000),
        )?;
        insert_track(
            conn,
            &track("/m/b1.flac", "Bob", "Bob", "Solo", "Jazz", 2010),
        )?;
        insert_track(
            conn,
            &track("/m/c1.mp3", "Carol", "VA", "Comp", "Pop", 2020),
        )?;
        insert_track(conn, &track("/m/c2.mp3", "Dave", "VA", "Comp", "Pop", 2020))?;
        Ok(())
    }

    fn artist_grouping() -> LibraryGrouping {
        LibraryGrouping {
            levels: [GroupBy::Artist, GroupBy::None, GroupBy::None],
        }
    }

    #[test]
    fn kind_from_u32_maps_every_discriminant() {
        let cases = [
            (0, BrowserNodeKind::All),
            (1, BrowserNodeKind::Artist),
            (2, BrowserNodeKind::Album),
            (3, BrowserNodeKind::Genre),
            (4, BrowserNodeKind::Year),
            (5, BrowserNodeKind::Folder),
            (6, BrowserNodeKind::Station),
            (7, BrowserNodeKind::Playlist),
            (8, BrowserNodeKind::SmartPlaylist),
            (9, BrowserNodeKind::File),
            (10, BrowserNodeKind::Divider),
            (11, BrowserNodeKind::VariousArtists),
        ];
        for (value, expected) in cases {
            assert_eq!(kind_from_u32(value), expected, "value {value}");
        }
        // Out-of-range discriminants fall back to `All`.
        assert_eq!(kind_from_u32(12), BrowserNodeKind::All);
        assert_eq!(kind_from_u32(u32::MAX), BrowserNodeKind::All);
    }

    #[test]
    fn query_all_returns_every_track() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let tracks = query_tracks_for_node(
            &conn,
            BrowserNodeKind::All,
            "",
            "",
            "",
            None,
            &artist_grouping(),
        );
        assert_eq!(tracks.len(), 5);
        Ok(())
    }

    #[test]
    fn query_file_node_resolves_single_track_or_empty() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let found = query_tracks_for_node(
            &conn,
            BrowserNodeKind::File,
            "/m/b1.flac",
            "",
            "",
            None,
            &artist_grouping(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_path, "/m/b1.flac");

        let missing = query_tracks_for_node(
            &conn,
            BrowserNodeKind::File,
            "/m/nope.flac",
            "",
            "",
            None,
            &artist_grouping(),
        );
        assert!(missing.is_empty());
        Ok(())
    }

    #[test]
    fn query_non_track_nodes_are_empty() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        for kind in [
            BrowserNodeKind::Divider,
            BrowserNodeKind::Station,
            BrowserNodeKind::Folder,
        ] {
            let tracks =
                query_tracks_for_node(&conn, kind, "anything", "", "", None, &artist_grouping());
            assert!(tracks.is_empty(), "{kind:?} should resolve to no tracks");
        }
        Ok(())
    }

    #[test]
    fn query_album_genre_year_filter_by_their_column() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let g = artist_grouping();

        let album = query_tracks_for_node(&conn, BrowserNodeKind::Album, "Debut", "", "", None, &g);
        assert_eq!(album.len(), 2);

        let genre = query_tracks_for_node(&conn, BrowserNodeKind::Genre, "Jazz", "", "", None, &g);
        assert_eq!(genre.len(), 1);
        assert_eq!(genre[0].file_path, "/m/b1.flac");

        // `year` is an INTEGER column; the node key arrives as text. This
        // exercises the `Year` column arm end-to-end (regression for the
        // int-vs-text affinity fix in big-media-data).
        let year = query_tracks_for_node(&conn, BrowserNodeKind::Year, "2020", "", "", None, &g);
        assert_eq!(year.len(), 2);
        let none = query_tracks_for_node(&conn, BrowserNodeKind::Year, "1999", "", "", None, &g);
        assert!(none.is_empty());
        Ok(())
    }

    #[test]
    fn artist_node_uses_artist_column_under_artist_grouping() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        // Plain artist grouping → filters on `artist`. "VA" is an album_artist
        // only, so it matches nothing here.
        let by_artist = query_tracks_for_node(
            &conn,
            BrowserNodeKind::Artist,
            "Alice",
            "",
            "",
            None,
            &artist_grouping(),
        );
        assert_eq!(by_artist.len(), 2);
        let va = query_tracks_for_node(
            &conn,
            BrowserNodeKind::Artist,
            "VA",
            "",
            "",
            None,
            &artist_grouping(),
        );
        assert!(va.is_empty());
        Ok(())
    }

    #[test]
    fn artist_node_uses_album_artist_column_under_album_artist_grouping() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let g = LibraryGrouping {
            levels: [GroupBy::AlbumArtist, GroupBy::None, GroupBy::None],
        };
        // Now the Artist node keys on `album_artist`; "VA" groups Carol + Dave.
        let va = query_tracks_for_node(&conn, BrowserNodeKind::Artist, "VA", "", "", None, &g);
        assert_eq!(va.len(), 2);
        // And "Alice" (album_artist) still resolves her two tracks.
        let alice =
            query_tracks_for_node(&conn, BrowserNodeKind::Artist, "Alice", "", "", None, &g);
        assert_eq!(alice.len(), 2);
        Ok(())
    }

    #[test]
    fn secondary_adds_parent_column_filter() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let g = artist_grouping();
        // Album "Debut" narrowed by parent artist=Alice keeps both tracks.
        let matched = query_tracks_for_node(
            &conn,
            BrowserNodeKind::Album,
            "Debut",
            "artist:Alice",
            "",
            None,
            &g,
        );
        assert_eq!(matched.len(), 2);
        // Narrowed by a non-matching parent artist removes them.
        let none = query_tracks_for_node(
            &conn,
            BrowserNodeKind::Album,
            "Debut",
            "artist:Bob",
            "",
            None,
            &g,
        );
        assert!(none.is_empty());
        Ok(())
    }

    #[test]
    fn various_artists_node_returns_compilation_tracks() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let tracks = query_tracks_for_node(
            &conn,
            BrowserNodeKind::VariousArtists,
            "",
            "",
            "",
            None,
            &artist_grouping(),
        );
        // Only the two-artist "Comp" album qualifies.
        assert_eq!(tracks.len(), 2);
        assert!(tracks.iter().all(|t| t.album.as_deref() == Some("Comp")));
        Ok(())
    }

    #[test]
    fn tree_empty_library_is_default() -> MediaResult<()> {
        let conn = setup_db()?;
        let tree_snapshot = build_library_tree_snapshot(&conn, &artist_grouping(), "", None);
        assert!(tree_snapshot.level0.is_empty());
        Ok(())
    }

    #[test]
    fn tree_songs_view_emits_flat_file_leaves() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let songs = LibraryGrouping {
            levels: [GroupBy::None, GroupBy::None, GroupBy::None],
        };
        let tree_snapshot = build_library_tree_snapshot(&conn, &songs, "", None);
        assert_eq!(tree_snapshot.level0.len(), 5);
        assert!(
            tree_snapshot
                .level0
                .iter()
                .all(|node| node.kind == BrowserNodeKind::File && node.children.is_empty())
        );
        Ok(())
    }

    #[test]
    fn tree_artist_grouping_has_all_tracks_then_various_then_groups() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let tree_snapshot = build_library_tree_snapshot(&conn, &artist_grouping(), "", None);

        // First node is "All Tracks" carrying the total count.
        assert_eq!(tree_snapshot.level0[0].kind, BrowserNodeKind::All);
        assert_eq!(tree_snapshot.level0[0].count, 5);

        // The compilation bucket appears under an artist top level.
        let various_artists_node = tree_snapshot
            .level0
            .iter()
            .find(|node| node.kind == BrowserNodeKind::VariousArtists);
        assert_eq!(various_artists_node.map(|node| node.count), Some(2));

        // Artist groups: Alice, Bob, Carol, Dave — each a leaf level (depth 1)
        // whose children are file nodes.
        let artist_nodes: Vec<_> = tree_snapshot
            .level0
            .iter()
            .filter(|node| node.kind == BrowserNodeKind::Artist)
            .collect();
        assert_eq!(artist_nodes.len(), 4);
        let alice = artist_nodes.iter().find(|node| node.key == "Alice");
        assert_eq!(alice.map(|node| node.count), Some(2));
        assert_eq!(alice.map(|node| node.children.len()), Some(2));
        assert!(alice.is_some_and(|node| {
            node.children
                .iter()
                .all(|child| child.kind == BrowserNodeKind::File)
        }));
        Ok(())
    }

    #[test]
    fn tree_two_level_grouping_nests_albums_under_artists() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let grouping = LibraryGrouping {
            levels: [GroupBy::Artist, GroupBy::Album, GroupBy::None],
        };
        let tree_snapshot = build_library_tree_snapshot(&conn, &grouping, "", None);
        let alice = tree_snapshot
            .level0
            .iter()
            .find(|node| node.kind == BrowserNodeKind::Artist && node.key == "Alice");
        // Alice → album "Debut" → two file leaves.
        assert_eq!(alice.map(|node| node.children.len()), Some(1));
        let debut = alice.and_then(|node| node.children.first());
        assert_eq!(debut.map(|node| node.kind), Some(BrowserNodeKind::Album));
        assert_eq!(debut.map(|node| node.key.as_str()), Some("Debut"));
        assert_eq!(debut.map(|node| node.children.len()), Some(2));
        assert!(debut.is_some_and(|node| {
            node.children
                .iter()
                .all(|child| child.kind == BrowserNodeKind::File)
        }));
        Ok(())
    }

    #[test]
    fn tree_without_artist_top_level_has_no_various_bucket() -> MediaResult<()> {
        let conn = setup_db()?;
        seed_library(&conn)?;
        let grouping = LibraryGrouping {
            levels: [GroupBy::Genre, GroupBy::None, GroupBy::None],
        };
        let tree_snapshot = build_library_tree_snapshot(&conn, &grouping, "", None);
        assert!(
            tree_snapshot
                .level0
                .iter()
                .all(|node| node.kind != BrowserNodeKind::VariousArtists)
        );
        Ok(())
    }
}
