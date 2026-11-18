use super::*;
use crate::player::mpv_backend::types::{AudioCacheProfile, TargetSource};
use std::time::{Duration, Instant};

fn wait_until(label: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {label}");
}

/// Run `program` and return its stdout, failing the test on error.
fn run(program: &str, args: &[&str]) -> Vec<u8> {
    let output = big_os_kit::subprocess::BigSubprocessSpec::builder()
        .program(program)
        .args(args)
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .unwrap();
    assert!(output.status.success(), "{}", output.stderr_lossy());
    output.stdout
}

#[test]
#[ignore = "requires BIG_MEDIA_TEST_AUDIO_ISOLATED=1, private PipeWire/Pulse and FFmpeg"]
fn native_sound_server_volume_tracks_external_changes() {
    assert_eq!(
        std::env::var("BIG_MEDIA_TEST_AUDIO_ISOLATED").as_deref(),
        Ok("1")
    );
    let temp = tempfile::tempdir().unwrap();
    let input = twenty_seconds_of_silence(temp.path());
    for driver in ["pulse", "pipewire"] {
        let name = format!("volume-test-{}-{driver}", std::process::id());
        let backend = muted_at_37_percent(driver, &name, &input);
        let (index, muted, percent) = sink_input(&name);
        assert!(muted, "{driver}: actual server mute");
        assert!((percent - 37.0).abs() < 1.0, "{driver}: server={percent}%");
        run("pactl", &["set-sink-input-volume", &index, "72%"]);
        run("pactl", &["set-sink-input-mute", &index, "0"]);
        wait_until("external mixer changes", || {
            (backend.get_volume() - 72.0).abs() < 0.1 && !backend.get_mute()
        });
        assert_eq!(
            backend.get_property_f64("volume"),
            100.0,
            "{driver}: no double gain"
        );
        backend.set_volume_max(250);
        backend.set_volume(170.0);
        wait_until("amplified volume", || {
            (backend.get_property_f64("ao-volume") - 100.0).abs() < 0.1
                && backend.get_property_f64("volume") == 170.0
        });
        backend.set_volume(72.0);
        backend.pause();
        std::thread::sleep(Duration::from_secs(1));
        run("pactl", &["set-sink-input-mute", &index, "1"]);
        backend.play();
        wait_until("external mute made while paused", || backend.get_mute());
        backend.set_mute(false);
        wait_until("unmute applied", || !backend.get_property_bool("ao-mute"));
        backend.stop();
        wait_until("output closed", || backend.get_current_ao().is_empty());
        backend.load_file(input.to_str().unwrap(), TargetSource::Person);
        wait_until("controls after reopening", || {
            backend.get_current_ao() == driver
                && (backend.get_volume() - 72.0).abs() < 0.1
                && !backend.get_mute()
        });
        backend.play();
        assert_volume_survives_rate_changes(&backend, driver);
    }
}

/// A backend on `driver` whose 37% muted volume, set before any output
/// exists, has moved to the sound server's stream once `input` plays.
fn muted_at_37_percent(driver: &str, name: &str, input: &std::path::Path) -> MpvBackend {
    let mpv = MpvClient::with_initializer(|init| {
        apply_audio_only_options(init)?;
        init.set_option("audio-client-name", name)?;
        init.set_option("ao", driver)
    })
    .unwrap();
    let (tx, _rx) = async_channel::unbounded();
    let backend =
        MpvBackend::finish_mpv_init(mpv, tx, "test-native-volume", "test native volume", true);
    backend.set_volume(37.0);
    backend.set_mute(true);
    wait_until("pending software volume", || {
        backend.get_property_f64("volume") == 37.0 && backend.get_property_bool("mute")
    });
    backend.load_file(input.to_str().unwrap(), TargetSource::Person);
    wait_until("native output and transferred controls", || {
        backend.get_current_ao() == driver
            && (backend.get_volume() - 37.0).abs() < 0.1
            && backend.get_property_bool("ao-mute")
            && backend.get_property_f64("volume") == 100.0
            && !backend.get_property_bool("mute")
    });
    backend
}

/// 20 s of 48 kHz stereo silence, long enough for the pauses and reopenings.
fn twenty_seconds_of_silence(dir: &std::path::Path) -> std::path::PathBuf {
    let input = dir.join("silence.wav");
    run(
        "ffmpeg",
        &[
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo",
            "-t",
            "20",
            "-c:a",
            "pcm_s16le",
            input.to_str().unwrap(),
        ],
    );
    input
}

