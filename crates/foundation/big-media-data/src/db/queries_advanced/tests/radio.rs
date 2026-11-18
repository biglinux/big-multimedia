use super::setup_db;
use crate::db::queries_advanced::{
    delete_radio_station, get_all_radio_stations, insert_radio_station,
};

#[test]
fn insert_and_get_radio_stations() {
    let conn = setup_db();
    insert_radio_station(
        &conn,
        "Test FM",
        "http://stream.example.com",
        "icon.png",
        "Rock",
    )
    .unwrap();

    let stations = get_all_radio_stations(&conn).unwrap();
    assert_eq!(stations.len(), 1);
    assert_eq!(stations[0].name, "Test FM");
    assert_eq!(stations[0].group_name, "Rock");
}

#[test]
fn delete_radio_station_removes_it() {
    let conn = setup_db();
    insert_radio_station(&conn, "X", "http://x", "", "").unwrap();
    let list = get_all_radio_stations(&conn).unwrap();
    assert_eq!(list.len(), 1);

    delete_radio_station(&conn, list[0].id).unwrap();
    assert!(get_all_radio_stations(&conn).unwrap().is_empty());
}
