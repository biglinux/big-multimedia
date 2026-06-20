use super::*;

impl AudioShell {
    pub(super) fn rebuild_library_tree(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let grouping = self.library_grouping.clone();
        let filter = self.library_filter.clone();
        let age = AgeFilter::from_index(self.library_age).seconds_ago();
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| library_query::build_library_tree_data(conn, &grouping, &filter, age),
            move |data| {
                let _ = input.send(AudioShellInput::LibraryTreeResolved(data));
            },
        );
    }

    /// Delete a custom playlist, then refresh the manager dialog.
    pub(super) fn delete_playlist(&self, id: i64, sender: &ComponentSender<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                let _ = queries::delete_playlist(conn, id);
            },
            move |()| {
                let _ = input.send(AudioShellInput::RefreshPlaylists);
            },
        );
    }

    /// Resolve the existing playlists off-thread, then open the add-to-playlist
    /// picker (shared library dialog) for `track`.
    pub(super) fn request_add_to_playlist(&self, track: Track, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                queries::get_all_playlists(conn)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| (p.id, p.name))
                    .collect::<Vec<_>>()
            },
            move |playlists| {
                let _ = input.send(AudioShellInput::AddToPlaylistResolved { track, playlists });
            },
        );
    }

    /// Add a track to an existing playlist (fire-and-forget).
    pub(super) fn add_track_to_playlist(&self, track_id: i64, playlist_id: i64) {
        let Some(db) = self.db.clone() else {
            return;
        };
        spawn_db_query(
            db,
            move |conn| {
                let _ = queries::add_track_to_playlist(conn, playlist_id, track_id);
            },
            |()| {},
        );
    }

    /// Create a new playlist seeded with one track, then refresh manager data.
    pub(super) fn create_playlist_with_track(
        &self,
        name: String,
        track_id: i64,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                if let Ok(playlist_id) = queries::create_playlist(conn, &name) {
                    let _ = queries::add_track_to_playlist(conn, playlist_id, track_id);
                }
            },
            move |()| {
                let _ = input.send(AudioShellInput::RefreshPlaylists);
            },
        );
    }

    /// Create a playlist from a set of track ids, then refresh manager data.
    pub(super) fn save_tracks_as_playlist_db(
        &self,
        name: String,
        track_ids: Vec<i64>,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                if let Ok(playlist_id) = queries::create_playlist(conn, &name) {
                    for id in &track_ids {
                        let _ = queries::add_track_to_playlist(conn, playlist_id, *id);
                    }
                }
            },
            move |()| {
                let _ = input.send(AudioShellInput::RefreshPlaylists);
            },
        );
    }

    /// Resolve an activated browse-tree node to its tracks off-thread, then
    /// append them to the queue (+ play when idle) via `NodeTracksResolved`.
    /// Mirrors the god-object Music tab: activating an artist/album/track
    /// enqueues its tracks rather than opening a separate detail view.
    pub(super) fn resolve_node_and_enqueue(
        &self,
        kind: u32,
        key: String,
        secondary: String,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let node_kind = library_query::kind_from_u32(kind);
        let grouping = self.library_grouping.clone();
        let filter = self.library_filter.clone();
        let age = AgeFilter::from_index(self.library_age).seconds_ago();
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                library_query::query_tracks_for_node(
                    conn, node_kind, &key, &secondary, &filter, age, &grouping,
                )
            },
            move |tracks| {
                let _ = input.send(AudioShellInput::NodeTracksResolved(tracks));
            },
        );
    }

    /// Resolve dropped/opened paths to library tracks off-thread, then insert
    /// them via `DroppedResolved` (the async result can't borrow `self`).
    pub(super) fn resolve_and_insert(
        &self,
        insert_pos: usize,
        paths: Vec<String>,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                paths
                    .iter()
                    .filter_map(|p| queries::get_track_by_path(conn, p).ok().flatten())
                    .collect::<Vec<Track>>()
            },
            move |tracks| {
                if !tracks.is_empty() {
                    let _ = input.send(AudioShellInput::DroppedResolved { insert_pos, tracks });
                }
            },
        );
    }
}
