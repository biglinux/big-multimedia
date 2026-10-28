//! Native container duration parsers (MP4/MOV, Matroska/WebM).
//! Avoids spawning ffprobe for fast duration lookup.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// `None` for unsupported formats or parse failures; the caller falls back to ffprobe.
pub(crate) fn fast_read_duration(path: &str) -> Option<f64> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" | "mov" | "3gp" => mp4_read_duration(path),
        "mkv" | "webm" => mkv_read_duration(path),
        _ => None,
    }
}

// ISO BMFF: MP4, MOV, M4V, 3GP.

struct AtomHeader {
    atom_type: [u8; 4],
    data_offset: u64,
    next_pos: u64,
}

fn read_atom_header(f: &mut File, pos: u64, container_end: u64) -> Option<AtomHeader> {
    if pos.checked_add(8)? > container_end {
        return None;
    }
    f.seek(SeekFrom::Start(pos)).ok()?;
    let mut buf = [0_u8; 8];
    f.read_exact(&mut buf).ok()?;
    let size32 = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let mut atom_type = [0_u8; 4];
    atom_type.copy_from_slice(&buf[4..8]);
    let (real_size, header_bytes) = if size32 == 1 {
        let mut ext = [0_u8; 8];
        f.read_exact(&mut ext).ok()?;
        (u64::from_be_bytes(ext), 16_u64)
    } else if size32 == 0 {
        (container_end.checked_sub(pos)?, 8_u64)
    } else {
        (u64::from(size32), 8_u64)
    };
    // Reject pathological size: < header or overflows past container. Fuzz
    // input `0:0:0:1, FF×12` triggers next_pos overflow without these guards.
    if real_size < header_bytes {
        return None;
    }
    let next_pos = pos.checked_add(real_size)?;
    if next_pos > container_end {
        return None;
    }
    Some(AtomHeader {
        atom_type,
        data_offset: pos.checked_add(header_bytes)?,
        next_pos,
    })
}

fn find_atom(f: &mut File, start: u64, end: u64, target: [u8; 4]) -> Option<AtomHeader> {
    let mut pos = start;
    while let Some(atom) = read_atom_header(f, pos, end) {
        if atom.atom_type == target {
            return Some(atom);
        }
        pos = atom.next_pos;
    }
    None
}

/// v0: 32-bit fields → timescale at +12. v1: 64-bit duration → timescale at +20.
fn parse_mvhd_duration(f: &mut File, mvhd_payload_offset: u64) -> Option<f64> {
    f.seek(SeekFrom::Start(mvhd_payload_offset)).ok()?;
    let mut version_buf = [0_u8; 1];
    f.read_exact(&mut version_buf).ok()?;
    match version_buf[0] {
        0 => read_mvhd_payload(f, mvhd_payload_offset + 12, 4),
        1 => read_mvhd_payload(f, mvhd_payload_offset + 20, 8),
        _ => None,
    }
}

fn read_mvhd_payload(f: &mut File, timescale_offset: u64, duration_bytes: u64) -> Option<f64> {
    f.seek(SeekFrom::Start(timescale_offset)).ok()?;
    let mut ts_buf = [0_u8; 4];
    f.read_exact(&mut ts_buf).ok()?;
    let timescale = f64::from(u32::from_be_bytes(ts_buf));
    let duration = match duration_bytes {
        4 => {
            let mut buf = [0_u8; 4];
            f.read_exact(&mut buf).ok()?;
            f64::from(u32::from_be_bytes(buf))
        }
        8 => {
            let mut buf = [0_u8; 8];
            f.read_exact(&mut buf).ok()?;
            u64::from_be_bytes(buf) as f64
        }
        _ => return None,
    };
    // A fragmented MP4 leaves the movie duration 0 and puts the length in
    // its fragments: unknown here, not zero, so the slower probe finds it.
    if timescale > 0.0 && duration > 0.0 {
        Some(duration / timescale)
    } else {
        None
    }
}

