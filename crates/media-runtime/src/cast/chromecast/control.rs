use std::sync::atomic::Ordering;
use std::time::Duration;

use log::warn;

use super::{CastCommand, CastSessionController};

impl CastSessionController {
    pub fn send_command(&self, cmd: CastCommand) {
        if let Some(tx) = self
            .cmd_tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            && tx.send(cmd).is_err()
        {
            warn!("Cast command send failed — session may have ended");
        }
    }

    pub fn stop_casting(&self) {
        self.stop_cast.store(true, Ordering::Relaxed);
        self.stop_http.store(true, Ordering::Relaxed);
        *self
            .cmd_tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        if let Ok(mut lock) = self.transcode_session.lock()
            && let Some(mut s) = lock.take()
        {
            s.stop();
        }
        *self
            .local_ip
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .cast_file_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

        if let Some(handle) = self
            .cast_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = handle.join();
                let _ = tx.send(());
            });
            if rx.recv_timeout(Duration::from_secs(5)).is_err() {
                warn!("Cast thread did not stop within 5s — forced disconnect");
            }
        }
        self.set_cast_state(None, false);
    }
}
