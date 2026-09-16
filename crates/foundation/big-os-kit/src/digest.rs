//! SHA-256 receipts without converting native file paths to text.
use ring::digest::{Context, SHA256};
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len()*2), |mut text, byte| {
        let _ = write!(text, "{byte:02x}"); text
    })
}
/// SHA-256 of a bounded caller-owned document. This is not authentication.
pub fn bytes(data: &[u8]) -> String { hex(ring::digest::digest(&SHA256, data).as_ref()) }
/// Hash a regular no-follow file with constant memory and cooperative cancellation.
/// The same open file is used throughout; callers still revalidate source identity
/// at publication, since hashing does not lock out independent filesystem writers.
pub fn file(path: &Path, cancelled: &AtomicBool) -> io::Result<String> {
    let mut options = std::fs::OpenOptions::new(); options.read(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK); }
    let mut file = options.open(path)?;
    let before = file.metadata()?;
    if !before.is_file() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "receipt requires a regular file")); }
    let mut hash = Context::new(&SHA256); let mut buffer = [0u8; 64*1024]; let mut size=0u64;
    loop {
        if cancelled.load(Ordering::Acquire) { return Err(io::Error::new(io::ErrorKind::Interrupted, "hash cancelled")); }
        let count = match file.read(&mut buffer) { Err(e) if e.kind()==io::ErrorKind::Interrupted => continue, other => other? };
        if count==0 { break; } size += count as u64; hash.update(&buffer[..count]);
    }
    let after=file.metadata()?;
    if size != before.len() || after.len()!=before.len() || before.modified()?!=after.modified()? {
        return Err(io::Error::other("file changed while its receipt was computed"));
    }
    Ok(hex(hash.finish().as_ref()))
}
#[cfg(test)] mod tests {
    #[test] fn standard_sha256_vector() {
        assert_eq!(super::bytes(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
