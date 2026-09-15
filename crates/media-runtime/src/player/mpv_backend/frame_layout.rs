//! Checked layout of mpv software-rendered BGR0 frames. No GTK or C dependency.

/// Bytes in a packed BGR0 frame. Reject dimensions and products that cannot be
/// represented by Rust slices (including on 32-bit hosts).
pub fn packed_layout(width: i32, height: i32) -> Option<(usize, usize)> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let stride = usize::try_from(width).ok()?.checked_mul(4)?;
    let length = stride.checked_mul(usize::try_from(height).ok()?)?;
    (length <= isize::MAX as usize).then_some((stride, length))
}

/// Padded rows are allowed, but never less than four bytes per pixel.
pub fn buffer_fits(width: i32, height: i32, stride: usize, available: usize) -> bool {
    let Some((minimum_stride, _)) = packed_layout(width, height) else {
        return false;
    };
    stride >= minimum_stride
        && stride.checked_mul(height as usize).is_some_and(|length| {
            length <= available && length <= isize::MAX as usize
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_and_padded_frames() {
        assert_eq!(packed_layout(1920, 1080), Some((7680, 8_294_400)));
        assert!(buffer_fits(3, 2, 16, 32));
        assert!(buffer_fits(3, 2, 12, 24));
    }

    #[test]
    fn rejects_short_rows_and_buffers() {
        assert!(!buffer_fits(3, 2, 8, 1024));
        assert!(!buffer_fits(3, 2, 16, 31));
    }

    #[test]
    fn rejects_invalid_dimensions_and_overflow() {
        assert_eq!(packed_layout(0, 1), None);
        assert_eq!(packed_layout(1, -1), None);
        assert_eq!(packed_layout(i32::MAX, i32::MAX), None);
        assert!(!buffer_fits(1, 2, usize::MAX, usize::MAX));
    }
}
