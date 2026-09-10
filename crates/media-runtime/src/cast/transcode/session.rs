use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
// std::process retained for the streaming transcode in `spawn_stage`: stdout is
// piped to the caller and stderr is redirected to a log file (Stdio::from),
// neither of which BigSubprocessSpec models. The one-shot subtitle extract uses
// big_os_kit::subprocess.
use std::process::{ChildStdout, Command, Stdio};
use std::time::Duration;

use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::info;
use tempfile::Builder;

use crate::cast::gpu::GpuType;
use crate::error::{Error, Result};

use super::probe::probe_video_dimensions;
use super::{TranscodeFilters, TranscodeSession, TranscodeStage};

impl TranscodeSession {
    /// Create a new session (does not start FFmpeg yet).
    pub fn new(
        input_path: &str,
        gpu_type: GpuType,
        device_path: &str,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        let (video_width, video_height) = probe_video_dimensions(input_path);
        Self {
            input_path: input_path.to_string(),
            gpu_type,
            device_path: device_path.to_string(),
            filters: TranscodeFilters::default(),
            child: None,
            _stop: stop,
            extracted_sub_path: None,
            extracted_sub_track: None,
            video_width,
            video_height,
            temp_dir: None,
        }
    }

    /// Lazily create an owned 0700 scratch dir under the user's runtime/tmp
    /// location. All cast scratch files (cmd log, stderr log, extracted
    /// subtitle) go inside; this defeats `/tmp` symlink races because the
    /// attacker cannot pre-create entries in a dir they don't own.
    fn ensure_temp_dir(&mut self) -> Option<&Path> {
        if self.temp_dir.is_none() {
            match Builder::new().prefix("big-media-cast-").tempdir() {
                Ok(d) => {
                    // Force 0700 explicitly — `tempdir()` honours the
                    // process umask, which on some distros leaves the dir
                    // world-readable. The Stage 5 H-2 invariant requires
                    // owner-only access.
                    let perms = std::fs::Permissions::from_mode(0o700);
                    if let Err(e) = std::fs::set_permissions(d.path(), perms) {
                        info!("Cast: failed to lock scratch dir to 0700: {e}");
                        return None;
                    }
                    self.temp_dir = Some(d);
                }
                Err(e) => {
                    info!("Cast: failed to create scratch dir: {e}");
                    return None;
                }
            }
        }
        self.temp_dir.as_ref().map(tempfile::TempDir::path)
    }

    pub(super) fn scratch_path(&mut self, name: &str) -> Option<PathBuf> {
        self.ensure_temp_dir().map(|d| d.join(name))
    }

    /// Set the active filters.
    pub fn set_filters(&mut self, filters: TranscodeFilters) {
        self.filters = filters;
    }

    /// Get the current filters.
    pub fn filters(&self) -> &TranscodeFilters {
        &self.filters
    }

    /// GPU type used.
    #[must_use]
    pub fn gpu_type(&self) -> GpuType {
        self.gpu_type
    }

    /// Start FFmpeg, seeking to the given position. Returns the `ChildStdout` for pipe reading.
    /// If a previous process is running, it is killed first.
    ///
    /// Uses a 3-stage fallback strategy (like big-video-converter):
    /// - Stage 1: Full GPU (hwaccel decode + GPU encode) — fastest
    /// - Stage 2: Software decode + GPU encode — more compatible
    /// - Stage 3: Full software (libx264) — universal fallback
    ///
    /// If `software_decode` is set in filters, starts at Stage 2.
    /// CPU-only mode always uses Stage 3.
    pub fn start(&mut self, seek_secs: f64) -> Result<ChildStdout> {
        self.kill_child();

        if let Some(override_gpu) = self.filters.gpu_override {
            self.gpu_type = override_gpu;
        }
        if !self.filters.gpu_device_path.is_empty() {
            self.device_path = self.filters.gpu_device_path.clone();
        }

        if let Some(sub_idx) = self.filters.subtitle_track {
            if !self.filters.subtitle_is_bitmap {
                self.resolve_subtitle_extraction(sub_idx);
            }
        } else {
            self.cleanup_extracted_sub();
        }

        // Sandbox ffmpeg: it re-encodes an arbitrary media file (a large C codec
        // surface). Confine it to no network and read-only access to just the
        // input (+ any extracted subtitle); output goes to the stdout pipe. Refuse
        // rather than transcode unsandboxed if bubblewrap is missing.
        let input_path = self.input_path.clone();
        let extracted_sub = self.extracted_sub_path.clone();
        let mut ro_paths: Vec<&Path> = vec![Path::new(&input_path)];
        if let Some(sub) = &extracted_sub {
            ro_paths.push(Path::new(sub));
        }
        let Some(sandbox_opts) = crate::cast::sandbox::options(&ro_paths, &[]) else {
            return Err(Error::ffmpeg(
                "bwrap",
                None,
                "bubblewrap sandbox unavailable — refusing to transcode untrusted media",
            ));
        };

        for stage in self.candidate_stages() {
            let (gpu, device) = match stage {
                TranscodeStage::Software => (GpuType::Cpu, String::new()),
                _ => (self.gpu_type, self.device_path.clone()),
            };

            let args = self.build_ffmpeg_args(seek_secs, stage, gpu, &device);
            let cmd_line = format!("ffmpeg {}", args.join(" "));
            info!("Cast transcode ({stage:?}): {cmd_line}");
            if let Some(cmd_log) = self.scratch_path("ffmpeg-cmd.log") {
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&cmd_log)
                {
                    Ok(mut f) => {
                        let _ = f.write_all(cmd_line.as_bytes());
                    }
                    _ => {
                        if let Ok(mut f) =
                            OpenOptions::new().write(true).truncate(true).open(&cmd_log)
                        {
                            let _ = f.write_all(cmd_line.as_bytes());
                        }
                    }
                }
            }

            // bigagents: app-local-subprocess — streaming transcode: stdout is
            // handed back to the caller as a live pipe and stderr is redirected
            // to a log file via Stdio::from. BigSubprocessSpec models run/spawn
            // but not fd-level stdout-handoff + stderr-to-file, so this stays on
            // std::process::Command by design.
            let mut cmd = Command::new("bwrap");
            cmd.args(&sandbox_opts)
                .arg("ffmpeg")
                .args(&args)
                .stdout(Stdio::piped())
                .stdin(Stdio::null());

            let stderr_file = self.scratch_path("ffmpeg-stderr.log").and_then(|p| {
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&p)
                    .ok()
            });
            if let Some(file) = stderr_file {
                cmd.stderr(Stdio::from(file));
            } else {
                cmd.stderr(Stdio::null());
            }