/// Index, mute and volume percent of the sound server's stream named `name`.
fn sink_input(name: &str) -> (String, bool, f64) {
    let listed = run("pactl", &["-f", "json", "list", "sink-inputs"]);
    let streams: Vec<serde_json::Value> = serde_json::from_slice(&listed).unwrap();
    let stream = streams
        .iter()
        .find(|s| s["properties"]["application.name"] == name)
        .expect("test stream visible to the sound server");
    let percent = stream["volume"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()["value_percent"]
        .as_str()
        .unwrap()
        .trim_end_matches('%')
        .parse::<f64>()
        .unwrap();
    let index = stream["index"].as_u64().unwrap().to_string();
    (index, stream["mute"] == true, percent)
}

/// Changing the effect chain reopens the output at the chain's rate. A new
/// stream reports 0 volume until the server applies ours, and that read-back
/// must not become the player's 72% volume.
fn assert_volume_survives_rate_changes(backend: &MpvBackend, driver: &str) {
    for rate in [44_100, 48_000, 44_100] {
        backend.with_mpv(|mpv| {
            mpv.set_property("af", format!("lavfi=[aresample={rate}]"))
                .unwrap();
            mpv.command("ao-reload", &[]).unwrap();
        });
        wait_until("output reopened at the chain's rate", || {
            backend.with_mpv(|mpv| {
                mpv.get_property::<i64>("audio-out-params/samplerate")
                    .is_ok_and(|value| value == rate)
            })
        });
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            (backend.get_volume() - 72.0).abs() < 0.1,
            "{driver}: volume after the chain changed to {rate} Hz: {}",
            backend.get_volume()
        );
    }
}

