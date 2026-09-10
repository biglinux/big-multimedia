//! Small video geometry helpers shared by players and editors.

/// Convert user rotation plus horizontal flip into the mpv/display rotation.
#[must_use]
pub fn effective_rotation(user_rotation: i32, horizontal_flip: bool) -> i32 {
    let extra = if horizontal_flip { 180 } else { 0 };
    (user_rotation + extra).rem_euclid(360)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_flip_adds_half_turn() {
        assert_eq!(effective_rotation(90, false), 90);
        assert_eq!(effective_rotation(90, true), 270);
        assert_eq!(effective_rotation(-90, true), 90);
    }
}
