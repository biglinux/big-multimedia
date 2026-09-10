use super::*;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "big-convertd-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn conversion_job(input: &Path, output_dir: &Path) -> DurableJobSpec {
    DurableJobSpec {
        job_key: "job-1".to_owned(),
        job_class: "video-conversion".to_owned(),
        is_durable: true,
        arguments: serde_json::json!({
            "schema_version": 1,
            "inputs": [input],
            "output_dir": output_dir,
            "profile": default_conversion_profile(),
            "audio_only_mode": false,
            "delete_source": false
        }),
    }
}

#[test]
fn validation_rejects_unknown_argument_fields() {
    let job = DurableJobSpec {
        job_key: "job-1".to_owned(),
        job_class: "video-conversion".to_owned(),
        is_durable: true,
        arguments: serde_json::json!({
            "schema_version": 1,
            "inputs": ["/tmp/input.mp4"],
            "gtk_widget": "forbidden"
        }),
    };

    let error = validate_conversion_job(&job).expect_err("unknown field must fail");
    assert!(error.contains("unknown field"));
}

#[test]
fn validation_rejects_relative_paths() {
    let job = DurableJobSpec {
        job_key: "job-1".to_owned(),
        job_class: "video-conversion".to_owned(),
        is_durable: true,
        arguments: serde_json::json!({
            "schema_version": 1,
            "inputs": ["relative.mp4"]
        }),
    };

    let error = validate_conversion_job(&job).expect_err("relative path must fail");
    assert!(error.contains("absolute"));
}

#[test]
fn validation_accepts_profile_payload_and_canonicalizes_paths() {
    let dir = TestDir::new("validate");
    let input = dir.path.join("input.mp4");
    let output = dir.path.join("out");
    fs::write(&input, b"not a real mp4").expect("write input");
    let job = conversion_job(&input, &output);

    let validated = validate_conversion_job(&job).expect("valid job");

    assert_eq!(
        validated
            .items
            .iter()
            .map(|conversion_input| conversion_input.input.clone())
            .collect::<Vec<_>>(),
        vec![input.canonicalize().expect("canonical input")]
    );
    assert_eq!(
        validated.output_dir,
        output.canonicalize().expect("canonical output")
    );
    assert!(!validated.audio_only_mode);
}

#[test]
fn restart_spec_policy_rejects_delete_source_conversions() {
    let dir = TestDir::new("restart-policy");
    let input = dir.path.join("input.mp4");
    let output = dir.path.join("out");
    fs::write(&input, b"not a real mp4").expect("write input");
    let restartable_job = conversion_job(&input, &output);
    let restartable =
        validate_conversion_job(&restartable_job).expect("restartable conversion validates");
    assert!(restart_spec_for_conversion(&restartable_job, &restartable).is_some());

    let delete_source_job = DurableJobSpec {
        arguments: serde_json::json!({
            "schema_version": 1,
            "inputs": [input],
            "output_dir": output,
            "profile": default_conversion_profile(),
            "delete_source": true
        }),
        ..restartable_job
    };
    let delete_source =
        validate_conversion_job(&delete_source_job).expect("delete-source validates");
    assert!(restart_spec_for_conversion(&delete_source_job, &delete_source).is_none());
}

#[test]
fn validation_preserves_serialized_entry_metadata() {
    let dir = TestDir::new("items");
    let input = dir.path.join("input.mp4");
    let subtitle = dir.path.join("input.srt");
    let output = dir.path.join("out");
    fs::write(&input, b"not a real mp4").expect("write input");
    fs::write(&subtitle, b"1\n00:00:00,000 --> 00:00:01,000\nok").expect("write subtitle");
    let job = DurableJobSpec {
        job_key: "job-1".to_owned(),
        job_class: "video-conversion".to_owned(),
        is_durable: true,
        arguments: serde_json::json!({
            "schema_version": 1,
            "items": [{
                "input": input,
                "segments": [{"start": 1.25, "end": 3.5}],
                "segment_mode": "split",
                "subtitle": {"external-file": subtitle},
                "audio_track": 2,
                "video_track": 1,
                "crop": {"w": 640, "h": 360, "x": 10, "y": 20},
                "video_overrides": {
                    "brightness": 0.1,
                    "contrast": 1.1,
                    "saturation": 1.2,
                    "hue": 0.0,
                    "rotation": 90,
                    "flip_h": true,
                    "flip_v": false
                },
                "input_audio_channels": 2,
                "skip_video_filters": true
            }],
            "output_dir": output,
            "profile": default_conversion_profile()
        }),
    };

    let validated = validate_conversion_job(&job).expect("valid conversion_input job");
    let conversion_input = validated.items.first().expect("validated conversion_input");

    assert_eq!(conversion_input.segments.len(), 1);
    assert_eq!(conversion_input.segments[0].start, Some(1.25));
    assert_eq!(conversion_input.segment_mode, SegmentOutputMode::Split);
    assert_eq!(conversion_input.audio_track, Some(2));
    assert_eq!(conversion_input.video_track, Some(1));
    assert!(conversion_input.crop.is_some());
    assert!(conversion_input
        .video_overrides
        .as_ref()
        .is_some_and(|overrides| overrides.flip_h));
    assert_eq!(conversion_input.input_audio_channels, 2);
    assert!(conversion_input.skip_video_filters);
}

