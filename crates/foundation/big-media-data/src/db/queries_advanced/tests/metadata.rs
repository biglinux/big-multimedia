use super::{insert_minimal_track, setup_db};
use crate::db::queries_advanced::update_track_rating;

#[test]
fn update_track_rating_sets_value() {
    let conn = setup_db();
    insert_minimal_track(&conn, 1, "A", "");

    update_track_rating(&conn, 1, 5).unwrap();
    let rating: i32 = conn
        .query_row("SELECT rating FROM tracks WHERE id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rating, 5);
}