pub(crate) fn mp4_read_duration(path: &str) -> Option<f64> {
    let mut f = File::open(path).ok()?;
    let file_size = f.metadata().ok()?.len();
    let moov = find_atom(&mut f, 0, file_size, *b"moov")?;
    let mvhd = find_atom(&mut f, moov.data_offset, moov.next_pos, *b"mvhd")?;
    parse_mvhd_duration(&mut f, mvhd.data_offset)
}

// EBML: Matroska, WebM.

const EBML_HEADER_ID: u32 = 0x1A45_DFA3;
const SEGMENT_ID: u32 = 0x1853_8067;
const MATROSKA_METADATA_ELEMENT_ID: u32 = 0x1549_A966;
const TIMESTAMP_SCALE_ID: u32 = 0x2A_D7B1;
const DURATION_ID: u32 = 0x4489;
/// EBML spec default: 1e6 ns per timestamp tick.
const DEFAULT_TIMESTAMP_SCALE: u64 = 1_000_000;
/// Real files store Info near the start; cap to 1 MiB against pathological inputs.
const MATROSKA_METADATA_SEARCH_LIMIT_BYTES: u64 = 1_048_576;

fn ebml_read_vint(f: &mut File) -> Option<(u64, u8)> {
    let mut first = [0_u8; 1];
    f.read_exact(&mut first).ok()?;
    let b = first[0];
    if b == 0 {
        return None;
    }
    let len = b.leading_zeros() as u8 + 1;
    if len > 8 {
        return None;
    }
    let mask = if len < 8 { 0xFF_u8 >> len } else { 0_u8 };
    let mut val = u64::from(b & mask);
    for _ in 1..len {
        let mut next = [0_u8; 1];
        f.read_exact(&mut next).ok()?;
        val = (val << 8) | u64::from(next[0]);
    }
    Some((val, len))
}

fn ebml_read_id(f: &mut File) -> Option<(u32, u8)> {
    let mut first = [0_u8; 1];
    f.read_exact(&mut first).ok()?;
    let b = first[0];
    if b == 0 {
        return None;
    }
    let len = b.leading_zeros() as u8 + 1;
    if len > 4 {
        return None;
    }
    let mut val = u32::from(b);
    for _ in 1..len {
        let mut next = [0_u8; 1];
        f.read_exact(&mut next).ok()?;
        val = (val << 8) | u32::from(next[0]);
    }
    Some((val, len))
}

/// On success the cursor sits on the first byte of Segment contents.
fn seek_to_matroska_segment_payload(f: &mut File) -> Option<()> {
    let (header_id, _) = ebml_read_id(f)?;
    if header_id != EBML_HEADER_ID {
        return None;
    }
    let (header_size, _) = ebml_read_vint(f)?;
    f.seek(SeekFrom::Current(header_size as i64)).ok()?;

    let (seg_id, _) = ebml_read_id(f)?;
    if seg_id != SEGMENT_ID {
        return None;
    }
    let (_seg_size, _) = ebml_read_vint(f)?;
    Some(())
}

/// Other sizes → `None`.
fn read_ebml_float(f: &mut File, size: u64) -> Option<f64> {
    match size {
        4 => {
            let mut buf = [0_u8; 4];
            f.read_exact(&mut buf).ok()?;
            Some(f64::from(f32::from_be_bytes(buf)))
        }
        8 => {
            let mut buf = [0_u8; 8];
            f.read_exact(&mut buf).ok()?;
            Some(f64::from_be_bytes(buf))
        }
        _ => None,
    }
}

/// Right-aligned in the destination buffer; size must be 1..=8.
fn read_ebml_uint(f: &mut File, size: u64) -> Option<u64> {
    if size == 0 || size > 8 {
        return None;
    }
    let mut buf = [0_u8; 8];
    f.read_exact(&mut buf[8 - size as usize..]).ok()?;
    Some(u64::from_be_bytes(buf))
}

