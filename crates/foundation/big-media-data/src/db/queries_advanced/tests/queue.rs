use super::{insert_minimal_track, setup_db};
use crate::db::queries_advanced::{load_queue, save_queue};

#[test]
fn save_and_load_queue_roundtrip() {
    let conn = setup_db();
    insert_minimal_track(&conn, 1, "Song", "Art");

    save_queue(&conn, &[1]).unwrap();
    let tracks = load_queue(&conn).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].id, 1);
}

#[test]
fn save_empty_queue() {
    let conn = setup_db();
    save_queue(&conn, &[]).unwrap();
    let tracks = load_queue(&conn).unwrap();
    assert!(tracks.is_empty());
}
