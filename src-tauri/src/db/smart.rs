//! Smart playlist rules: JSON rule model + safe (fully parameterized) SQL
//! builder that evaluates rules as a SELECT over the tracks table.
//!
//! Rule JSON shape (mirrored in src/lib/types/index.ts as `SmartRules`):
//! {
//!   "match": "all" | "any",
//!   "rules": [{ "field": "genre", "op": "contains", "value": "rock" }, ...],
//!   "sort": { "field": "play_count", "dir": "desc" },   // optional
//!   "limit": 100                                          // optional
//! }

use rusqlite::types::Value as SqlValue;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::models::{Track, TrackPage};
use super::tracks::{row_to_track, TRACK_COLUMNS};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SmartRule {
    pub field: String,
    pub op: String,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SmartSort {
    pub field: String,
    pub dir: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SmartRules {
    /// "all" (AND) or "any" (OR)
    #[serde(rename = "match", default = "default_match")]
    pub match_mode: String,
    #[serde(default)]
    pub rules: Vec<SmartRule>,
    #[serde(default)]
    pub sort: Option<SmartSort>,
    #[serde(default)]
    pub limit: Option<i64>,
}

fn default_match() -> String {
    "all".to_string()
}

/// Kind of a field — decides which ops/value types are valid.
#[derive(PartialEq, Clone, Copy)]
enum FieldKind {
    Text,
    Number,
    Date,
}

/// Whitelist: rule field name -> (SQL column, kind). Values are NEVER
/// interpolated; only these fixed column names ever reach the SQL string.
fn column_for(field: &str) -> Option<(&'static str, FieldKind)> {
    Some(match field {
        "title" => ("t.title", FieldKind::Text),
        "artist" => ("a.name", FieldKind::Text),
        "album" => ("al.title", FieldKind::Text),
        "genre" => ("t.genre", FieldKind::Text),
        "format" => ("t.format", FieldKind::Text),
        "year" => ("t.year", FieldKind::Number),
        "duration_ms" => ("t.duration_ms", FieldKind::Number),
        "play_count" => ("t.play_count", FieldKind::Number),
        "last_played_at" => ("t.last_played_at", FieldKind::Date),
        // "added" is an alias the UI may use for date_added
        "created_at" | "added" | "date_added" => ("t.date_added", FieldKind::Date),
        _ => return None,
    })
}

pub fn parse_rules(rules_json: &str) -> Result<SmartRules, String> {
    serde_json::from_str::<SmartRules>(rules_json)
        .map_err(|e| format!("Invalid smart playlist rules: {}", e))
}

/// Validate rules by parsing and building the WHERE clause. Used by
/// create/update commands so bad rules are rejected up front.
pub fn validate_rules(rules_json: &str) -> Result<(), String> {
    let rules = parse_rules(rules_json)?;
    build_where(&rules).map(|_| ())
}

fn value_as_string(value: &Option<serde_json::Value>) -> Result<String, String> {
    match value {
        Some(serde_json::Value::String(s)) => Ok(s.clone()),
        Some(serde_json::Value::Number(n)) => Ok(n.to_string()),
        _ => Err("Rule value must be a string or number".to_string()),
    }
}

fn value_as_number(value: &Option<serde_json::Value>) -> Result<f64, String> {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_f64().ok_or_else(|| "Invalid number".to_string()),
        Some(serde_json::Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("Rule value '{}' is not a number", s)),
        _ => Err("Rule value must be a number".to_string()),
    }
}

/// Bind a scalar comparison value for a column: numbers bind as REAL/INTEGER,
/// text binds as TEXT.
fn bind_value(kind: FieldKind, value: &Option<serde_json::Value>) -> Result<SqlValue, String> {
    match kind {
        FieldKind::Number => {
            let n = value_as_number(value)?;
            if n.fract() == 0.0 {
                Ok(SqlValue::Integer(n as i64))
            } else {
                Ok(SqlValue::Real(n))
            }
        }
        FieldKind::Text | FieldKind::Date => Ok(SqlValue::Text(value_as_string(value)?)),
    }
}

/// Build the parameterized WHERE clause for a rule set.
/// Returns (where_sql_without_WHERE_keyword, params). An empty rule list
/// yields "1=1" (matches everything).
pub fn build_where(rules: &SmartRules) -> Result<(String, Vec<SqlValue>), String> {
    let joiner = match rules.match_mode.as_str() {
        "all" => " AND ",
        "any" => " OR ",
        other => return Err(format!("Unknown match mode: {}", other)),
    };

    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();

    for rule in &rules.rules {
        let (col, kind) = column_for(&rule.field)
            .ok_or_else(|| format!("Unknown rule field: {}", rule.field))?;

        match rule.op.as_str() {
            "contains" => {
                if kind != FieldKind::Text {
                    return Err(format!("Op 'contains' requires a text field, got: {}", rule.field));
                }
                params.push(SqlValue::Text(value_as_string(&rule.value)?));
                clauses.push(format!("{col} LIKE '%' || ?{} || '%'", params.len()));
            }
            "equals" => {
                params.push(bind_value(kind, &rule.value)?);
                clauses.push(format!("{col} = ?{}", params.len()));
            }
            "not_equals" => {
                params.push(bind_value(kind, &rule.value)?);
                clauses.push(format!("({col} IS NULL OR {col} <> ?{})", params.len()));
            }
            "gt" => {
                params.push(bind_value(kind, &rule.value)?);
                clauses.push(format!("{col} > ?{}", params.len()));
            }
            "lt" => {
                params.push(bind_value(kind, &rule.value)?);
                clauses.push(format!("{col} < ?{}", params.len()));
            }
            "in_last_days" | "not_in_last_days" => {
                if kind != FieldKind::Date {
                    return Err(format!("Op '{}' requires a date field, got: {}", rule.op, rule.field));
                }
                let days = value_as_number(&rule.value)?;
                if !(0.0..=100_000.0).contains(&days) {
                    return Err("Days value must be between 0 and 100000".to_string());
                }
                // The modifier is a bound TEXT parameter, e.g. "-30 days".
                params.push(SqlValue::Text(format!("-{} days", days as i64)));
                if rule.op == "in_last_days" {
                    clauses.push(format!("{col} >= datetime('now', ?{})", params.len()));
                } else {
                    clauses.push(format!(
                        "({col} IS NULL OR {col} < datetime('now', ?{}))",
                        params.len()
                    ));
                }
            }
            "is_null" => {
                clauses.push(format!("({col} IS NULL OR {col} = '')"));
            }
            other => return Err(format!("Unknown rule op: {}", other)),
        }
    }

    let where_sql = if clauses.is_empty() {
        "1=1".to_string()
    } else {
        clauses.join(joiner)
    };

    Ok((where_sql, params))
}

/// Whitelisted ORDER BY clause. Defaults to date added, newest first.
fn order_clause(sort: &Option<SmartSort>) -> Result<String, String> {
    let Some(sort) = sort else {
        return Ok("t.date_added DESC".to_string());
    };
    let (col, _) = column_for(&sort.field)
        .ok_or_else(|| format!("Unknown sort field: {}", sort.field))?;
    let dir = match sort.dir.as_str() {
        "asc" => "ASC",
        "desc" => "DESC",
        other => return Err(format!("Unknown sort direction: {}", other)),
    };
    Ok(format!("{col} {dir}"))
}

const SMART_FROM: &str = "FROM tracks t
     LEFT JOIN artists a ON t.artist_id = a.id
     LEFT JOIN albums al ON t.album_id = al.id";

/// Count matching tracks (capped at the rule set's own limit, if any).
pub fn count_tracks(conn: &Connection, rules: &SmartRules) -> Result<i64, String> {
    let (where_sql, params) = build_where(rules)?;
    let sql = format!("SELECT COUNT(*) {} WHERE {}", SMART_FROM, where_sql);
    let count: i64 = conn
        .query_row(&sql, rusqlite::params_from_iter(params), |row| row.get(0))
        .map_err(|e| e.to_string())?;
    Ok(match rules.limit {
        Some(l) if l >= 0 => count.min(l),
        _ => count,
    })
}

/// Evaluate a rule set into a page of tracks. `offset`/`page_limit` paginate
/// within the (possibly rule-limited) result set; pass offset=0 and a large
/// page_limit to fetch everything.
pub fn evaluate_page(
    conn: &Connection,
    rules: &SmartRules,
    offset: i64,
    page_limit: i64,
) -> Result<TrackPage, String> {
    let total = count_tracks(conn, rules)?;

    let offset = offset.max(0);
    // Clamp the page to the remainder of the rule-limited result set.
    let effective_limit = page_limit.max(0).min((total - offset).max(0));

    let (where_sql, params) = build_where(rules)?;
    let order = order_clause(&rules.sort)?;
    // LIMIT/OFFSET are Rust-computed i64s — safe to inline.
    let sql = format!(
        "SELECT {} {} WHERE {} ORDER BY {} LIMIT {} OFFSET {}",
        TRACK_COLUMNS, SMART_FROM, where_sql, order, effective_limit, offset
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let tracks = stmt
        .query_map(rusqlite::params_from_iter(params), row_to_track)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(TrackPage { tracks, total })
}

/// Evaluate a rule set into the full track list (respecting the rule limit).
pub fn evaluate_all(conn: &Connection, rules_json: &str) -> Result<Vec<Track>, String> {
    let rules = parse_rules(rules_json)?;
    Ok(evaluate_page(conn, &rules, 0, i64::MAX)?.tracks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> SmartRules {
        parse_rules(json).expect("rules should parse")
    }

    #[test]
    fn builds_parameterized_where_for_valid_ops() {
        let rules = parse(
            r#"{
                "match": "all",
                "rules": [
                    {"field": "genre", "op": "contains", "value": "rock"},
                    {"field": "year", "op": "gt", "value": 1999},
                    {"field": "last_played_at", "op": "in_last_days", "value": 30},
                    {"field": "artist", "op": "is_null"}
                ],
                "sort": {"field": "play_count", "dir": "desc"},
                "limit": 50
            }"#,
        );
        let (where_sql, params) = build_where(&rules).expect("builder should accept valid rules");

        // One bound parameter per value-carrying rule (is_null binds none).
        assert_eq!(params.len(), 3);
        assert!(where_sql.contains("t.genre LIKE '%' || ?1 || '%'"));
        assert!(where_sql.contains("t.year > ?2"));
        assert!(where_sql.contains("t.last_played_at >= datetime('now', ?3)"));
        assert!(where_sql.contains("(a.name IS NULL OR a.name = '')"));
        assert!(where_sql.contains(" AND "));
        // Raw values must never leak into the SQL text.
        assert!(!where_sql.contains("rock"));
        assert!(!where_sql.contains("1999"));

        // "any" joins with OR
        let mut any_rules = rules.clone();
        any_rules.match_mode = "any".to_string();
        let (where_any, _) = build_where(&any_rules).unwrap();
        assert!(where_any.contains(" OR "));
        assert!(!where_any.contains(" AND "));
    }

    #[test]
    fn rejects_unknown_fields_ops_and_bad_values() {
        let bad_field = parse(r#"{"match":"all","rules":[{"field":"password","op":"equals","value":"x"}]}"#);
        assert!(build_where(&bad_field).unwrap_err().contains("Unknown rule field"));

        let bad_op = parse(r#"{"match":"all","rules":[{"field":"title","op":"regex","value":"x"}]}"#);
        assert!(build_where(&bad_op).unwrap_err().contains("Unknown rule op"));

        let bad_match = parse(r#"{"match":"some","rules":[]}"#);
        assert!(build_where(&bad_match).unwrap_err().contains("Unknown match mode"));

        // 'contains' on a numeric field is invalid
        let bad_kind = parse(r#"{"match":"all","rules":[{"field":"year","op":"contains","value":"19"}]}"#);
        assert!(build_where(&bad_kind).is_err());

        // days op needs a numeric value
        let bad_days = parse(r#"{"match":"all","rules":[{"field":"date_added","op":"in_last_days","value":"soon"}]}"#);
        assert!(build_where(&bad_days).is_err());
    }

    #[test]
    fn evaluates_rules_against_a_real_database() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::run(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO artists (id, name) VALUES (1, 'Daft Punk');
             INSERT INTO tracks (id, title, artist_id, genre, year, file_path, play_count)
             VALUES (1, 'One More Time', 1, 'House', 2000, '/a.mp3', 10),
                    (2, 'Aerodynamic', 1, 'House', 2001, '/b.mp3', 2),
                    (3, 'Quiet Song', NULL, 'Ambient', 1995, '/c.mp3', 0);",
        )
        .unwrap();

        let rules = parse(
            r#"{
                "match": "all",
                "rules": [
                    {"field": "genre", "op": "equals", "value": "House"},
                    {"field": "year", "op": "gt", "value": 2000}
                ],
                "sort": {"field": "title", "dir": "asc"}
            }"#,
        );
        let page = evaluate_page(&conn, &rules, 0, 100).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.tracks[0].title, "Aerodynamic");

        // "any" + limit caps the total
        let rules = parse(
            r#"{"match":"any","rules":[
                {"field":"genre","op":"equals","value":"House"},
                {"field":"genre","op":"equals","value":"Ambient"}
            ],"sort":{"field":"play_count","dir":"desc"},"limit":2}"#,
        );
        let page = evaluate_page(&conn, &rules, 0, 100).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.tracks[0].title, "One More Time");
    }
}
