use super::*;

pub(super) struct TreeRequest {
    db: DbHandle,
    ticket: library_generation::LibraryTicket,
    grouping: LibraryGrouping,
    filter: String,
    age: Option<i64>,
}
fn start_tree(
    mailbox: library_generation::LatestMailbox<TreeRequest>,
    input: relm4::Sender<AudioShellInput>,
    request: TreeRequest,
) {
    let generation = request.ticket.value;
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || {
            if !request.ticket.is_current() {
                return Ok(None);
            }
            let conn = request.db.lock().map_err(|_| "library lock failed")?;
            if !request.ticket.is_current() {
                return Ok(None);
            }
            let tree = library_query::try_build_library_tree_snapshot(
                &conn,
                &request.grouping,
                &request.filter,
                request.age,
            )?;
            Ok::<_, &'static str>(request.ticket.is_current().then_some(tree))
        })
        .await;
        match result {
            Ok(Ok(Some(data))) => {
                let _ = input.send(AudioShellInput::LibraryTreeResolved { generation, data });
            }
            Ok(Ok(None)) => {} // stale: do not display empty results or an error
            _ => {
                let _ = input.send(AudioShellInput::LibraryTreeFailed { generation });
            }
        }
        if let Some(next) = mailbox.complete() {
            start_tree(mailbox, input, next);
        }
    });
}
impl AudioShell {
    pub(super) fn rebuild_library_tree(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let request = TreeRequest {
            db,
            ticket: self.library_generation.begin(),
            grouping: self.library_grouping.clone(),
            filter: self.library_filter.clone(),
            age: AgeFilter::from_index(self.library_age).seconds_ago(),
        };
        if let Some(request) = self.library_queries.submit(request) {
            start_tree(
                self.library_queries.clone(),
                sender.input_sender().clone(),
                request,
            );
        }
    }

    /// Delete a custom playlist, then refresh the manager dialog.
    pub(super) fn delete_playlist(&self, id: i64, sender: &ComponentSender<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| queries::delete_playlist(conn, id).is_ok(),
            move |success| {
                let _ = input.send(AudioShellInput::PlaylistMutationFinished(success));
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
                queries::get_all_playlists(conn).map(|playlists| {
                    playlists
                        .into_iter()
                        .map(|p| (p.id, p.name))
                        .collect::<Vec<_>>()
                })
            },
            move |playlists| match playlists {
                Ok(playlists) => {
                    let _ = input.send(AudioShellInput::AddToPlaylistResolved { track, playlists });
                }
                Err(_) => {
                    let _ = input.send(AudioShellInput::PlaylistReadFailed);
                }
            },
        );
    }

    /// Add one track atomically and report failures instead of silently losing the action.
    pub(super) fn add_track_to_playlist(
        &self,
        track_id: i64,
        playlist_id: i64,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| queries::add_track_to_playlist(conn, playlist_id, track_id).is_ok(),
            move |success| {
                let _ = input.send(AudioShellInput::PlaylistMutationFinished(success));
            },
        );
    }

    /// Create a new playlist seeded with one track, then refresh manager data.
    pub(super) fn create_playlist_with_track(
        &self,
        name: String,
        track_id: i64,
        sender: &ComponentSender<Self>,
    ) {
        self.save_tracks_as_playlist_db(name, vec![track_id], sender);
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
            move |conn| queries::create_playlist_with_tracks(conn, &name, &track_ids).is_ok(),
            move |success| {
                let _ = input.send(AudioShellInput::PlaylistMutationFinished(success));
            },
        );
    }

    /// Resolve an activated browse-tree node to its tracks off-thread, then
    /// append them to the queue (+ play when idle) via `NodeTracksResolved`.
    /// Activating an artist/album/track enqueues its tracks rather than opening
    /// a separate detail view.
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
