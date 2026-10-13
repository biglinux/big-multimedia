//! Checked layout of mpv software-rendered BGR0 frames. No GTK or C dependency.

/// Bytes in a packed BGR0 frame. Reject dimensions and products that cannot be
/// represented by Rust slices (including on 32-bit hosts).
pub fn packed_layout(width: i32, height: i32) -> Option<(usize, usize)> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let stride = usize::try_from(width).ok()?.checked_mul(4)?;
    let length = stride.checked_mul(usize::try_from(height).ok()?)?;
    isize::try_from(length).is_ok().then_some((stride, length))
}

/// Pad rows to mpv's recommended SIMD alignment without changing image width.
pub fn aligned_layout(width: i32, height: i32) -> Option<(usize, usize)> {
    let (minimum, _) = packed_layout(width, height)?;
    let stride = minimum.checked_add(63)? & !63;
    let length = stride.checked_mul(usize::try_from(height).ok()?)?;
    isize::try_from(length).is_ok().then_some((stride, length))
}

/// Padded rows are allowed, but never less than four bytes per pixel.
pub fn buffer_fits(width: i32, height: i32, stride: usize, available: usize) -> bool {
    let Some((minimum_stride, _)) = packed_layout(width, height) else {
        return false;
    };
    stride.is_multiple_of(4)
        && stride >= minimum_stride
        && stride
            .checked_mul(height as usize)
            .is_some_and(|length| length <= available && isize::try_from(length).is_ok())
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
        assert!(!buffer_fits(3, 2, 13, 1024));
        assert_eq!(aligned_layout(3, 2), Some((64, 128)));
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

/// Aspect-preserving size ceiling for each of the three offload texture slots.
pub fn bounded_surface(width: i32, height: i32) -> Option<(i32, i32)> {
    const MAX_EDGE: i32 = 4096;
    const MAX_PIXELS: f64 = 3840.0 * 2160.0;
    if width <= 0 || height <= 0 {
        return None;
    }
    let w = f64::from(width);
    let h = f64::from(height);
    let scale = (MAX_PIXELS / (w * h))
        .sqrt()
        .min(f64::from(MAX_EDGE) / w)
        .min(f64::from(MAX_EDGE) / h)
        .min(1.0);
    Some((
        (w * scale).floor().max(1.0) as i32,
        (h * scale).floor().max(1.0) as i32,
    ))
}

#[cfg(test)]
mod bounded_surface_tests {
    use super::*;
    #[test]
    fn bounds_scale_without_stretching_or_overflow() {
        assert_eq!(bounded_surface(1920, 1080), Some((1920, 1080)));
        assert_eq!(bounded_surface(7680, 4320), Some((3840, 2160)));
        assert_eq!(bounded_surface(0, 100), None);
        let (w, h) = bounded_surface(i32::MAX, 1).unwrap();
        assert!(w <= 4096 && h >= 1);
    }
}
