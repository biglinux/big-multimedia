// SPDX-License-Identifier: MIT

//! SubRip (`.srt`) parser.
//!
//! Strict enough to reject malformed timing and indices; lenient about
//! blank-line separators and the UTF-8 BOM. Simple HTML-style markup
//! (`<i>`, `<b>`, `<u>`, `<font>`) is stripped from cue text so
//! callers can render the value directly.

use crate::limits::ParseLimits;
use crate::subtitle::types::{Cue, CueTime, ParseError};

/// Parse a `.srt` byte buffer into a vector of [`Cue`] values.
///
/// The parser is panic-free on hostile input, performs no I/O, and
/// enforces the supplied [`ParseLimits`] for both byte length and cue
/// count. UTF-8 input is decoded directly; otherwise the bytes are
/// re-interpreted as Latin-1 (every byte maps to a valid code point),
/// preserving legacy `.srt` files exported by Windows tooling.
///
/// # Errors
///
/// Returns [`ParseError`] when the byte length exceeds
/// [`ParseLimits::max_bytes`], when the produced cue count exceeds
/// [`ParseLimits::max_records`], or when a cue index or timing line is
/// malformed.
///
/// # Examples
///
/// Typical parse-then-iterate flow:
///
/// ```
/// use big_media_engines::ParseLimits;
/// use big_media_engines::subtitle::parse_srt;
///
/// let src = b"\
/// 1\n00:00:01,000 --> 00:00:02,500\nHello world\n\
/// \n\
/// 2\n00:00:03,000 --> 00:00:04,000\n<i>Goodbye</i>\n";
///
/// let cues = parse_srt(src, ParseLimits::SUBTITLE)?;
/// for cue in &cues {
///     assert!(cue.start <= cue.end);
/// }
/// assert_eq!(cues[0].text, "Hello world");
/// assert_eq!(cues[1].text, "Goodbye"); // markup stripped
/// # Ok::<(), big_media_engines::subtitle::ParseError>(())
/// ```
pub fn parse_srt(bytes: &[u8], limits: ParseLimits) -> Result<Vec<Cue>, ParseError> {
    if bytes.len() > limits.max_bytes {
        return Err(ParseError::InputTooLarge {
            len: bytes.len(),
            limit: limits.max_bytes,
        });
    }
    let text = decode_to_utf8(bytes)?;
    let mut cues = Vec::new();
    let mut iter = text.lines().enumerate().peekable();
    while let Some((mut line_no, mut line)) = iter.next() {
        // Skip blank lines between cues.
        while line.trim().is_empty() {
            match iter.next() {
                Some((n, l)) => {
                    line_no = n;
                    line = l;
                }
                None => return Ok(cues),
            }
        }
        // Cue index.
        let index: u32 = line
            .trim()
            .parse()
            .map_err(|_| ParseError::MalformedIndex {
                line_number: line_no as u32 + 1,
            })?;
        // Timing line.
        let (timing_no, timing) = iter.next().ok_or(ParseError::MalformedTiming {
            line_number: line_no as u32 + 2,
        })?;
        let (start, end) = parse_timing(timing).ok_or(ParseError::MalformedTiming {
            line_number: timing_no as u32 + 1,
        })?;
        // Text lines until blank.
        let mut text_lines: Vec<&str> = Vec::new();
        while let Some(&(_, t)) = iter.peek() {
            if t.trim().is_empty() {
                iter.next();
                break;
            }
            text_lines.push(t);
            iter.next();
        }
        let cue_text = strip_simple_markup(&text_lines.join("\n"));
        cues.push(Cue {
            index,
            start,
            end,
            text: cue_text,
        });
        if cues.len() > limits.max_records {
            return Err(ParseError::TooManyCues {
                count: cues.len(),
                limit: limits.max_records,
            });
        }
    }
    Ok(cues)
}

