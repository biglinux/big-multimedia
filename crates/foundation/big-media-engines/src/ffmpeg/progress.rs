//! FFmpeg `-progress pipe:2` parser.

use std::io::BufRead;
use std::time::Instant;

/// One snapshot of FFmpeg progress derived from `key=value` output.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq)]
pub struct FfmpegProgressSnapshot {
    pub time_secs: f64,
    pub total_secs: f64,
    pub speed: f64,
    pub size_bytes: u64,
    pub bitrate_kbps: f64,
    pub frame: u64,
    pub fps: f64,
    pub eta_secs: f64,
    pub percent: f64,
    pub done: bool,
}

impl Default for FfmpegProgressSnapshot {
    fn default() -> Self {
        Self {
            time_secs: 0.0,
            total_secs: 0.0,
            speed: 0.0,
            size_bytes: 0,
            bitrate_kbps: 0.0,
            frame: 0,
            fps: 0.0,
            eta_secs: 0.0,
            percent: 0.0,
            done: false,
        }
    }
}

/// Parse FFmpeg `-progress pipe:2` lines and emit snapshots.
///
/// Returns at most the last 20 stderr lines that did not match `key=value`.
pub fn parse_progress<R: BufRead, F: FnMut(FfmpegProgressSnapshot)>(
    reader: R,
    total_duration_secs: f64,
    on_progress: F,
) -> Vec<String> {
    parse_progress_inner(reader, total_duration_secs, on_progress, |_| {})
}

/// Parse FFmpeg progress and forward non-`key=value` stderr lines as they arrive.
pub fn parse_progress_live<R, F, S>(
    reader: R,
    total_duration_secs: f64,
    on_progress: F,
    on_stderr: S,
) -> Vec<String>
where
    R: BufRead,
    F: FnMut(FfmpegProgressSnapshot),
    S: FnMut(&str),
{
    parse_progress_inner(reader, total_duration_secs, on_progress, on_stderr)
}

fn parse_progress_inner<R, F, S>(
    mut reader: R,
    total_duration_secs: f64,
    mut on_progress: F,
    mut on_stderr: S,
) -> Vec<String>
where
    R: BufRead,
    F: FnMut(FfmpegProgressSnapshot),
    S: FnMut(&str),
{
    let mut snapshot = FfmpegProgressSnapshot {
        total_secs: total_duration_secs,
        ..Default::default()
    };
    let start = Instant::now();
    let mut line = String::new();
    let mut error_lines = Vec::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            _ => {}
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            on_stderr(trimmed);
            push_capped(&mut error_lines, trimmed);
            continue;
        };

        if apply_progress_field(&mut snapshot, key, value) {
            continue;
        }

        if key == "progress" {
            if value == "end" {
                snapshot.done = true;
                snapshot.percent = 100.0;
                on_progress(snapshot.clone());
                return error_lines;
            }
            update_eta_percent(
                &mut snapshot,
                total_duration_secs,
                start.elapsed().as_secs_f64(),
            );
            on_progress(snapshot.clone());
        }
    }

    error_lines
}

fn apply_progress_field(snapshot: &mut FfmpegProgressSnapshot, key: &str, value: &str) -> bool {
    match key {
        "frame" => snapshot.frame = value.parse().unwrap_or(0),
        "fps" => snapshot.fps = value.trim().parse().unwrap_or(0.0),
        "out_time_us" => snapshot.time_secs = value.parse::<f64>().unwrap_or(0.0) / 1_000_000.0,
        "out_time_ms" => snapshot.time_secs = value.parse::<f64>().unwrap_or(0.0) / 1_000_000.0,
        "out_time" => snapshot.time_secs = parse_time(value),
        "total_size" => snapshot.size_bytes = value.parse().unwrap_or(0),
        "bitrate" => {
            snapshot.bitrate_kbps = value.trim_end_matches("kbits/s").parse().unwrap_or(0.0)
        }
        "speed" => snapshot.speed = value.trim_end_matches('x').parse().unwrap_or(0.0),
        _ => return false,
    }
    true
}

