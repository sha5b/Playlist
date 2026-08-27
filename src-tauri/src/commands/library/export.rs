//! Export the library to a structured folder tree.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Emitter, State};

use crate::db::DbPool;

// --- Export Library ---

/// Sanitize a string for use as a filename/directory name
fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    let trimmed = sanitized.trim().trim_matches('.');
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(serde::Serialize, Clone)]
pub struct ExportProgress {
    pub current: i64,
    pub total: i64,
    pub track_title: String,
}

#[derive(serde::Serialize)]
pub struct ExportResult {
    pub exported: i64,
    pub skipped: i64,
    pub failed: i64,
    pub destination: String,
}

#[tauri::command]
pub async fn library_export(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    destination: String,
) -> Result<ExportResult, String> {
    let dest = PathBuf::from(&destination);
    if !dest.exists() {
        std::fs::create_dir_all(&dest).map_err(|e| format!("Failed to create destination: {}", e))?;
    }

    // Query all tracks with artist and album names
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, Option<String>, Option<String>, Option<i64>, Option<i64>, String)> = {
        let conn = crate::db::lock(&db)?;
        let mut stmt = conn.prepare(
            "SELECT t.id, t.file_path, ar.name, al.title, t.track_number, t.disc_number, t.title
             FROM tracks t
             LEFT JOIN artists ar ON t.artist_id = ar.id
             LEFT JOIN albums al ON t.album_id = al.id
             ORDER BY ar.name, al.title, t.disc_number, t.track_number"
        ).map_err(|e| e.to_string())?;

        let result: Vec<_> = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
        result
    };

    let total = rows.len() as i64;
    let mut exported: i64 = 0;
    let mut skipped: i64 = 0;
    let mut failed: i64 = 0;

    for (i, (track_id, file_path, artist_name, album_title, track_num, disc_num, title)) in rows.iter().enumerate() {
        // Emit progress
        let _ = app_handle.emit("export-progress", ExportProgress {
            current: i as i64 + 1,
            total,
            track_title: title.clone(),
        });

        let src = Path::new(file_path);
        if !src.exists() {
            skipped += 1;
            continue;
        }

        let ext = src.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp3");

        let artist_dir = sanitize_filename(artist_name.as_deref().unwrap_or("Unknown Artist"));
        let album_dir = sanitize_filename(album_title.as_deref().unwrap_or("Unknown Album"));

        // Build filename: "01 - Title.ext" or "1-01 - Title.ext" for multi-disc.
        // On a name collision (two distinct tracks that share disc/track/title,
        // e.g. duplicate downloads or a retag) disambiguate with the track id
        // instead of silently skipping the second file.
        let track_prefix = match (disc_num, track_num) {
            (Some(d), Some(n)) if *d > 1 => format!("{}-{:02}", d, n),
            (_, Some(n)) => format!("{:02}", n),
            _ => String::new(),
        };
        let safe_title = sanitize_filename(title);
        let mut filename = if track_prefix.is_empty() {
            format!("{} [{}].{}", safe_title, track_id, ext)
        } else {
            format!("{} - {}.{}", track_prefix, safe_title, ext)
        };

        let target_dir = dest.join(&artist_dir).join(&album_dir);
        if let Err(e) = std::fs::create_dir_all(&target_dir) {
            log::warn!("Failed to create dir {:?}: {}", target_dir, e);
            failed += 1;
            continue;
        }

        let mut target_file = target_dir.join(&filename);
        if target_file.exists() {
            let stem = if track_prefix.is_empty() {
                format!("{} [{}]", safe_title, track_id)
            } else {
                format!("{} - {} [{}]", track_prefix, safe_title, track_id)
            };
            filename = format!("{}.{}", stem, ext);
            target_file = target_dir.join(&filename);
            if target_file.exists() {
                skipped += 1;
                continue;
            }
        }

        match std::fs::copy(src, &target_file) {
            Ok(_) => exported += 1,
            Err(e) => {
                log::warn!("Failed to copy {:?} -> {:?}: {}", src, target_file, e);
                failed += 1;
            }
        }
    }

    log::info!("Export complete: {} exported, {} skipped, {} failed to {}", exported, skipped, failed, destination);
    Ok(ExportResult {
        exported,
        skipped,
        failed,
        destination,
    })
}
