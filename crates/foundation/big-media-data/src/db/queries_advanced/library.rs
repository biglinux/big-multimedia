use rusqlite::Connection;

use crate::db::models::Track;
use crate::db::queries::{row_to_track, track_columns};

/// Backs the "All Tracks" tree node.
pub fn get_library_track_count(
    conn: &Connection,
    filter_text: &str,
    age_secs: Option<i64>,
) -> crate::Result<i64> {
    let mut sql = "SELECT COUNT(*) FROM tracks WHERE 1=1".to_string();
    let mut param_values: Vec<String> = Vec::new();

    append_text_filter(&mut sql, &mut param_values, filter_text);
    append_age_filter(&mut sql, &mut param_values, age_secs);

    let mut stmt = conn.prepare(&sql)?;
    let params_ref = params_ref(&param_values);
    let n: i64 = stmt.query_row(params_ref.as_slice(), |row| row.get(0))?;
    Ok(n)
}

/// Backs leaf nodes and the detail panel.
pub fn get_library_tracks(
    conn: &Connection,
    parent_filters: &[(&str, &str)],
    filter_text: &str,
    age_secs: Option<i64>,
) -> crate::Result<Vec<Track>> {
    let mut sql = String::from(concat!(
        "SELECT ",
        track_columns!(),
        " FROM tracks WHERE 1=1"
    ));
    let mut param_values: Vec<String> = Vec::new();

    append_parent_filters(&mut sql, &mut param_values, parent_filters)?;
    append_text_filter(&mut sql, &mut param_values, filter_text);
    append_age_filter(&mut sql, &mut param_values, age_secs);
    sql.push_str(" ORDER BY artist, album, disc_number, track_number, title");

    let mut stmt = conn.prepare(&sql)?;
    let params_ref = params_ref(&param_values);
    let tracks = crate::db::decoded_rows(stmt.query_map(params_ref.as_slice(), row_to_track)?)?;
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

    append_parent_filters(&mut sql, &mut param_values, parent_filters)?;
    append_text_filter(&mut sql, &mut param_values, filter_text);
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
    let rows = crate::db::decoded_rows(stmt.query_map(params_ref.as_slice(), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?)?;
    Ok(rows)
}

fn append_parent_filters(
    sql: &mut String,
    param_values: &mut Vec<String>,
    parent_filters: &[(&str, &str)],
) -> crate::Result<()> {
    for (pcol, pval) in parent_filters {
        let pc = validate_column(pcol)?;
        let expr = column_text_expr(pc);
        let idx = param_values.len() + 1;
        sql.push_str(&format!(" AND {expr} = ?{idx}"));
        param_values.push(pval.to_string());
    }
    Ok(())
}

/// SQL `COALESCE(...,'')` expression that always yields TEXT for a validated
/// column. INTEGER columns (`year`/`bitrate`/`sample_rate`) are `CAST` to TEXT
/// so they compare equal to the string params the library tree passes; TEXT
/// columns stay bare to keep their indexes usable. `col` must already be allowlisted by [`validate_column`].
fn column_text_expr(col: &str) -> String {
    match col {
        "year" | "bitrate" | "sample_rate" => format!("COALESCE(CAST({col} AS TEXT), '')"),
        _ => format!("COALESCE({col}, '')"),
    }
}

/// `tracks_fts` always exists: the schema fails to open without FTS5.
fn append_text_filter(sql: &mut String, param_values: &mut Vec<String>, filter_text: &str) {
    if filter_text.trim().is_empty() {
        return;
    }

    let idx = param_values.len() + 1;
    sql.push_str(&format!(
        " AND id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?{idx})"
    ));
    param_values.push(crate::db::fts::fts_match_query(filter_text));
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

/// SQL-injection guard: only these columns may be interpolated unquoted. An
/// unknown name is a caller bug and fails instead of quietly grouping by artist.
pub(super) fn validate_column(col: &str) -> crate::Result<&str> {
    match col {
        "artist" | "album_artist" | "album" | "genre" | "year" | "format" | "bitrate"
        | "sample_rate" => Ok(col),
        _ => Err(crate::Error::invalid_argument(
            "library column",
            format!("{col:?} is not a groupable column"),
        )),
    }
}
