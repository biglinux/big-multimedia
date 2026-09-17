use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use log::info;

use super::network::{guess_mime, start_file_server, start_pipe_server};
use super::{CastCommand, CastSessionController};
use crate::error::{Error, Result};

impl CastSessionController {
    /// Kills FFmpeg, restarts at the new position, opens a fresh HTTP server,
    /// and sends `ReloadMedia` — Cast cannot seek inside a piped MP4 stream.
    pub fn transcode_seek(&self, seek_secs: f64) -> Result<()> {
        if !self.transcoding.load(Ordering::Relaxed) {
            return Err(Error::chromecast("not transcoding"));
        }

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
            session.restart_at(seek_secs)?
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
        let (gpu, device_path) = if let Some(override_gpu) = filters.gpu_override {
            (override_gpu, filters.gpu_device_path.clone())
        } else {
            crate::cast::gpu::detect_gpu_device()
        };
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

    /// Switches file without tearing down the Cast session — re-probes
    /// compatibility and picks direct vs transcoded path automatically.
    pub fn cast_switch_file(
        &self,
        new_file_path: &str,
        local_ip: &str,
        filters: crate::cast::transcode::TranscodeFilters,
        start_secs: f64,
    ) -> Result<()> {
        if !self.is_casting() {
            return Err(Error::chromecast("not casting"));
        }
        if !Path::new(new_file_path).exists() {
            return Err(Error::not_found("file"));
        }

        self.stop_http.store(true, Ordering::Relaxed);
        if let Ok(mut lock) = self.transcode_session.lock()
            && let Some(mut s) = lock.take()
        {
            s.stop();
        }

        *self
            .cast_file_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(new_file_path.to_string());
        *self
            .local_ip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(local_ip.to_string());

        let probe = crate::cast::probe::probe_cast_compatibility(new_file_path);
        let direct_ok = probe.as_ref().is_ok_and(|p| p.direct_compatible);
        let needs_transcode = !direct_ok || filters.has_any_filter();

        self.stop_http.store(false, Ordering::Relaxed);

        if needs_transcode {
            if !crate::cast::gpu::ffmpeg_available() {
                return Err(Error::ffmpeg(
                    "ffmpeg",
                    None,
                    "ffmpeg is not installed — required for transcoding",
                ));
            }
            let (gpu, device_path) = if let Some(override_gpu) = filters.gpu_override {
                (override_gpu, filters.gpu_device_path.clone())
            } else {
                crate::cast::gpu::detect_gpu_device()
            };
            let mut session = crate::cast::transcode::TranscodeSession::new(
                new_file_path,
                gpu,
                &device_path,
                Arc::clone(&self.stop_http),
            );
            session.set_filters(filters.clone());
            let stdout = session.start(start_secs)?;

            *self
                .active_filters
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = filters;
            *self
                .transcode_session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(session);

            let (http_port, http_token) = start_pipe_server(stdout, Arc::clone(&self.stop_http))?;
            let url = format!("http://{local_ip}:{http_port}/{http_token}");

            self.transcoding.store(true, Ordering::Relaxed);
            self.send_command(CastCommand::ReloadMedia {
                url,
                mime: "video/mp4".to_string(),
                seek_secs: 0.0,
                live: true,
            });
        } else {
            let mime = guess_mime(new_file_path);
            let (http_port, http_token) =
                start_file_server(new_file_path, Arc::clone(&self.stop_http))?;
            let url = format!("http://{local_ip}:{http_port}/{http_token}");

            self.transcoding.store(false, Ordering::Relaxed);
            self.send_command(CastCommand::ReloadMedia {
                url,
                mime,
                seek_secs: start_secs,
                live: false,
            });
        }

        info!(
            "Cast switched to {}",
            big_os_kit::url::MediaLogTarget(new_file_path)
        );
        Ok(())
    }
}