fn update_eta_percent(snapshot: &mut FfmpegProgressSnapshot, total: f64, elapsed: f64) {
    if total > 0.0 && snapshot.time_secs > 0.0 {
        snapshot.percent = (snapshot.time_secs / total * 100.0).min(100.0);
        let remaining_ratio = (total - snapshot.time_secs) / snapshot.time_secs;
        snapshot.eta_secs = (elapsed * remaining_ratio).max(0.0);
    }
}

fn parse_time(time_text: &str) -> f64 {
    let parts: Vec<&str> = time_text.split(':').collect();
    if parts.len() == 3 {
        let hours: f64 = parts[0].parse().unwrap_or(0.0);
        let minutes: f64 = parts[1].parse().unwrap_or(0.0);
        let seconds: f64 = parts[2].parse().unwrap_or(0.0);
        hours * 3600.0 + minutes * 60.0 + seconds
    } else {
        0.0
    }
}

fn push_capped(buffer: &mut Vec<String>, line: &str) {
    if buffer.len() >= 20 {
        buffer.remove(0);
    }
    buffer.push(line.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_complete_progress_flow() {
        let progress_lines = "\
frame=100\n\
out_time=00:00:10.000\n\
total_size=1048576\n\
bitrate=800.0kbits/s\n\
speed=2.5x\n\
progress=continue\n\
frame=200\n\
out_time=00:00:20.000\n\
total_size=2097152\n\
bitrate=850.0kbits/s\n\
speed=2.3x\n\
progress=end\n";
        let mut updates = Vec::new();

        let error_lines = parse_progress(Cursor::new(progress_lines), 20.0, |snapshot| {
            updates.push(snapshot);
        });

        assert!(error_lines.is_empty());
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].frame, 100);
        assert!((updates[0].time_secs - 10.0).abs() < 0.001);
        assert!((updates[0].total_secs - 20.0).abs() < f64::EPSILON);
        assert!((updates[0].percent - 50.0).abs() < 1.0);
        assert!(updates[1].done);
        assert!((updates[1].percent - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_alternate_time_units_and_numeric_fields() {
        let progress_lines = "\
out_time_us=5000000\n\
progress=continue\n\
out_time_ms=7000000\n\
frame=42\n\
fps=29.97\n\
bitrate=1234.5kbits/s\n\
speed=1.75x\n\
total_size=12345\n\
progress=continue\n";
        let mut updates = Vec::new();

        let _ = parse_progress(Cursor::new(progress_lines), 10.0, |snapshot| {
            updates.push(snapshot);
        });

        assert!((updates[0].time_secs - 5.0).abs() < 0.001);
        assert!((updates[1].time_secs - 7.0).abs() < 0.001);
        assert_eq!(updates[1].frame, 42);
        assert!((updates[1].fps - 29.97).abs() < 0.001);
        assert!((updates[1].bitrate_kbps - 1234.5).abs() < 0.001);
        assert!((updates[1].speed - 1.75).abs() < 0.001);
        assert_eq!(updates[1].size_bytes, 12_345);
    }

    #[test]
    fn collects_stderr_ring_buffer_and_live_lines() {
        let mut progress_lines = String::new();
        for index in 0..30 {
            progress_lines.push_str(&format!("error line {index}\n"));
        }
        progress_lines.push_str("progress=end\n");
        let mut live_lines = Vec::new();

        let collected = parse_progress_live(
            Cursor::new(progress_lines),
            0.0,
            |_| {},
            |line| live_lines.push(line.to_string()),
        );

        assert_eq!(live_lines.len(), 30);
        assert_eq!(collected.len(), 20);
        assert_eq!(collected.first().map(String::as_str), Some("error line 10"));
        assert_eq!(collected.last().map(String::as_str), Some("error line 29"));
    }

    #[test]
    fn leaves_percent_unknown_without_positive_total_or_time() {
        let mut zero_total = FfmpegProgressSnapshot {
            time_secs: 50.0,
            ..Default::default()
        };
        update_eta_percent(&mut zero_total, 0.0, 10.0);
        assert_eq!(zero_total.percent, 0.0);
        assert_eq!(zero_total.eta_secs, 0.0);

        let mut zero_time = FfmpegProgressSnapshot {
            time_secs: 0.0,
            ..Default::default()
        };
        update_eta_percent(&mut zero_time, 100.0, 10.0);
        assert_eq!(zero_time.percent, 0.0);
        assert_eq!(zero_time.eta_secs, 0.0);
    }
}
