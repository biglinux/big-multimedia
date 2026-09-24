//! Durable, bounded per-output completion receipts. Existence is never proof.
//! Resume verifies source+plan, a full SHA-256 of the output, and ffprobe again.
//! A Prepared record is durable before no-replace publication; after a crash it
//! can recognize that exact output, without trusting a half-written file.
use super::{
    ConversionQueueEntry,
    validation::{Expectation, validate_output_cancellable},
};
use crate::converter::{ffmpeg::TrimRange, profiles::Profile, subtitle::SubtitleInput};
use crate::error::{Error, Result};
use big_os_kit::{
    digest,
    storage::{self, StagedFile, WriteLock},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
const RECEIPT_VERSION: u32 = 1;
// Version 2 preserves requested filters and uses corrected color conversion.
const PLAN_VERSION: u32 = 2;
const MAX_RECEIPT: usize = 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    plan: String,
    sha256: String,
    bytes: u64,
    published: bool,
}
/// Owns a cooperating-writer lock for the output and its receipt until finished.
pub(super) struct Receipt {
    target: PathBuf,
    sidecar: PathBuf,
    plan: String,
    _lock: WriteLock,
}
pub(super) fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<String> {
    digest::file(path, cancel).map_err(|error| {
        if error.kind() == std::io::ErrorKind::Interrupted {
            Error::Cancelled
        } else {
            error.into()
        }
    })
}
fn invalid(message: &str) -> Error {
    Error::internal(message)
}
fn sidecar(target: &Path) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(target.file_name().unwrap_or_default());
    name.push(".big-media-receipt.json");
    target.with_file_name(name)
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
impl Receipt {
    pub(super) fn open(target: &Path, plan: &str) -> Result<Self> {
        if !valid_hash(plan) {
            return Err(invalid("Invalid export plan fingerprint"));
        }
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let lock = WriteLock::try_acquire(target)?;
        Ok(Self {
            target: target.into(),
            sidecar: sidecar(target),
            plan: plan.into(),
            _lock: lock,
        })
    }
    fn read(&self) -> Result<Option<Record>> {
        let Some(bytes) = storage::read_regular_limited(&self.sidecar, MAX_RECEIPT)? else {
            return Ok(None);
        };
        let record: Record = serde_json::from_slice(&bytes)?;
        if record.version != RECEIPT_VERSION
            || !valid_hash(&record.plan)
            || !valid_hash(&record.sha256)
            || record.bytes == 0
        {
            return Err(invalid(
                "Invalid export receipt. Keep the existing output and choose another name.",
            ));
        }
        Ok(Some(record))
    }
    fn persist(&self, record: &Record) -> Result<()> {
        let bytes = serde_json::to_vec(record)?;
        if bytes.len() > MAX_RECEIPT {
            return Err(invalid("Export receipt exceeds its bound"));
        }
        storage::atomic_write_private_receipt(&self.sidecar, &bytes)?.into_io_result()?;
        Ok(())
    }
    /// A successful result proves exact bytes AND media structure against this
    /// plan. An unknown/preexisting file is never overwritten or called Done.
    pub(super) fn completed(
        &self,
        expected: &Expectation,
        cancel: &AtomicBool,
        resume: bool,
    ) -> Result<bool> {
        let metadata = match std::fs::symlink_metadata(&self.target) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.read()?;
                return Ok(false);
            }
            Err(e) => return Err(e.into()),
        };
        if !resume {
            return Err(invalid(
                "An output already exists; verified resume is disabled",
            ));
        }
        let mut receipt = self.read()?.ok_or_else(|| {
            invalid("An output exists without a completion receipt. Choose a new name.")
        })?;
        if receipt.plan != self.plan || !metadata.is_file() || metadata.len() != receipt.bytes {
            return Err(invalid(
                "The existing output does not match the reviewed export plan",
            ));
        }
        if hash_file(&self.target, cancel)? != receipt.sha256 {
            return Err(invalid(
                "The existing output changed after its export receipt",
            ));
        }
        validate_output_cancellable(&self.target, expected, cancel)?;
        // Recheck after probing before accepting (probe itself uses the pathname).
        if hash_file(&self.target, cancel)? != receipt.sha256 {
            return Err(invalid("Output changed during verification"));
        }
        storage::sync_parent(&self.target)?;
        if !receipt.published {
            receipt.published = true;
            self.persist(&receipt)?;
        }
        Ok(true)
    }
    pub(super) fn publish(&self, stage: &StagedFile, cancel: &AtomicBool) -> Result<()> {
        let record = Record {
            version: RECEIPT_VERSION,
            plan: self.plan.clone(),
            sha256: hash_file(stage.path(), cancel)?,
            bytes: std::fs::metadata(stage.path())?.len(),
            published: false,
        };
        // Journal durable before the public name can exist.
        self.persist(&record)?;
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        stage.publish(&self.target)?.into_io_result()?;
        if hash_file(&self.target, cancel)? != record.sha256 {
            return Err(invalid(
                "Published output changed before receipt confirmation",
            ));
        }
        self.persist(&Record {
            published: true,
            ..record
        })
    }
}
/// Includes operational profile fields and all caller choices, not a queue id
/// or display title. Native path bytes preserve non-UTF-8 names on this platform.
pub(super) fn plan(
    entry: &ConversionQueueEntry,
    profile: &Profile,
    segments: &[TrimRange],
    source_hash: &str,
    audio_only: bool,
    cancel: &AtomicBool,
) -> Result<String> {
    let input = std::fs::canonicalize(&entry.input)?;
    let parent = entry
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let output = std::fs::canonicalize(parent)?.join(
        entry
            .output
            .file_name()
            .ok_or_else(|| invalid("Missing output filename"))?,
    );
    let subtitle = match &entry.subtitle {
        SubtitleInput::ExternalFile(path) => {
            serde_json::json!({"external_path":std::fs::canonicalize(path)?.as_os_str().as_encoded_bytes(),"sha256":hash_file(path,cancel)?})
        }
        other => serde_json::to_value(other)?,
    };
    let value = serde_json::json!({
        "version":PLAN_VERSION,"source_sha256":source_hash,"source_path":input.as_os_str().as_encoded_bytes(),
        "output_path":output.as_os_str().as_encoded_bytes(),"video":&profile.video,"audio":&profile.audio,
        "subtitles":&profile.subtitles,"output":&profile.output,"segments":segments,"mode":entry.segment_mode,
        "subtitle_input":subtitle,"audio_track":entry.audio_track,"video_track":entry.video_track,
        "crop":&entry.crop,"overrides":&entry.video_overrides,"audio_only":audio_only,
        "input_audio_channels":entry.input_audio_channels,
    });
    let bytes = serde_json::to_vec(&value)?;
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("Export plan is too large to fingerprint"));
    }
    Ok(digest::bytes(&bytes))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_version_and_hash_fields_are_strict() {
        assert!(!valid_hash(""));
        assert!(valid_hash(&"a".repeat(64)));
        assert!(
            serde_json::from_str::<Record>(
                r#"{"version":1,"plan":"x","sha256":"y","bytes":1,"published":true,"extra":0}"#
            )
            .is_err()
        );
    }
    #[test]
    fn receipt_is_a_sidecar_not_the_output() {
        assert_eq!(
            sidecar(Path::new("/tmp/movie.mp4")),
            Path::new("/tmp/.movie.mp4.big-media-receipt.json")
        );
    }
    #[test]
    fn prepared_receipt_does_not_create_a_public_output() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("movie.mp4");
        let plan = "a".repeat(64);
        let receipt = Receipt::open(&output, &plan).unwrap();
        receipt
            .persist(&Record {
                version: 1,
                plan: plan.clone(),
                sha256: "b".repeat(64),
                bytes: 12,
                published: false,
            })
            .unwrap();
        let record = receipt.read().unwrap().unwrap();
        assert!(!record.published);
        assert_eq!(record.plan, plan);
        assert!(!output.exists());
    }
    #[test]
    fn malformed_receipt_never_rewrites_existing_output() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("kept.mp4");
        std::fs::write(&output, b"user-owned").unwrap();
        let receipt = Receipt::open(&output, &"a".repeat(64)).unwrap();
        std::fs::write(sidecar(&output), b"{invalid").unwrap();
        assert!(receipt.read().is_err());
        assert_eq!(std::fs::read(output).unwrap(), b"user-owned");
    }
    #[test]
    fn cancelled_fingerprint_is_not_a_completed_result() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("input");
        std::fs::write(&source, b"payload").unwrap();
        assert!(matches!(
            hash_file(&source, &AtomicBool::new(true)),
            Err(Error::Cancelled)
        ));
    }
    #[test]
    fn receipt_decode_rejects_zero_size_and_unknown_versions() {
        let dir = tempfile::tempdir().unwrap();
        let receipt = Receipt::open(&dir.path().join("out"), &"a".repeat(64)).unwrap();
        for (version, bytes) in [(2, 12), (1, 0)] {
            let record = Record {
                version,
                plan: "a".repeat(64),
                sha256: "b".repeat(64),
                bytes,
                published: true,
            };
            std::fs::write(&receipt.sidecar, serde_json::to_vec(&record).unwrap()).unwrap();
            assert!(receipt.read().is_err());
        }
    }
}
