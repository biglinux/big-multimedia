use std::sync::Arc;
use std::sync::atomic::Ordering;

use log::info;

use super::network::start_pipe_server;
use super::{CastCommand, CastSessionController};
use crate::error::{Error, Result};

impl CastSessionController {
    /// Restarts FFmpeg with new filters mid-cast — keeps the Cast session
    /// alive (no reconnect, just `ReloadMedia` with the new pipe URL).
    pub fn transcode_update_filters(
        &self,
        filters: crate::cast::transcode::TranscodeFilters,
        current_secs: f64,
    ) -> Result<()> {
        if !self.transcoding.load(Ordering::Relaxed) {
            return Err(Error::chromecast("not transcoding"));
        }

        *self
            .active_filters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = filters.clone();

        let local_ip = self
            .local_ip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| Error::chromecast("no local IP stored"))?;

        self.stop_http.store(true, Ordering::Relaxed);
        let stdout = {
            let mut lock = self
                .transcode_session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let session = lock
                .as_mut()
                .ok_or_else(|| Error::chromecast("no transcode session"))?;
            session.update_filters(filters, current_secs)?
        };

        self.stop_http.store(false, Ordering::Relaxed);
        let (http_port, http_token) = start_pipe_server(stdout, Arc::clone(&self.stop_http))?;
        let url = format!("http://{local_ip}:{http_port}/{http_token}");

        self.send_command(CastCommand::ReloadMedia {
            url,
            mime: "video/mp4".to_string(),
            seek_secs: 0.0,
            live: true,
        });
        Ok(())
    }

    /// Direct cast → transcoded cast (seamless — no app reconnect).
    pub fn switch_to_transcoding(
        &self,
        filters: crate::cast::transcode::TranscodeFilters,
        current_secs: f64,
    ) -> Result<()> {
        if !self.is_casting() {
            return Err(Error::chromecast("not casting"));
        }

        let file_path = self
            .cast_file_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| Error::chromecast("no file path stored"))?;
        let local_ip = self
            .local_ip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| Error::chromecast("no local IP stored"))?;

        self.stop_http.store(true, Ordering::Relaxed);
        let (gpu, device_path) = self.resolve_gpu(&filters);
        let mut session = crate::cast::transcode::TranscodeSession::new(
            &file_path,
            gpu,
            &device_path,
            Arc::clone(&self.stop_http),
        );
        session.set_filters(filters.clone());
        let stdout = session.start(current_secs)?;

        *self
            .active_filters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = filters;
        *self
            .transcode_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(session);

        self.stop_http.store(false, Ordering::Relaxed);
        let (http_port, http_token) = start_pipe_server(stdout, Arc::clone(&self.stop_http))?;
        let url = format!("http://{local_ip}:{http_port}/{http_token}");

        self.transcoding.store(true, Ordering::Relaxed);
        self.send_command(CastCommand::ReloadMedia {
            url,
            mime: "video/mp4".to_string(),
            seek_secs: 0.0,
            live: true,
        });

        info!("Switched to transcoded casting at {current_secs:.1}s");
        Ok(())
    }
}
