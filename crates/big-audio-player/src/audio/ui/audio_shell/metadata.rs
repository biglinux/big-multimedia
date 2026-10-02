use super::*;

impl AudioShell {
    pub(super) fn load_external_metadata(resolved: Vec<String>, sender: &ComponentSender<Self>) {
        // Start listening immediately. Resolve the first track first, then batch
        // the remainder so a large playlist does not rebuild the UI per file.
        let input = sender.input_sender().clone();
        spawn_blocking_result(
            move || {
                let mut batch = Vec::new();
                for (index, path) in resolved.into_iter().enumerate() {
                    if Path::new(&path).is_file()
                        && let Ok(track) = crate::library::metadata::read_track_metadata(&path)
                    {
                        batch.push(track);
                    }
                    if (index == 0 || batch.len() >= 32)
                        && !batch.is_empty()
                        && input
                            .send(AudioShellInput::ExternalMetadataResolved(std::mem::take(
                                &mut batch,
                            )))
                            .is_err()
                    {
                        return;
                    }
                }
                if !batch.is_empty() {
                    let _ = input.send(AudioShellInput::ExternalMetadataResolved(batch));
                }
            },
            |()| {},
        );
    }

    pub(super) fn resolve_external_metadata(
        &mut self,
        tracks: Vec<Track>,
        sender: &ComponentSender<Self>,
    ) {
        let tracks: std::collections::HashMap<_, _> = tracks
            .into_iter()
            .map(|track| (track.file_path.clone(), track))
            .collect();
        let mut current_changed = false;
        let mut changed = false;
        for track in self
            .queue
            .tracks_mut()
            .iter_mut()
            .chain(
                self.saved_music_queue
                    .iter_mut()
                    .flat_map(|(tracks, _)| tracks.iter_mut()),
            )
            .chain(
                self.saved_podcast_queue
                    .iter_mut()
                    .flat_map(|(tracks, _)| tracks.iter_mut()),
            )
        {
            if track.id == 0
                && let Some(metadata) = tracks.get(&track.file_path)
                && track.apply_file_metadata(metadata)
            {
                changed = true;
                current_changed |= !self.is_stream && track.file_path == self.current_file;
            }
        }
        self.history.resolve_metadata(&tracks);
        if changed {
            self.commit_queue();
        }
        if current_changed {
            self.now_playing_gen = self.now_playing_gen.wrapping_add(1);
            self.sync_now_playing_card();
            self.fetch_lyrics_for_current(sender);
            self.push_mpris_metadata(sender);
        }
    }

    pub(super) fn push_mpris_metadata(&self, sender: &ComponentSender<Self>) {
        if self.is_stream {
            return;
        }
        let Some(track) = self.queue.current() else {
            return;
        };
        let path = track.file_path.clone();
        let art_artist = track.artist.clone();
        let art_album = track.album.clone();
        let r#gen = self.now_playing_gen;
        let input = sender.input_sender().clone();
        spawn_blocking_result(
            move || {
                let bytes = crate::library::metadata::extract_album_art(&path).or_else(|| {
                    let a = art_artist.as_deref()?;
                    let b = art_album.as_deref()?;
                    if a.is_empty() || b.is_empty() {
                        return None;
                    }
                    crate::library::coverart::fetch_cover_art(a, b)
                })?;
                // Decoded and written here so the main loop never sees the
                // original, which can be a 3000² JPEG.
                Some(ResolvedArt {
                    blur: big_media_components::media::images::bytes_to_thumbnail(&bytes, 16),
                    card: big_media_components::media::images::bytes_to_thumbnail(&bytes, 48),
                    url: save_mpris_art(&bytes),
                })
            },
            move |art| {
                let _ = input.send(AudioShellInput::ArtResolved { r#gen, art });
            },
        );
    }

    /// Re-evaluate the blurred album-art background against the current setting
    /// and last resolved cover (on track change, and when the Appearance toggle
    /// flips). When off, or no cover, clears it.
    pub(super) fn refresh_blur_art(&self) {
        let enabled = self.settings.get_bool_or(keys::AUDIO_ALBUM_ART_BG, true);
        let blur = if enabled {
            self.last_blur_art.clone()
        } else {
            None
        };
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetBlurArt(blur));
    }

    /// Apply resolved album art: blur background, sidebar card, MPRIS `artUrl`.
    pub(super) fn apply_resolved_art(&mut self, art: Option<ResolvedArt>) {
        let Some(art) = art else {
            self.last_blur_art = None;
            self.mpris_state
                .art_url
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            self.playback_bar
                .emit(AudioPlaybackBarInput::SetBlurArt(None));
            self.np_art.set_icon_name(Some("audio-x-generic-symbolic"));
            return;
        };
        self.last_blur_art = art.blur;
        self.refresh_blur_art();
        match &art.card {
            Some(card) => self.np_art.set_paintable(Some(card)),
            None => self.np_art.set_icon_name(Some("audio-x-generic-symbolic")),
        }
        if let Some(url) = art.url {
            self.apply_mpris_art_url(url);
        }
    }

    /// Store the cached `artUrl`; the next MPRIS snapshot publishes it.
    fn apply_mpris_art_url(&self, url: String) {
        *self
            .mpris_state
            .art_url
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = url;
    }
}

/// A track's cover, decoded off the main loop at the sizes the shell shows.
#[derive(Debug)]
pub struct ResolvedArt {
    /// 16 px source of the blurred background.
    blur: Option<gtk::gdk::Texture>,
    /// 48 px sidebar now-playing card.
    card: Option<gtk::gdk::Texture>,
    /// `file://` URL of the copy cached for MPRIS, if the write succeeded.
    url: Option<String>,
}