/// Voluntary context switches of this process's thread named `name`.
fn thread_wakeups(name: &str) -> u64 {
    for task in std::fs::read_dir("/proc/self/task").unwrap() {
        let task = task.unwrap().path();
        // Threads of other tests in this process may exit while we look.
        let Ok(comm) = std::fs::read_to_string(task.join("comm")) else {
            continue;
        };
        if comm.trim() != name {
            continue;
        }
        let status = std::fs::read_to_string(task.join("status")).unwrap();
        return status
            .lines()
            .find_map(|line| line.strip_prefix("voluntary_ctxt_switches:"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
    }
    panic!("no thread named {name}");
}

/// An audio-only backend on mpv's null output, its event thread named `thread`.
fn null_output_backend(thread: &str) -> (MpvBackend, async_channel::Receiver<PlayerEvent>) {
    let mpv = MpvClient::with_initializer(|init| {
        apply_audio_only_options(init)?;
        init.set_option("ao", "null")
    })
    .unwrap();
    let (tx, rx) = async_channel::unbounded();
    let backend = MpvBackend::finish_mpv_init(mpv, tx, thread, thread, true);
    (backend, rx)
}

#[test]
fn idle_event_thread_waits_for_mpv() {
    let (backend, _rx) = null_output_backend("test-idle");
    let wakeups = wakeups_in_one_second("test-idle");
    assert!(wakeups <= 1, "{wakeups} wakeups with nothing loaded");
    backend.set_volume(40.0);
    wait_until("volume applied while idle", || {
        backend.get_property_f64("volume") == 40.0
    });
}

#[test]
fn volume_mute_work_without_pulse() {
    let (backend, rx) = null_output_backend("test-volume");
    for value in 0..1000 {
        backend.set_volume(f64::from(value % 100));
    }
    backend.set_volume(37.0);
    backend.set_mute(true);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if backend.get_property_f64("volume") == 37.0 && backend.get_property_bool("mute") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(backend.get_property_f64("volume"), 37.0);
    assert!(backend.get_property_bool("mute"));
    assert_eq!(backend.get_volume(), 37.0);
    assert!(backend.get_mute());
    let mut volume_event = false;
    let mut mute_event = false;
    while Instant::now() < deadline && !(volume_event && mute_event) {
        for event in std::iter::from_fn(|| rx.try_recv().ok()) {
            volume_event |= matches!(event, PlayerEvent::VolumeChanged(v) if v == 37.0);
            mute_event |= matches!(event, PlayerEvent::MuteChanged(true));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(volume_event, "applied volume must reach the UI");
    assert!(mute_event, "applied mute must reach the UI");
}

/// Write four seconds of 8 kHz mono 16-bit silence to `dir/silence.wav`.
fn write_silence(dir: &std::path::Path) -> String {
    let input = dir.join("silence.wav");
    let data_len: u32 = 8000 * 2 * 4;
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data_len).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    for field in [16_u32, 0x0001_0001, 8000, 16000, 0x0010_0002] {
        wav.extend(field.to_le_bytes());
    }
    wav.extend(b"data");
    wav.extend(data_len.to_le_bytes());
    wav.resize(wav.len() + data_len as usize, 0);
    std::fs::write(&input, wav).unwrap();
    input.to_str().unwrap().to_owned()
}

/// Wait until an event received so far, kept in `seen`, satisfies `want`.
fn wait_for_event(
    rx: &async_channel::Receiver<PlayerEvent>,
    seen: &mut Vec<PlayerEvent>,
    label: &str,
    want: impl Fn(&PlayerEvent) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        seen.extend(std::iter::from_fn(|| rx.try_recv().ok()));
        if seen.iter().any(&want) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {label}: {seen:?}");
}

fn wakeups_in_one_second(thread: &str) -> u64 {
    std::thread::sleep(Duration::from_millis(500));
    let before = thread_wakeups(thread);
    std::thread::sleep(Duration::from_secs(1));
    thread_wakeups(thread) - before
}

#[test]
fn paused_event_thread_waits_for_mpv() {
    let temp = tempfile::tempdir().unwrap();
    let input = write_silence(temp.path());
    let (backend, rx) = null_output_backend("test-paused");
    let mut seen = Vec::new();
    backend.load_audio_paused(&input, TargetSource::Person, AudioCacheProfile::Music, true);
    wait_for_event(&rx, &mut seen, "file loaded", |e| {
        matches!(e, PlayerEvent::FileLoaded(_))
    });
    wait_for_event(
        &rx,
        &mut seen,
        "duration",
        |e| matches!(e, PlayerEvent::DurationChanged(d) if (d - 4.0).abs() < 0.01),
    );
    wait_for_event(&rx, &mut seen, "paused", |e| {
        matches!(e, PlayerEvent::PauseChanged(true))
    });
    let wakeups = wakeups_in_one_second("test-paused");
    assert!(wakeups <= 1, "{wakeups} wakeups while paused");

    backend.seek_absolute(2.0);
    wait_for_event(
        &rx,
        &mut seen,
        "seek while paused",
        |e| matches!(e, PlayerEvent::Seeked(p) if (p - 2.0).abs() < 0.01),
    );
    backend.set_speed(1.5);
    wait_for_event(
        &rx,
        &mut seen,
        "rate",
        |e| matches!(e, PlayerEvent::RateChanged(r) if (r - 1.5).abs() < 0.01),
    );
    backend.play();
    wait_for_event(&rx, &mut seen, "playing", |e| {
        matches!(e, PlayerEvent::PauseChanged(false))
    });
    wait_for_event(
        &rx,
        &mut seen,
        "position advancing",
        |e| matches!(e, PlayerEvent::PositionChanged(p) if *p > 2.2),
    );
}

#[test]
fn ended_event_thread_waits_for_mpv() {
    let temp = tempfile::tempdir().unwrap();
    let input = write_silence(temp.path());
    let (backend, rx) = null_output_backend("test-ended");
    let mut seen = Vec::new();
    backend.load_audio_paused(
        &input,
        TargetSource::Person,
        AudioCacheProfile::Music,
        false,
    );
    wait_for_event(&rx, &mut seen, "playing", |e| {
        matches!(e, PlayerEvent::PositionChanged(_))
    });
    backend.seek_absolute(3.5);
    wait_for_event(&rx, &mut seen, "end of file", |e| {
        matches!(e, PlayerEvent::EndFile(EndReason::Eof))
    });
    let wakeups = wakeups_in_one_second("test-ended");
    assert!(wakeups <= 1, "{wakeups} wakeups at the end of the file");
}

/// Settings lines across the directory; mpv also writes comment-only
/// redirect entries for the parent directories.
fn saved_watch_later_lines(watch_dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(watch_dir)
        .map(|entries| {
            entries
                .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
                .flat_map(|text| {
                    text.lines()
                        .filter(|line| !line.starts_with('#'))
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn close_keeps_position_forgets_finished() {
    let temp = tempfile::tempdir().unwrap();
    let input = write_silence(temp.path());
    let input = input.as_str();
    let watch_dir = temp.path().join("watch_later");
    let open = || {
        let mpv = MpvClient::with_initializer(|init| {
            apply_audio_only_options(init)?;
            init.set_option("ao", "null")?;
            init.set_option("watch-later-directory", watch_dir.to_str().unwrap())?;
            init.set_option("watch-later-options", AUDIO_WATCH_LATER_OPTIONS)
        })
        .unwrap();
        let (tx, rx) = async_channel::unbounded();
        let backend = MpvBackend::finish_mpv_init(mpv, tx, "test-resume", "test resume", true);
        backend.set_save_position(true);
        (backend, rx)
    };
    let saved = || saved_watch_later_lines(&watch_dir);

    let (backend, _rx) = open();
    backend.load_audio_paused(input, TargetSource::Person, AudioCacheProfile::Music, true);
    // Seek once audio output is up; sent earlier, mpv sometimes drops it.
    wait_until("playback started", || !backend.get_current_ao().is_empty());
    backend.seek_absolute(1.5);
    backend.with_mpv(|m| m.set_property("af", "volume=0.5".to_string()).unwrap());
    wait_until("seek", || {
        (backend.get_property_f64("time-pos") - 1.5).abs() < 0.01
    });
    drop(backend);
    assert_eq!(saved(), ["start=1.500000"], "filters stay with the app");

    let (backend, _rx) = open();
    backend.load_audio_paused(input, TargetSource::Person, AudioCacheProfile::Music, true);
    wait_until("resumed", || {
        (backend.get_property_f64("time-pos") - 1.5).abs() < 0.01
    });
    backend.seek_absolute(4.0);
    wait_until("end of file", || backend.get_property_bool("eof-reached"));
    drop(backend);
    assert!(
        saved().is_empty(),
        "a finished file must not resume at its end"
    );
}

/// A lavfi graph whose `ametadata` filter creates `written` once mpv opens it.
fn file_writing_graph(written: &std::path::Path) -> String {
    format!(
        "av://lavfi:sine=d=1,ametadata=mode=print:file={}",
        written.display()
    )
}

#[test]
fn list_targets_never_reach_loadfile() {
    use big_os_kit::url::ListOrigin;
    let temp = tempfile::tempdir().unwrap();
    let (backend, rx) = null_output_backend("test-list-guard");
    for origin in [ListOrigin::Network, ListOrigin::LocalFile] {
        let source = TargetSource::List(origin);
        let target = file_writing_graph(&temp.path().join(format!("{origin:?}")));
        backend.load_file(&target, source);
        backend.load_file_titled(&target, source, "Channel");
        backend.load_file_append(&target, source);
        backend.load_files_append(std::slice::from_ref(&target), source);
        backend.load_files_at(std::slice::from_ref(&target), source, Some(0));
        backend.load_audio_paused(&target, source, AudioCacheProfile::Podcast, false);
        backend.load_file_with_audio_profile(&target, source, AudioCacheProfile::Radio);
    }
    assert_eq!(backend.get_playlist_count(), 0);
    // The same graph from the person loads and writes, so the checks here
    // would see a list entry that got through.
    let control = temp.path().join("person");
    backend.load_file(&file_writing_graph(&control), TargetSource::Person);
    let mut seen = Vec::new();
    wait_for_event(&rx, &mut seen, "file loaded", |e| {
        matches!(e, PlayerEvent::FileLoaded(_))
    });
    wait_until("the person's graph writes", || control.exists());
    let written: Vec<_> = std::fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(written, ["person"]);
}

#[cfg(feature = "video")]
#[test]
fn subtitle_delay_stays_within_limit() {
    let (backend, _rx) = null_output_backend("test-sub-delay");
    for (asked, applied) in [(25.0, 10.0), (-25.0, -10.0), (-2.5, -2.5)] {
        backend.set_sub_delay(asked);
        assert_eq!(
            backend.get_property_f64("sub-delay"),
            applied,
            "asked {asked}"
        );
    }
}

#[test]
fn rejected_custom_option_keeps_the_rest() {
    let options = parse_custom_options("no-such-option=1,cache-secs=7");
    let mpv = MpvClient::with_initializer(|init| {
        apply_custom_options(init, &options);
        Ok(())
    })
    .unwrap();
    assert_eq!(mpv.get_property::<f64>("cache-secs").unwrap(), 7.0);
}