fn decode_to_utf8(bytes: &[u8]) -> Result<String, ParseError> {
    let stripped = strip_bom(bytes);
    match std::str::from_utf8(stripped) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => {
            // Latin-1 fallback — never fails, all bytes map.
            Ok(stripped.iter().map(|&b| b as char).collect())
        }
    }
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

fn parse_timing(line: &str) -> Option<(CueTime, CueTime)> {
    // Format: "HH:MM:SS,ms --> HH:MM:SS,ms"
    let mut parts = line.split("-->");
    let start = parts.next()?.trim();
    let end = parts.next()?.trim();
    if parts.next().is_some() {
        return None;
    }
    Some((parse_clock(start)?, parse_clock(end)?))
}

fn parse_clock(s: &str) -> Option<CueTime> {
    let (clock, ms) = s.rsplit_once(',').or_else(|| s.rsplit_once('.'))?;
    let mut parts = clock.split(':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let sec: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || m >= 60 || sec >= 60 {
        return None;
    }
    let millis: u64 = ms.parse().ok()?;
    if millis >= 1000 {
        return None;
    }
    Some(CueTime::from_hms_millis(h, m, sec, millis))
}

fn strip_simple_markup(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut iter = input.chars().peekable();
    while let Some(c) = iter.next() {
        if c == '<' {
            for ch in iter.by_ref() {
                if ch == '>' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "1\n00:00:01,000 --> 00:00:03,500\nHello world\n\n2\n00:00:04,000 --> 00:00:05,500\n<i>Goodbye</i>\n";

    #[test]
    fn parse_two_cues() {
        let cues = parse_srt(SAMPLE.as_bytes(), ParseLimits::SUBTITLE).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].index, 1);
        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[1].text, "Goodbye");
    }

    #[test]
    fn timing_milliseconds_are_decoded() {
        let cues = parse_srt(SAMPLE.as_bytes(), ParseLimits::SUBTITLE).unwrap();
        assert_eq!(cues[0].start.0, 1_000);
        assert_eq!(cues[0].end.0, 3_500);
    }

    #[test]
    fn bom_is_stripped() {
        let bom = [0xEF, 0xBB, 0xBF];
        let mut buf = bom.to_vec();
        buf.extend_from_slice(SAMPLE.as_bytes());
        let cues = parse_srt(&buf, ParseLimits::SUBTITLE).unwrap();
        assert_eq!(cues.len(), 2);
    }

    #[test]
    fn too_large_rejected() {
        let limits = ParseLimits {
            max_bytes: 10,
            max_records: 100,
        };
        let err = parse_srt(SAMPLE.as_bytes(), limits).unwrap_err();
        assert!(matches!(err, ParseError::InputTooLarge { .. }));
    }

    #[test]
    fn malformed_index_rejected() {
        let src = "abc\n00:00:01,000 --> 00:00:02,000\nx\n";
        let err = parse_srt(src.as_bytes(), ParseLimits::SUBTITLE).unwrap_err();
        assert!(matches!(err, ParseError::MalformedIndex { .. }));
    }

    #[test]
    fn malformed_timing_rejected() {
        let src = "1\n00-00-01 --> 00-00-02\nx\n";
        let err = parse_srt(src.as_bytes(), ParseLimits::SUBTITLE).unwrap_err();
        assert!(matches!(err, ParseError::MalformedTiming { .. }));
    }

    #[test]
    fn latin1_fallback_does_not_panic() {
        // 0xE9 is "é" in Latin-1 but invalid as a UTF-8 start byte.
        let bytes = b"1\n00:00:01,000 --> 00:00:02,000\nCaf\xE9\n";
        let cues = parse_srt(bytes, ParseLimits::SUBTITLE).unwrap();
        assert_eq!(cues.len(), 1);
        assert!(cues[0].text.contains("Caf"));
    }

    #[test]
    fn html_markup_stripped() {
        let src = "1\n00:00:01,000 --> 00:00:02,000\n<i>italic</i> and <b>bold</b>\n";
        let cues = parse_srt(src.as_bytes(), ParseLimits::SUBTITLE).unwrap();
        assert_eq!(cues[0].text, "italic and bold");
    }

    #[test]
    fn empty_input_returns_empty_vec() {
        let cues = parse_srt(b"", ParseLimits::SUBTITLE).unwrap();
        assert!(cues.is_empty());
    }

    #[test]
    fn input_exactly_at_byte_limit_accepted() {
        // Boundary: len == max_bytes must pass (guard is strictly `>`).
        let src = "1\n00:00:01,000 --> 00:00:02,000\nx\n";
        let exact = ParseLimits {
            max_bytes: src.len(),
            max_records: 100,
        };
        let cues = parse_srt(src.as_bytes(), exact).unwrap();
        assert_eq!(cues.len(), 1);
    }

    #[test]
    fn cue_count_exactly_at_limit_accepted() {
        // Boundary: exactly max_records cues must pass (guard is strictly `>`).
        let limits = ParseLimits {
            max_bytes: 4096,
            max_records: 2,
        };
        let cues = parse_srt(SAMPLE.as_bytes(), limits).unwrap();
        assert_eq!(cues.len(), 2);
    }

    #[test]
    fn malformed_index_reports_first_line_number() {
        // Bad index on line 1 -> line_number == 1 (pins `line_no + 1`).
        let src = "abc\n00:00:01,000 --> 00:00:02,000\nx\n";
        let err = parse_srt(src.as_bytes(), ParseLimits::SUBTITLE).unwrap_err();
        assert_eq!(err, ParseError::MalformedIndex { line_number: 1 });
    }

    #[test]
    fn missing_timing_at_eof_reports_line_number() {
        // Index on line 1 then EOF -> MalformedTiming line_number == 2 (pins `line_no + 2`).
        let err = parse_srt(b"1\n", ParseLimits::SUBTITLE).unwrap_err();
        assert_eq!(err, ParseError::MalformedTiming { line_number: 2 });
    }

    #[test]
    fn malformed_timing_reports_timing_line_number() {
        // Timing on line 2 is malformed -> line_number == 2 (pins `timing_no + 1`).
        let err = parse_srt(b"1\nNOTTIMING\nx\n", ParseLimits::SUBTITLE).unwrap_err();
        assert_eq!(err, ParseError::MalformedTiming { line_number: 2 });
    }

    #[test]
    fn minutes_at_or_above_sixty_rejected() {
        // m >= 60 must be rejected (pins the `|| m >= 60` clock guard).
        let src = "1\n00:60:00,000 --> 00:60:01,000\nx\n";
        let err = parse_srt(src.as_bytes(), ParseLimits::SUBTITLE).unwrap_err();
        assert!(matches!(err, ParseError::MalformedTiming { .. }));
    }

    #[test]
    fn seconds_at_or_above_sixty_rejected() {
        // sec >= 60 must be rejected (pins the `|| sec >= 60` clock guard).
        let src = "1\n00:00:60,000 --> 00:00:61,000\nx\n";
        let err = parse_srt(src.as_bytes(), ParseLimits::SUBTITLE).unwrap_err();
        assert!(matches!(err, ParseError::MalformedTiming { .. }));
    }

    #[test]
    fn cue_time_from_hms_millis_computes_total() {
        // 1h 2m 3s 4ms = 3_723_004 ms. Distinct nonzero parts so each
        // arithmetic mutation (+/-, */ /) changes the result.
        assert_eq!(CueTime::from_hms_millis(1, 2, 3, 4).0, 3_723_004);
    }

    #[test]
    fn parse_error_display_messages() {
        // Pins the Display impl against a `Ok(Default::default())` (empty) mutant.
        assert_eq!(
            ParseError::MalformedTiming { line_number: 7 }.to_string(),
            "malformed timing on line 7"
        );
        assert_eq!(
            ParseError::InputTooLarge { len: 9, limit: 5 }.to_string(),
            "input too large: 9 bytes, limit 5"
        );
    }
}
