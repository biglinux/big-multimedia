use rusqlite::{Connection, params};

use crate::db::models::RadioStation;

pub fn insert_radio_station(
    conn: &Connection,
    name: &str,
    url: &str,
    favicon: &str,
    group_name: &str,
) -> crate::Result<i64> {
    let pos: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM radio_stations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
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
    })?);
    Ok(stations)
}

pub fn delete_radio_station(conn: &Connection, station_id: i64) -> crate::Result<()> {
    conn.execute(
        "DELETE FROM radio_stations WHERE id = ?1",
        params![station_id],
    )?;
    Ok(())
}

/// One-shot: imports legacy `radio_stations.json` then renames it to
/// `.json.migrated` so re-runs no-op.
pub fn migrate_radio_from_json(conn: &Connection) -> crate::Result<()> {
    let path = big_os_kit::xdg_dirs::config_dir()
        .unwrap_or_default()
        .join("big-audio-player")
        .join("radio_stations.json");
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data)
        && let Some(arr) = val.as_array()
    {
        for item in arr {
            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let favicon = item.get("favicon").and_then(|v| v.as_str()).unwrap_or("");
            let group = item.get("group").and_then(|v| v.as_str()).unwrap_or("");
            if !url.is_empty() {
                let _ = insert_radio_station(conn, name, url, favicon, group);
            }
        }
    }
    let migrated = path.with_extension("json.migrated");
    let _ = std::fs::rename(&path, &migrated);
    Ok(())
}
