use rusqlite::{Connection, params};

use crate::db::models::{SmartPlaylist, Track};
use crate::db::queries::{TRACK_COLUMNS, row_to_track};

pub fn get_most_played(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let sql = format!(
        "SELECT {TRACK_COLUMNS} FROM tracks WHERE play_count > 0 ORDER BY play_count DESC LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

pub fn get_recently_added(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks ORDER BY date_added DESC LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

pub fn get_never_played(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let sql =
        format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE play_count = 0 ORDER BY title LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

/// All tracks sharing a `(LOWER(title), LOWER(artist))` pair with another —
/// both fields must be non-empty.
pub fn get_duplicate_tracks(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let sql = format!(
        "SELECT {TRACK_COLUMNS} FROM tracks
         WHERE LOWER(COALESCE(title,'')) || '|||' || LOWER(COALESCE(artist,''))
               IN (
                   SELECT LOWER(COALESCE(title,'')) || '|||' || LOWER(COALESCE(artist,''))
                   FROM tracks
                   WHERE title IS NOT NULL AND title != ''
                     AND artist IS NOT NULL AND artist != ''
                   GROUP BY LOWER(title), LOWER(artist)
                   HAVING COUNT(*) > 1
               )
         ORDER BY LOWER(COALESCE(artist,'')), LOWER(COALESCE(title,'')), file_path
         LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = crate::db::decoded_rows(stmt.query_map(params![limit], row_to_track)?);
    Ok(tracks)
}

pub fn create_smart_playlist(
    conn: &Connection,
    name: &str,
    rules_json: &str,
    logic: &str,
    sort_field: Option<&str>,
    sort_dir: &str,
    max_tracks: i32,
) -> crate::Result<i64> {
    if serde_json::from_str::<Vec<serde_json::Value>>(rules_json).is_err() {
        // caveman: typed invalid-arg over generic sqlite param-name error
        return Err(crate::Error::invalid_argument(
            "rules_json",
            "not a valid JSON array",
        ));
    }
    conn.execute(
        "INSERT INTO smart_playlists (name, rules_json, logic, sort_field, sort_dir, max_tracks) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![name, rules_json, logic, sort_field, sort_dir, max_tracks],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_smart_playlists(conn: &Connection) -> crate::Result<Vec<SmartPlaylist>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, rules_json, logic, sort_field, sort_dir, max_tracks FROM smart_playlists ORDER BY name",
    )?;
    let items = crate::db::decoded_rows(stmt.query_map([], |row| {
        Ok(SmartPlaylist {
            id: row.get(0)?,
            name: row.get(1)?,
            rules_json: row.get(2)?,
            logic: row.get(3)?,
            sort_field: row.get(4)?,
            sort_dir: row.get(5)?,
            max_tracks: row.get(6)?,
        })
    })?);
    Ok(items)
}

pub fn delete_smart_playlist(conn: &Connection, id: i64) -> crate::Result<()> {
    conn.execute("DELETE FROM smart_playlists WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn execute_smart_playlist_rules(
    conn: &Connection,
    rules_json: &str,
    logic: &str,
    sort_field: Option<&str>,
    sort_dir: &str,
    max_tracks: i32,
) -> crate::Result<Vec<Track>> {
    let rules: Vec<serde_json::Value> = serde_json::from_str(rules_json).unwrap_or_default();
    if rules.is_empty() {
        return Ok(Vec::new());
    }

    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for rule in &rules {
        let field = rule
            .get("field")
            .and_then(|v| v.as_str())
            .unwrap_or("title");
        let op = rule
            .get("operator")
            .and_then(|v| v.as_str())
            .unwrap_or("contains");
        let value = rule.get("value").and_then(|v| v.as_str()).unwrap_or("");

        if !valid_rule_fields().contains(&field) {
            continue;
        }

        let condition = match op {
            "contains" => {
                params.push(Box::new(format!("%{value}%")));
                format!("{field} LIKE ?")
            }
            "not_contains" => {
                params.push(Box::new(format!("%{value}%")));
                format!("{field} NOT LIKE ?")
            }
            "equals" => {
                params.push(Box::new(value.to_string()));
                format!("{field} = ?")
            }
            "not_equals" => {
                params.push(Box::new(value.to_string()));
                format!("{field} != ?")
            }
            "greater" => {
                params.push(Box::new(value.to_string()));
                format!("{field} > ?")
            }
            "less" => {
                params.push(Box::new(value.to_string()));
                format!("{field} < ?")
            }
            "starts_with" => {
                params.push(Box::new(format!("{value}%")));
                format!("{field} LIKE ?")
            }
            "ends_with" => {
                params.push(Box::new(format!("%{value}")));
                format!("{field} LIKE ?")
            }
            "is_empty" => format!("({field} IS NULL OR {field} = '')"),
            "is_not_empty" => format!("({field} IS NOT NULL AND {field} != '')"),
            _ => continue,
        };
        conditions.push(condition);
    }

    if conditions.is_empty() {
        return Ok(Vec::new());
    }

    let joiner = if logic == "OR" { " OR " } else { " AND " };
    let where_clause = conditions.join(joiner);
    let sort_col = resolve_sort_column(sort_field.unwrap_or("title"));
    let dir = if sort_dir == "DESC" { "DESC" } else { "ASC" };
    let limit_clause = if max_tracks > 0 {
        format!(" LIMIT {max_tracks}")
    } else {
        String::new()
    };
    let sql = format!(
        "SELECT {TRACK_COLUMNS} FROM tracks WHERE {where_clause} ORDER BY {sort_col} {dir}{limit_clause}"
    );

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(std::convert::AsRef::as_ref).collect();
    let tracks = crate::db::decoded_rows(stmt.query_map(param_refs.as_slice(), row_to_track)?);
    Ok(tracks)
}

fn valid_rule_fields() -> &'static [&'static str] {
    &[
        "title",
        "artist",
        "album",
        "album_artist",
        "genre",
        "year",
        "play_count",
        "rating",
        "duration_secs",
        "bitrate",
        "sample_rate",
        "format",
        "date_added",
        "last_played",
    ]
}

fn resolve_sort_column(sort: &str) -> &str {
    match sort {
        "title" | "artist" | "album" | "genre" | "year" | "play_count" | "rating"
        | "duration_secs" | "date_added" | "last_played" => sort,
        _ => "title",
    }
}