#[test]
fn restart_marks_non_terminal_jobs_interrupted() {
    let dir = TestDir::new("journal");
    let journal = dir.path.join("jobs.json");
    let job = conversion_job(&dir.path.join("missing.mp4"), &dir.path.join("out"));
    let records = vec![
        ConversionJournalRecord {
            snapshot: DurableJobSnapshot {
                job_id: "running".to_owned(),
                job_class: "video-conversion".to_owned(),
                state: DurableJobState::Running,
                progress: Some(JobProgress::new(1, Some(2))),
                status_text: None,
                details: None,
            },
            restart_spec: Some(job.clone()),
        },
        ConversionJournalRecord {
            snapshot: DurableJobSnapshot {
                job_id: "done".to_owned(),
                job_class: "audio-conversion".to_owned(),
                state: DurableJobState::Completed,
                progress: Some(JobProgress::new(1, Some(1))),
                status_text: None,
                details: None,
            },
            restart_spec: None,
        },
    ];
    persist_conversion_journal(&journal, &records).expect("persist journal");

    let loaded = load_conversion_journal(&journal).expect("load journal");

    assert_eq!(
        loaded.get("running").expect("running").snapshot.state,
        DurableJobState::Interrupted
    );
    assert_eq!(
        loaded
            .get("running")
            .expect("running")
            .restart_spec
            .as_ref()
            .map(|spec| spec.job_key.as_str()),
        Some("job-1")
    );
    assert_eq!(
        loaded.get("done").expect("done").snapshot.state,
        DurableJobState::Completed
    );
}

#[test]
fn v1_journal_without_restart_specs_loads_without_quarantine() {
    let dir = TestDir::new("journal-v1");
    let journal = dir.path.join("jobs.json");
    let snapshots = vec![DurableJobSnapshot {
        job_id: "running".to_owned(),
        job_class: "video-conversion".to_owned(),
        state: DurableJobState::Running,
        progress: Some(JobProgress::new(1, Some(2))),
        status_text: None,
        details: None,
    }];
    let encoded = serde_json::json!({
        "version": 1,
        "jobs": snapshots,
    });
    fs::write(&journal, encoded.to_string()).expect("write v1 journal");

    let loaded = load_conversion_journal(&journal).expect("load journal");

    assert_eq!(
        loaded.get("running").expect("running").snapshot.state,
        DurableJobState::Interrupted
    );
    assert!(journal.exists());
    assert!(loaded
        .get("running")
        .expect("running")
        .restart_spec
        .is_none());
}

#[test]
fn recover_returns_interrupted_conversion_snapshot() {
    let dir = TestDir::new("recover");
    let registry = Arc::new(ConversionRegistry {
        jobs: Mutex::new(HashMap::from([(
            "running".to_owned(),
            RuntimeJob {
                snapshot: DurableJobSnapshot {
                    job_id: "running".to_owned(),
                    job_class: "video-conversion".to_owned(),
                    state: DurableJobState::Interrupted,
                    progress: Some(JobProgress::new(1, Some(2))),
                    status_text: Some("interrupted by daemon restart".to_owned()),
                    details: None,
                },
                restart_spec: None,
                queue: None,
            },
        )])),
        journal_path: dir.path.join("jobs.json"),
        is_shutdown_requested: AtomicBool::new(false),
    });
    let (mut daemon_stream, mut client_stream) = UnixStream::pair().expect("socket pair");

    serve_frame(
        &mut daemon_stream,
        &registry,
        MicrodaemonFrame::command(
            MicrodaemonId::BigConvertd,
            MicrodaemonCommand::Recover {
                job_id: "running".to_owned(),
            },
        ),
    )
    .expect("recover command");
    let frame = read_frame(&mut client_stream)
        .expect("read frame")
        .expect("event frame");
    assert!(
        matches!(
            &frame.body,
            MicrodaemonBody::Event {
                event: MicrodaemonEvent::JobSnapshot { .. }
            }
        ),
        "unexpected frame: {frame:?}"
    );
    let MicrodaemonBody::Event {
        event: MicrodaemonEvent::JobSnapshot { snapshot },
    } = frame.body
    else {
        return;
    };

    assert_eq!(snapshot.job_id, "running");
    assert_eq!(snapshot.state, DurableJobState::Interrupted);
    assert_eq!(
        snapshot.status_text.as_deref(),
        Some("interrupted by daemon restart")
    );
}

