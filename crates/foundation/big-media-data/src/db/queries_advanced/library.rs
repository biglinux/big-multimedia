use rusqlite::Connection;

use crate::db::models::Track;
use crate::db::queries::{TRACK_COLUMNS, row_to_track};

/// `(distinct value, track count)` pairs, alphabetic. `column` allowlisted
/// (see `validate_column`); `filter_text` uses FTS if available, else LIKE.
pub fn get_library_groups(
    conn: &Connection,
    column: &str,
    filter_text: &str,
    parent_filters: &[(&str, &str)],
    age_secs: Option<i64>,
) -> crate::Result<Vec<(String, i64)>> {
    let col = validate_column(column);
    let grp_expr = column_text_expr(col);
    let mut sql = format!("SELECT {grp_expr} AS grp, COUNT(*) AS cnt FROM tracks WHERE 1=1");
    let mut param_values: Vec<String> = Vec::new();

    append_parent_filters(&mut sql, &mut param_values, parent_filters);
    append_text_filter(conn, &mut sql, &mut param_values, filter_text);
    append_age_filter(&mut sql, &mut param_values, age_secs);
    sql.push_str(" GROUP BY grp ORDER BY grp COLLATE NOCASE");

    query_string_count_pairs(conn, &sql, &param_values)
}

/// Backs the "All Tracks" tree node.
pub fn get_library_track_count(
    conn: &Connection,
    filter_text: &str,
    age_secs: Option<i64>,
) -> crate::Result<i64> {
    let mut sql = "SELECT COUNT(*) FROM tracks WHERE 1=1".to_string();
    let mut param_values: Vec<String> = Vec::new();

    append_text_filter(conn, &mut sql, &mut param_values, filter_text);
    append_age_filter(&mut sql, &mut param_values, age_secs);

    let mut stmt = conn.prepare(&sql)?;
    let params_ref = params_ref(&param_values);
    // caveman: explicit ? converts rusqlite -> crate Error via #[from]
    let n: i64 = stmt.query_row(params_ref.as_slice(), |row| row.get(0))?;
    Ok(n)
}

/// Backs leaf nodes and the detail panel — same filter shape as
/// `get_library_groups`.
pub fn get_library_tracks(
    conn: &Connection,
    parent_filters: &[(&str, &str)],
    filter_text: &str,
    age_secs: Option<i64>,
) -> crate::Result<Vec<Track>> {
    let mut sql = format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE 1=1");
    let mut param_values: Vec<String> = Vec::new();

    append_parent_filters(&mut sql, &mut param_values, parent_filters);
    append_text_filter(conn, &mut sql, &mut param_values, filter_text);
    append_age_filter(&mut sql, &mut param_values, age_secs);
    sql.push_str(" ORDER BY artist, album, disc_number, track_number, title");

    let mut stmt = conn.prepare(&sql)?;
    let params_ref = params_ref(&param_values);
    let tracks = stmt
        .query_map(params_ref.as_slice(), row_to_track)?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(tracks)
}

/// Albums with >1 distinct artist (compilations).
pub fn get_compilation_albums(
    conn: &Connection,
    filter_text: &str,
    parent_filters: &[(&str, &str)],
    age_secs: Option<i64>,
) -> crate::Result<Vec<(String, i64)>> {
    let mut sql =
        String::from("SELECT COALESCE(album, '') AS alb, COUNT(*) AS cnt FROM tracks WHERE 1=1");
    let mut param_values: Vec<String> = Vec::new();

    append_parent_filters(&mut sql, &mut param_values, parent_filters);
    append_text_filter(conn, &mut sql, &mut param_values, filter_text);
    append_age_filter(&mut sql, &mut param_values, age_secs);
    sql.push_str(
        " GROUP BY alb HAVING COUNT(DISTINCT COALESCE(artist, '')) > 1 ORDER BY alb COLLATE NOCASE",
    );

    query_string_count_pairs(conn, &sql, &param_values)
}

fn query_string_count_pairs(
    conn: &Connection,
    sql: &str,
    param_values: &[String],
) -> crate::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(sql)?;
    let params_ref = params_ref(param_values);
    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(rows)
}

fn append_parent_filters(
    sql: &mut String,
    param_values: &mut Vec<String>,
    parent_filters: &[(&str, &str)],
) {
    for (pcol, pval) in parent_filters {
        let pc = validate_column(pcol);
        let expr = column_text_expr(pc);
        let idx = param_values.len() + 1;
        sql.push_str(&format!(" AND {expr} = ?{idx}"));
        param_values.push(pval.to_string());
    }
}

/// SQL `COALESCE(...,'')` expression that always yields TEXT for a validated
/// column. INTEGER columns (`year`/`bitrate`/`sample_rate`) are `CAST` to TEXT
/// so they compare equal to the string params the library tree passes and so
/// group keys decode as `String`; TEXT columns stay bare to keep their indexes
/// usable. `col` must already be allowlisted by [`validate_column`].
fn column_text_expr(col: &str) -> String {
    match col {
        "year" | "bitrate" | "sample_rate" => format!("COALESCE(CAST({col} AS TEXT), '')"),
        _ => format!("COALESCE({col}, '')"),
    }
}

fn append_text_filter(
    conn: &Connection,
    sql: &mut String,
    param_values: &mut Vec<String>,
    filter_text: &str,
) {
    if filter_text.is_empty() {
        return;
    }

    let idx = param_values.len() + 1;
    if fts_available(conn) {
        sql.push_str(&format!(
            " AND id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?{idx})"
        ));
        let fts_query = crate::db::fts::fts_match_query(filter_text);
        param_values.push(fts_query);
    } else {
        sql.push_str(&format!(
            " AND (title LIKE ?{idx} OR artist LIKE ?{idx} OR album LIKE ?{idx} OR genre LIKE ?{idx})"
        ));
        param_values.push(format!("%{filter_text}%"));
    }
}

fn append_age_filter(sql: &mut String, param_values: &mut Vec<String>, age_secs: Option<i64>) {
    if let Some(secs) = age_secs {
        let idx = param_values.len() + 1;
        sql.push_str(&format!(
            " AND date_added >= (strftime('%s','now') - ?{idx})"
        ));
        param_values.push(secs.to_string());
    }
}

fn params_ref(param_values: &[String]) -> Vec<&dyn rusqlite::types::ToSql> {
    param_values
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect()
}

fn fts_available(conn: &Connection) -> bool {
    conn.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='tracks_fts'")
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false)
}

/// SQL-injection guard: only these columns may be interpolated unquoted.
pub(super) fn validate_column(col: &str) -> &str {
    match col {
        "artist" | "album_artist" | "album" | "genre" | "year" | "format" | "bitrate"
        | "sample_rate" => col,
        _ => "artist",
    }
}