            match cmd.spawn() {
                Ok(mut child) => {
                    std::thread::sleep(Duration::from_millis(500));
                    match child.try_wait() {
                        Ok(Some(status)) if !status.success() => {
                            info!(
                                "Cast transcode: {:?} failed (exit {}), trying next stage",
                                stage,
                                status.code().unwrap_or(-1)
                            );
                        }
                        _ => {
                            let stdout = child.stdout.take().ok_or_else(|| {
                                Error::ffmpeg("ffmpeg", None, "stdout not captured")
                            })?;
                            self.child = Some(child);
                            return Ok(stdout);
                        }
                    }
                }
                Err(e) => {
                    info!("Cast transcode: {stage:?} spawn failed: {e}");
                }
            }
        }

        Err(Error::ffmpeg("ffmpeg", None, "all transcode stages failed"))
    }

    /// Restart FFmpeg at a new seek position (for cast seek during transcoding).
    pub fn restart_at(&mut self, seek_secs: f64) -> Result<ChildStdout> {
        self.start(seek_secs)
    }

    /// Update filters and restart the transcode.
    pub fn update_filters(
        &mut self,
        filters: TranscodeFilters,
        current_secs: f64,
    ) -> Result<ChildStdout> {
        self.filters = filters;
        self.start(current_secs)
    }

    /// Kill the running FFmpeg process (if any).
    pub fn stop(&mut self) {
        self.kill_child();
        self.cleanup_extracted_sub();
    }

    /// Whether the FFmpeg process is still running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    fn candidate_stages(&self) -> Vec<TranscodeStage> {
        if self.gpu_type == GpuType::Cpu {
            vec![TranscodeStage::Software]
        } else if self.filters.software_decode || self.filters.has_video_filters() {
            vec![TranscodeStage::PartialGpu, TranscodeStage::Software]
        } else {
            vec![
                TranscodeStage::FullGpu,
                TranscodeStage::PartialGpu,
                TranscodeStage::Software,
            ]
        }
    }

    fn kill_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Extract a text subtitle stream to a temp file for faster subtitles= filter usage.
    /// Blocks until extraction completes (typically < 5s even for large files).
    /// Uses `-c copy` + MKV output to avoid re-encoding — just demuxes subtitle packets.
    /// If already extracted for this track, reuses the cached file.
    fn resolve_subtitle_extraction(&mut self, sub_idx: usize) {
        if self.extracted_sub_track == Some(sub_idx) && self.extracted_sub_path.is_some() {
            return;
        }

        self.cleanup_extracted_sub();
        self.extracted_sub_track = Some(sub_idx);

        let Some(temp_pathbuf) = self.scratch_path(&format!("sub-{sub_idx}.mkv")) else {
            info!("Cast: scratch dir unavailable, skipping subtitle extraction");
            return;
        };
        let scratch_dir = temp_pathbuf.parent().map(Path::to_path_buf);
        let temp_path = temp_pathbuf.to_string_lossy().into_owned();
        info!("Cast: extracting subtitle track {sub_idx} to {temp_path} ...");
        let map_arg = format!("0:s:{sub_idx}");
        let ffmpeg_args = [
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            self.input_path.as_str(),
            "-map",
            map_arg.as_str(),
            "-c",
            "copy",
            // `--` end-of-options: defend output even though `temp_path`
            // is generated inside an owned 0700 scratch dir.
            "--",
            temp_path.as_str(),
        ];
        // Same sandbox as the main transcode: ffmpeg demuxes the untrusted file;
        // it may only read the input and write into the scratch dir.
        let rw: Vec<&Path> = scratch_dir.as_deref().into_iter().collect();
        let Some(sandboxed) = crate::cast::sandbox::wrap_args(
            "ffmpeg",
            &ffmpeg_args,
            &[Path::new(self.input_path.as_str())],
            &rw,
        ) else {
            info!("Cast: sandbox unavailable, skipping subtitle extraction");
            return;
        };
        let status = BigSubprocessSpec::builder()
            .program("bwrap")
            .args(sandboxed)
            .stdout(BigSubprocessOutputMode::Null)
            .stderr(BigSubprocessOutputMode::Null)
            .build()
            .run();

        match status {
            Ok(o) if o.status.success() => {
                info!("Cast: subtitle extraction complete");
                self.extracted_sub_path = Some(temp_path);
            }
            _ => {
                info!("Cast: subtitle extraction failed, will use inline filter");
            }
        }
    }

    /// Clean up any extracted subtitle temp file.
    fn cleanup_extracted_sub(&mut self) {
        if let Some(path) = self.extracted_sub_path.take() {
            let _ = std::fs::remove_file(&path);
        }
        self.extracted_sub_track = None;
    }
}

impl Drop for TranscodeSession {
    fn drop(&mut self) {
        self.kill_child();
    }
}