#[test]
fn restart_rejects_conversion_without_safe_spec() {
    let dir = TestDir::new("restart-reject");
    let registry = Arc::new(ConversionRegistry {
        jobs: Mutex::new(HashMap::from([(
            "delete-source".to_owned(),
            RuntimeJob {
                snapshot: DurableJobSnapshot {
                    job_id: "delete-source".to_owned(),
                    job_class: "video-conversion".to_owned(),
                    state: DurableJobState::Failed,
                    progress: Some(JobProgress::new(10, Some(100))),
                    status_text: Some("failed".to_owned()),
                    details: None,
                },
                restart_spec: None,
                queue: None,
            },
        )])),
        journal_path: dir.path.join("jobs.json"),
        is_shutdown_requested: AtomicBool::new(false),
    });
    let (mut daemon_stream, mut client_stream) = UnixStream::pair().expect("socket pair");

    serve_frame(
        &mut daemon_stream,
        &registry,
        MicrodaemonFrame::command(
            MicrodaemonId::BigConvertd,
            MicrodaemonCommand::Restart {
                job_id: "delete-source".to_owned(),
            },
        ),
    )
    .expect("restart command");
    let frame = read_frame(&mut client_stream)
        .expect("read frame")
        .expect("event frame");
    assert!(
        matches!(
            &frame.body,
            MicrodaemonBody::Event {
                event: MicrodaemonEvent::Error { .. }
            }
        ),
        "unexpected frame: {frame:?}"
    );
    let MicrodaemonBody::Event {
        event:
            MicrodaemonEvent::Error {
                job_id,
                code,
                message,
            },
    } = frame.body
    else {
        return;
    };

    assert_eq!(job_id.as_deref(), Some("delete-source"));
    assert_eq!(code, "conversion-not-restartable");
    assert!(message.contains("no safe restart policy"));
}

#[test]
fn restart_uses_persisted_spec_for_validation() {
    let dir = TestDir::new("restart-spec");
    let missing_input = dir.path.join("missing.mp4");
    let output = dir.path.join("out");
    let restart_spec = conversion_job(&missing_input, &output);
    let registry = Arc::new(ConversionRegistry {
        jobs: Mutex::new(HashMap::from([(
            "job-1".to_owned(),
            RuntimeJob {
                snapshot: DurableJobSnapshot {
                    job_id: "job-1".to_owned(),
                    job_class: "video-conversion".to_owned(),
                    state: DurableJobState::Failed,
                    progress: Some(JobProgress::new(10, Some(100))),
                    status_text: Some("failed".to_owned()),
                    details: None,
                },
                restart_spec: Some(restart_spec),
                queue: None,
            },
        )])),
        journal_path: dir.path.join("jobs.json"),
        is_shutdown_requested: AtomicBool::new(false),
    });
    let (mut daemon_stream, mut client_stream) = UnixStream::pair().expect("socket pair");

    serve_frame(
        &mut daemon_stream,
        &registry,
        MicrodaemonFrame::command(
            MicrodaemonId::BigConvertd,
            MicrodaemonCommand::Restart {
                job_id: "job-1".to_owned(),
            },
        ),
    )
    .expect("restart command");
    let frame = read_frame(&mut client_stream)
        .expect("read frame")
        .expect("event frame");
    assert!(
        matches!(
            &frame.body,
            MicrodaemonBody::Event {
                event: MicrodaemonEvent::Error { .. }
            }
        ),
        "unexpected frame: {frame:?}"
    );
    let MicrodaemonBody::Event {
        event:
            MicrodaemonEvent::Error {
                job_id,
                code,
                message,
            },
    } = frame.body
    else {
        return;
    };

    assert_eq!(job_id.as_deref(), Some("job-1"));
    assert_eq!(code, "invalid-conversion");
    assert!(message.contains("missing.mp4"));
}

