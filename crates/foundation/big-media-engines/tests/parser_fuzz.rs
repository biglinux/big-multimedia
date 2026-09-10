// Random-byte fuzz tests for the public parsers. Gated by `fuzz`.
#![cfg(feature = "fuzz")]

//! Invariants:
//! - parsers never panic on arbitrary bytes
//! - parsers never exceed `ParseLimits::max_records`
//! - parsers always return within bounded time (proptest will timeout)

use std::path::Path;

use big_media_engines::ParseLimits;
use big_media_engines::playlist::parse_m3u;
use big_media_engines::subtitle::parse_srt;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn parse_m3u_does_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..16_384)) {
        // tight cap forces the cap branch on adversarial inputs
        let limits = ParseLimits { max_bytes: 64 * 1024, max_records: 1024 };
        let playlist_result = parse_m3u(&bytes, Path::new("/tmp"), limits);
        if let Ok(parsed_playlist) = playlist_result {
            prop_assert!(parsed_playlist.entries.len() <= limits.max_records);
        }
    }

    #[test]
    fn parse_srt_does_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..16_384)) {
        let limits = ParseLimits { max_bytes: 64 * 1024, max_records: 1024 };
        let subtitle_result = parse_srt(&bytes, limits);
        if let Ok(parsed_cues) = subtitle_result {
            prop_assert!(parsed_cues.len() <= limits.max_records);
        }
    }

    // Edge — input larger than limit must reject quickly.
    #[test]
    fn parse_m3u_oversize_rejected(extra in 1usize..256) {
        let limits = ParseLimits { max_bytes: 128, max_records: 16 };
        let bytes = vec![b'#'; limits.max_bytes + extra];
        let playlist_result = parse_m3u(&bytes, Path::new("/tmp"), limits);
        prop_assert!(playlist_result.is_err());
    }

    #[test]
    fn parse_srt_oversize_rejected(extra in 1usize..256) {
        let limits = ParseLimits { max_bytes: 128, max_records: 16 };
        let bytes = vec![b'1'; limits.max_bytes + extra];
        let subtitle_result = parse_srt(&bytes, limits);
        prop_assert!(subtitle_result.is_err());
    }
}
