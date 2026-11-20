//! Unguessable identifiers from the kernel's random source.
use std::io::{self, Read};

/// 128 bits from `/dev/urandom` as 32 lowercase hex digits, for tokens, keyring
/// item ids and staging names. Fails closed: no weaker source stands in.
///
/// # Errors
///
/// When `/dev/urandom` cannot be opened or read.
pub fn random_hex_id() -> io::Result<String> {
    random_hex_id_from(std::fs::File::open("/dev/urandom")?)
}

fn random_hex_id_from(mut source: impl Read) -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    source.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_32_lowercase_hex() {
        assert_eq!(
            random_hex_id_from(io::Cursor::new([0x00, 0x0f, 0xa2, 0xff].repeat(4))).unwrap(),
            "000fa2ff".repeat(4)
        );
        let id = random_hex_id().unwrap();
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')));
        assert_ne!(id, random_hex_id().unwrap());
    }

    #[test]
    fn a_short_source_is_an_error() {
        assert!(random_hex_id_from(io::Cursor::new([0_u8; 15])).is_err());
    }
}
