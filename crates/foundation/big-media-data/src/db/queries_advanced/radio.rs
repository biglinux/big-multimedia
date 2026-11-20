use rusqlite::{Connection, params};

use crate::db::models::RadioStation;

pub fn insert_radio_station(
    conn: &Connection,
    name: &str,
    url: &str,
    favicon: &str,
    group_name: &str,
) -> crate::Result<i64> {
    let pos: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM radio_stations",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO radio_stations (name, url, favicon, group_name, position) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![name, url, favicon, group_name, pos],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_all_radio_stations(conn: &Connection) -> crate::Result<Vec<RadioStation>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, url, favicon, group_name FROM radio_stations ORDER BY position, id",
    )?;
    let stations = crate::db::decoded_rows(stmt.query_map([], |row| {
        Ok(RadioStation {
            id: row.get(0)?,
            name: row.get(1)?,
            url: row.get(2)?,
            favicon: row.get(3)?,
            group_name: row.get(4)?,
        })
    })?)?;
    Ok(stations)
}

pub fn delete_radio_station(conn: &Connection, station_id: i64) -> crate::Result<()> {
    conn.execute(
        "DELETE FROM radio_stations WHERE id = ?1",
        params![station_id],
    )?;
    Ok(())
}
