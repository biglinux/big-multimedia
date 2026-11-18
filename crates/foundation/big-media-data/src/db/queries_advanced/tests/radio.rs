use super::setup_db;
use crate::db::queries_advanced::radio::migrate_radio_file;
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

#[test]
fn unparsable_file_stays_unmigrated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("radio_stations.json");
    for content in ["not json", r#"{"name": "A", "url": "https://a.test"}"#] {
        std::fs::write(&path, content).unwrap();
        assert!(migrate_radio_file(&setup_db(), &path).is_err(), "{content}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }
    assert!(!path.with_extension("json.migrated").exists());
}

#[test]
fn failed_insert_imports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("radio_stations.json");
    std::fs::write(
        &path,
        r#"[{"name": "A", "url": "https://a.test"}, {"name": "B", "url": "https://refused.test"}]"#,
    )
    .unwrap();
    let conn = setup_db();
    conn.execute_batch(
        "CREATE TEMP TRIGGER refuse BEFORE INSERT ON radio_stations
         WHEN NEW.url = 'https://refused.test'
         BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    )
    .unwrap();

    assert!(migrate_radio_file(&conn, &path).is_err());
    assert!(get_all_radio_stations(&conn).unwrap().is_empty());
    assert!(path.exists());
}

#[test]
fn migration_imports_then_renames() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("radio_stations.json");
    std::fs::write(
        &path,
        r#"[{"name": "A", "url": "https://a.test", "group": "News"}, {"name": "B", "url": "https://b.test"}]"#,
    )
    .unwrap();
    let conn = setup_db();

    migrate_radio_file(&conn, &path).unwrap();
    let stations = get_all_radio_stations(&conn).unwrap();
    let urls: Vec<_> = stations.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(urls, ["https://a.test", "https://b.test"]);
    assert_eq!(stations[0].group_name, "News");
    assert!(!path.exists());
    assert!(path.with_extension("json.migrated").exists());
}
