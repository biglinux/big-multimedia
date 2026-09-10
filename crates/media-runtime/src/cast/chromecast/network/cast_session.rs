use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::thread;
use std::time::Duration;

use log::{info, warn};
use rust_cast::channels::media::{Media, ResumeState, StreamType};
use rust_cast::channels::receiver::CastDeviceApp;
use rust_cast::{CastDevice, ChannelMessage};

use crate::cast::chromecast::CastCommand;
use crate::error::{Error, Result};

pub(in crate::cast::chromecast) fn cast_session_loop(
    host: &str,
    port: u16,
    content_url: &str,
    mime: &str,
    casting: &AtomicBool,
    stop: &AtomicBool,
    cmd_rx: std::sync::mpsc::Receiver<CastCommand>,
) -> Result<()> {
    let device = CastDevice::connect_without_host_verification(host, port)
        .map_err(|e| Error::chromecast(format!("connection failed: {e}")))?;

    device
        .connection
        .connect("receiver-0")
        .map_err(|e| Error::chromecast(format!("connection channel: {e}")))?;

    let app = device
        .receiver
        .launch_app(&CastDeviceApp::DefaultMediaReceiver)
        .map_err(|e| Error::chromecast(format!("launch app: {e}")))?;

    let transport_id = app.transport_id.clone();
    device
        .connection
        .connect(&transport_id)
        .map_err(|e| Error::chromecast(format!("transport connect: {e}")))?;

    let media = Media {
        content_id: content_url.to_string(),
        stream_type: StreamType::Buffered,
        content_type: mime.to_string(),
        metadata: None,
        duration: None,
    };

    let status = device
        .media
        .load(&transport_id, &app.session_id, &media)
        .map_err(|e| Error::chromecast(format!("load media: {e}")))?;

    let media_session_id = Arc::new(AtomicI32::new(
        status.entries.first().map_or(0, |e| e.media_session_id),
    ));

    info!(
        "Casting to {}:{} — media loaded (session {})",
        host,
        port,
        media_session_id.load(Ordering::Relaxed)
    );
    casting.store(true, Ordering::Relaxed);

    let mut last_ping = std::time::Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if last_ping.elapsed() >= Duration::from_secs(5) {
            if device.heartbeat.ping().is_err() {
                warn!("Cast heartbeat failed");
                break;
            }
            last_ping = std::time::Instant::now();
        }

        while let Ok(cmd) = cmd_rx.try_recv() {
            let msid = media_session_id.load(Ordering::Relaxed);
            match cmd {
                CastCommand::Seek(secs) => {
                    info!("Cast seek to {secs:.1}s");
                    if let Err(e) = device.media.seek(
                        &*transport_id,
                        msid,
                        Some(secs as f32),
                        Some(ResumeState::PlaybackStart),
                    ) {
                        warn!("Cast seek failed: {e}");
                    }
                }
                CastCommand::Pause => {
                    if let Err(e) = device.media.pause(&*transport_id, msid) {
                        warn!("Cast pause failed: {e}");
                    }
                }
                CastCommand::Play => {
                    if let Err(e) = device.media.play(&*transport_id, msid) {
                        warn!("Cast play failed: {e}");
                    }
                }
                CastCommand::SetVolume(vol) => {
                    let level = vol.clamp(0.0, 1.0) as f32;
                    if let Err(e) = device.receiver.set_volume(level) {
                        warn!("Cast set_volume failed: {e}");
                    }
                }
                CastCommand::ReloadMedia {
                    url,
                    mime,
                    seek_secs,
                    live,
                } => {
                    info!("Cast reload media: {url} (seek {seek_secs:.1}s, live={live})");
                    let host2 = host.to_string();
                    let transport_id2 = transport_id.clone();
                    let session_id2 = app.session_id.clone();
                    let msid_ref = Arc::clone(&media_session_id);
                    thread::spawn(move || {
                        let result = (|| -> Result<()> {
                            let dev2 = CastDevice::connect_without_host_verification(&host2, port)
                                .map_err(|e| Error::chromecast(format!("reload connect: {e}")))?;
                            dev2.connection
                                .connect("receiver-0")
                                .map_err(|e| Error::chromecast(format!("reload conn: {e}")))?;
                            dev2.connection
                                .connect(&*transport_id2)
                                .map_err(|e| Error::chromecast(format!("reload transport: {e}")))?;

                            let new_media = Media {
                                content_id: url,
                                stream_type: if live {
                                    StreamType::Live
                                } else {
                                    StreamType::Buffered
                                },
                                content_type: mime,
                                metadata: None,
                                duration: None,
                            };
                            let new_status = dev2
                                .media
                                .load(&transport_id2, &session_id2, &new_media)
                                .map_err(|e| Error::chromecast(format!("reload load: {e}")))?;
                            let new_msid = new_status
                                .entries
                                .first()
                                .map_or(msid, |e| e.media_session_id);
                            msid_ref.store(new_msid, Ordering::Relaxed);
                            info!("Cast reload complete (session {new_msid})");

                            if seek_secs > 0.5 {
                                let _ = dev2.media.seek(
                                    &*transport_id2,
                                    new_msid,
                                    Some(seek_secs as f32),
                                    Some(ResumeState::PlaybackStart),
                                );
                            }
                            Ok(())
                        })();
                        if let Err(e) = result {
                            warn!("Cast reload thread failed: {e}");
                        }
                    });
                }
            }
        }

        match device.receive() {
            Ok(ChannelMessage::Heartbeat(_)) => {
                let _ = device.heartbeat.pong();
            }
            Ok(_) => {}
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
            }
        }

        thread::sleep(Duration::from_millis(50));
    }

    let final_msid = media_session_id.load(Ordering::Relaxed);
    let _ = device.media.stop(&transport_id, final_msid);
    let _ = device.receiver.stop_app(&app.session_id);
    let _ = device.connection.disconnect(&transport_id);
    let _ = device.connection.disconnect("receiver-0");
    info!("Cast session ended — full cleanup done");
    Ok(())
}