#[test]
fn interrupted_jobs_are_retryable_but_running_jobs_are_active() {
    assert!(is_active_conversion_state(DurableJobState::Queued));
    assert!(is_active_conversion_state(DurableJobState::Running));
    assert!(is_active_conversion_state(DurableJobState::Cancelling));
    assert!(!is_active_conversion_state(DurableJobState::Interrupted));
    assert!(!is_terminal(DurableJobState::Interrupted));
}

#[test]
fn retry_build_queue_restores_original_output_paths() {
    let dir = TestDir::new("resume-output");
    let input = dir.path.join("input.mp4");
    let output_dir = dir.path.join("out");
    let original_output = dir.path.join("out").join("original.mp4");
    fs::write(&input, b"not a real mp4").expect("write input");
    fs::create_dir_all(&output_dir).expect("create output");
    let arguments = validate_conversion_job(&conversion_job(&input, &output_dir)).expect("job");
    let resume_outputs = HashMap::from([(0, original_output.clone())]);

    let queue = build_queue(&arguments, &resume_outputs, Some("job-1"));
    let queue_guard = lock_queue(&queue).expect("queue");
    let details = serde_json::from_value::<ConversionJobDetails>(
        conversion_details_value(&queue_guard).expect("details"),
    )
    .expect("decode details");

    assert_eq!(queue_guard.items()[0].output, original_output);
    assert_eq!(details.items[0].output.as_ref(), Some(&original_output));
}

#[test]
fn completed_entry_percent_is_bounded() {
    assert_eq!(completed_entry_percent(0, 4), 0.0);
    assert_eq!(completed_entry_percent(1, 4), 25.0);
    assert_eq!(completed_entry_percent(5, 4), 100.0);
    assert_eq!(completed_entry_percent(1, 0), 100.0);
}

#[test]
fn entry_progress_updates_snapshot_details_and_aggregate_units() {
    let dir = TestDir::new("conversion_input-progress");
    let input = dir.path.join("input.mp4");
    let output = dir.path.join("out");
    fs::write(&input, b"not a real mp4").expect("write input");
    fs::create_dir_all(&output).expect("create output");
    let arguments = validate_conversion_job(&conversion_job(&input, &output)).expect("job");
    let queue = build_queue(&arguments, &HashMap::new(), Some("job-1"));
    let entry_id = lock_queue(&queue).expect("queue").items()[0].id;
    let registry = Arc::new(ConversionRegistry {
        jobs: Mutex::new(HashMap::new()),
        journal_path: dir.path.join("jobs.json"),
        is_shutdown_requested: AtomicBool::new(false),
    });
    let snapshot = DurableJobSnapshot {
        job_id: "job-1".to_owned(),
        job_class: "video-conversion".to_owned(),
        state: DurableJobState::Running,
        progress: Some(JobProgress::new(0, Some(100))),
        status_text: None,
        details: Some(
            conversion_details_value(&lock_queue(&queue).expect("queue")).expect("details"),
        ),
    };
    lock_jobs(&registry.jobs).expect("jobs").insert(
        "job-1".to_owned(),
        RuntimeJob {
            snapshot,
            restart_spec: None,
            queue: Some(queue),
        },
    );

    update_entry_started(&registry, "job-1", entry_id, "ffmpeg -i input".to_owned())
        .expect("started");
    update_entry_progress(
        &registry,
        "job-1",
        entry_id,
        FfmpegProgressSnapshot {
            percent: 42.0,
            ..Default::default()
        },
    )
    .expect("progress");

    let jobs = lock_jobs(&registry.jobs).expect("jobs");
    let snapshot = &jobs.get("job-1").expect("job").snapshot;
    let details = read_conversion_details(snapshot).expect("details");

    assert_eq!(snapshot.progress, Some(JobProgress::new(42, Some(100))));
    assert_eq!(details.items[0].state, ConversionEntryState::Running);
    assert_eq!(details.items[0].command.as_deref(), Some("ffmpeg -i input"));
    assert_eq!(
        details.items[0]
            .progress
            .as_ref()
            .map(|progress| progress.percent),
        Some(42.0)
    );
}
