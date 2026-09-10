//! Edit-page rotation math (user rotation combined with horizontal flip).

#[must_use]
pub(super) fn effective_rotation(user_rotation: i32, flip_h: bool) -> i32 {
    big_media_components::video::geometry::effective_rotation(user_rotation, flip_h)
}

#[cfg(test)]
mod tests {
    use super::effective_rotation;

    #[test]
    fn effective_rotation_accounts_for_horizontal_flip() {
        assert_eq!(effective_rotation(90, false), 90);
        assert_eq!(effective_rotation(90, true), 270);
        assert_eq!(effective_rotation(-90, true), 90);
    }
}