/// Returns `(duration_ticks, scale_ns)`.
fn parse_matroska_metadata_children(
    f: &mut File,
    matroska_metadata_end: u64,
) -> Option<(f64, u64)> {
    let mut duration: Option<f64> = None;
    let mut timestamp_scale = DEFAULT_TIMESTAMP_SCALE;
    while f.stream_position().ok()? < matroska_metadata_end {
        let (child_id, _) = ebml_read_id(f)?;
        let (child_size, _) = ebml_read_vint(f)?;
        let child_payload_pos = f.stream_position().ok()?;
        match child_id {
            TIMESTAMP_SCALE_ID => {
                if let Some(v) = read_ebml_uint(f, child_size) {
                    timestamp_scale = v;
                }
            }
            DURATION_ID => {
                duration = read_ebml_float(f, child_size);
            }
            _ => {}
        }
        f.seek(SeekFrom::Start(child_payload_pos + child_size))
            .ok()?;
    }
    duration.map(|d| (d, timestamp_scale))
}

fn find_matroska_metadata_and_parse(f: &mut File, limit: u64) -> Option<(f64, u64)> {
    while f.stream_position().ok()? < limit {
        let pos_before = f.stream_position().ok()?;
        let (eid, _) = ebml_read_id(f)?;
        let (esize, _) = ebml_read_vint(f)?;
        let element_payload_pos = f.stream_position().ok()?;

        if eid == MATROSKA_METADATA_ELEMENT_ID {
            return parse_matroska_metadata_children(f, element_payload_pos + esize);
        }
        if esize == 0 || pos_before + esize >= limit {
            return None;
        }
        f.seek(SeekFrom::Start(element_payload_pos + esize)).ok()?;
    }
    None
}

pub(crate) fn mkv_read_duration(path: &str) -> Option<f64> {
    let mut f = File::open(path).ok()?;
    let file_size = f.metadata().ok()?.len();

    seek_to_matroska_segment_payload(&mut f)?;
    let segment_payload_start = f.stream_position().ok()?;
    let search_limit = std::cmp::min(
        segment_payload_start + MATROSKA_METADATA_SEARCH_LIMIT_BYTES,
        file_size,
    );

    let (duration, scale) = find_matroska_metadata_and_parse(&mut f, search_limit)?;
    Some(duration * scale as f64 / 1_000_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, bytes: &[u8]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        (dir, path.to_str().unwrap().to_owned())
    }

    #[test]
    fn mp4_duration_from_mvhd() {
        // moov { mvhd v0: timescale 1000, duration 5000 }
        let mut mvhd = vec![0_u8; 12];
        mvhd.extend_from_slice(&1000_u32.to_be_bytes());
        mvhd.extend_from_slice(&5000_u32.to_be_bytes());
        let mut file = Vec::new();
        file.extend_from_slice(&(16 + mvhd.len() as u32).to_be_bytes());
        file.extend_from_slice(b"moov");
        file.extend_from_slice(&(8 + mvhd.len() as u32).to_be_bytes());
        file.extend_from_slice(b"mvhd");
        file.extend_from_slice(&mvhd);
        let (_dir, path) = write_temp("clip.mp4", &file);
        assert_eq!(fast_read_duration(&path), Some(5.0));
    }

    #[test]
    fn mkv_duration_from_info() {
        let file = [
            &[0x1A, 0x45, 0xDF, 0xA3, 0x80][..],         // EBML header, empty
            &[0x18, 0x53, 0x80, 0x67, 0x91],             // Segment, 17 bytes
            &[0x15, 0x49, 0xA9, 0x66, 0x8C],             // Info, 12 bytes
            &[0x2A, 0xD7, 0xB1, 0x81, 0x01],             // TimestampScale 1 ns
            &[0x44, 0x89, 0x84, 0x4E, 0x6E, 0x6B, 0x28], // Duration 1e9 ticks
        ]
        .concat();
        let (_dir, path) = write_temp("clip.mkv", &file);
        assert_eq!(fast_read_duration(&path), Some(1.0));
    }

    #[test]
    fn truncated_mp4_has_no_duration() {
        let (_dir, path) = write_temp("clip.mp4", b"\0\0\0\x10moov");
        assert!(fast_read_duration(&path).is_none());
    }

    #[test]
    fn unsupported_extension_returns_none() {
        assert!(fast_read_duration("test.txt").is_none());
        assert!(fast_read_duration("noext").is_none());
    }
}
