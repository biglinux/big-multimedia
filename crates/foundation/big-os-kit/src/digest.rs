//! SHA-256 receipts without converting native file paths to text.
use ring::digest::SHA256;

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}
/// SHA-256 of a bounded caller-owned document. This is not authentication.
pub fn bytes(data: &[u8]) -> String {
    hex(ring::digest::digest(&SHA256, data).as_ref())
}
#[cfg(test)]
mod tests {
    #[test]
    fn standard_sha256_vector() {
        assert_eq!(
            super::bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
