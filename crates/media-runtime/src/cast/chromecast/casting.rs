use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use log::error;

use super::network::{cast_session_loop, guess_mime, start_file_server, start_pipe_server};
use super::{CastCommand, CastSessionController, DiscoveredCastDevice};
use crate::error::{Error, Result};

impl CastSessionController {
    pub fn cast_file(
        &self,
        device: &DiscoveredCastDevice,
        file_path: &str,
        local_ip: &str,
    ) -> Result<()> {
        if !Path::new(file_path).exists() {
            return Err(Error::not_found("file"));
        }

        let mime = guess_mime(file_path);
        self.stop_http.store(false, Ordering::Relaxed);
        let (http_port, http_token) = start_file_server(file_path, Arc::clone(&self.stop_http))?;
        let content_url = format!("http://{local_ip}:{http_port}/{http_token}");

        *self
            .local_ip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(local_ip.to_string());
        *self
            .cast_file_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(file_path.to_string());

        let rx = self.replace_command_channel();
        let handle = spawn_cast_thread(
            device,
            &content_url,
            &mime,
            Arc::clone(&self.casting),
            Arc::clone(&self.stop_cast),
            rx,
        );

        *self
            .cast_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
        self.casting.store(true, Ordering::Relaxed);
        self.set_cast_state(Some(device.name.clone()), false);
        Ok(())
    }

    pub fn cast_url(&self, device: &DiscoveredCastDevice, url: &str, mime: &str) -> Result<()> {
        let rx = self.replace_command_channel();
        let handle = spawn_cast_thread(
            device,
            url,
            mime,
            Arc::clone(&self.casting),
            Arc::clone(&self.stop_cast),
            rx,
        );

        *self
            .cast_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
        self.casting.store(true, Ordering::Relaxed);
        self.set_cast_state(Some(device.name.clone()), false);
        Ok(())
    }

    /// FFmpeg transcoding path — used when the device can't decode the
    /// container natively, or when the user enabled filters/effects.
    pub fn cast_file_transcoded(
        &self,
        device: &DiscoveredCastDevice,
        file_path: &str,
        local_ip: &str,
        filters: crate::cast::transcode::TranscodeFilters,
        start_secs: f64,
    ) -> Result<()> {
        if !Path::new(file_path).exists() {
            return Err(Error::not_found("file"));
        }

        let (gpu, device_path) = self.resolve_gpu(&filters);
        self.stop_http.store(false, Ordering::Relaxed);

        let mut session = crate::cast::transcode::TranscodeSession::new(
            file_path,
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
        *self
            .local_ip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(local_ip.to_string());
        *self
            .cast_file_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(file_path.to_string());

        let (http_port, http_token) = start_pipe_server(stdout, Arc::clone(&self.stop_http))?;
        let content_url = format!("http://{local_ip}:{http_port}/{http_token}");

        let rx = self.replace_command_channel();
        let mime = "video/mp4";
        let casting = Arc::clone(&self.casting);
        let stop = Arc::clone(&self.stop_cast);
        let host = device.host.clone();
        let port = device.port;
        let transcode_session = Arc::clone(&self.transcode_session);
        stop.store(false, Ordering::Relaxed);

        let handle = thread::spawn(move || {
            if let Err(e) = cast_session_loop(&host, port, &content_url, mime, &casting, &stop, rx)
            {
                error!("Cast transcode session error: {e}");
            }
            if let Ok(mut lock) = transcode_session.lock()
                && let Some(mut s) = lock.take()
            {
                s.stop();
            }
            casting.store(false, Ordering::Relaxed);
        });

        *self
            .cast_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
        self.casting.store(true, Ordering::Relaxed);
        self.set_cast_state(Some(device.name.clone()), true);
        Ok(())
    }

    fn replace_command_channel(&self) -> std::sync::mpsc::Receiver<CastCommand> {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut sender_lock = self
            .cmd_tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *sender_lock = Some(tx);
        rx
    }
}

fn spawn_cast_thread(
    device: &DiscoveredCastDevice,
    content_url: &str,
    mime: &str,
    casting: Arc<std::sync::atomic::AtomicBool>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    rx: std::sync::mpsc::Receiver<CastCommand>,
) -> thread::JoinHandle<()> {
    let host = device.host.clone();
    let port = device.port;
    let content_url = content_url.to_string();
    let mime = mime.to_string();
    stop.store(false, Ordering::Relaxed);

    thread::spawn(move || {
        if let Err(e) = cast_session_loop(&host, port, &content_url, &mime, &casting, &stop, rx) {
            error!("Cast session error: {e}");
        }
        casting.store(false, Ordering::Relaxed);
    })
}
