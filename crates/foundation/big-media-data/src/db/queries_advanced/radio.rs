use std::path::Path;

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

/// One-shot: imports legacy `radio_stations.json` in one transaction, then
/// renames it to `.json.migrated` so re-runs no-op. On any error the file stays
/// for the next start.
pub fn migrate_radio_from_json(conn: &Connection) -> crate::Result<()> {
    let Some(config) = big_os_kit::xdg_dirs::config_dir() else {
        return Ok(());
    };
    migrate_radio_file(
        conn,
        &config.join("big-audio-player").join("radio_stations.json"),
    )
}

pub(super) fn migrate_radio_file(conn: &Connection, path: &Path) -> crate::Result<()> {
    let Some(data) = big_os_kit::storage::read_regular_limited(path, 4 * 1024 * 1024)? else {
        return Ok(());
    };
    let serde_json::Value::Array(stations) = serde_json::from_slice(&data)? else {
        return Err(crate::Error::InvalidArgument {
            what: "radio_stations.json",
            why: "expected a list of stations".to_owned(),
        });
    };
    let transaction = conn.unchecked_transaction()?;
    for station in &stations {
        let field = |key| station.get(key).and_then(|v| v.as_str()).unwrap_or("");
        if !field("url").is_empty() {
            insert_radio_station(
                &transaction,
                field("name"),
                field("url"),
                field("favicon"),
                field("group"),
            )?;
        }
    }
    transaction.commit()?;
    std::fs::rename(path, path.with_extension("json.migrated"))?;
    Ok(())
}
