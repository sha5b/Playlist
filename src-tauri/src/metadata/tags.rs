use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::Accessor;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TagData {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: Option<u64>,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub format: Option<String>,
    pub has_cover_art: bool,
}

pub fn read_tags(path: &Path) -> Result<TagData, String> {
    let tagged_file = Probe::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?
        .read()
        .map_err(|e| format!("Failed to read tags: {}", e))?;

    let properties = tagged_file.properties();
    let duration = properties.duration();

    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    let mut data = TagData {
        duration_ms: Some(duration.as_millis() as u64),
        bitrate: properties.audio_bitrate(),
        sample_rate: properties.sample_rate(),
        channels: properties.channels(),
        format,
        ..Default::default()
    };

    // Try primary tag first, then any tag
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    if let Some(tag) = tag {
        data.title = tag.title().map(|s| s.to_string());
        data.artist = tag.artist().map(|s| s.to_string());
        data.album = tag.album().map(|s| s.to_string());
        data.genre = tag.genre().map(|s| s.to_string());
        data.year = tag.get_string(lofty::tag::ItemKey::Year)
            .and_then(|s| s.parse::<u32>().ok())
            .or_else(|| tag.get_string(lofty::tag::ItemKey::RecordingDate)
                .and_then(|s| s.get(..4))
                .and_then(|s| s.parse::<u32>().ok()));
        data.track_number = tag.track();
        data.disc_number = tag.disk();
        data.has_cover_art = !tag.pictures().is_empty();

        // Try to get album artist from tag items
        // lofty uses different item keys per format
        for item in tag.items() {
            let key_str = format!("{:?}", item.key());
            if key_str.contains("AlbumArtist") || key_str.contains("ALBUMARTIST") {
                if let lofty::tag::ItemValue::Text(val) = item.value() {
                    data.album_artist = Some(val.clone());
                    break;
                }
            }
        }
    }

    // Fallback: use filename as title if no title tag
    if data.title.is_none() {
        data.title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
    }

    Ok(data)
}

pub fn extract_cover_art(path: &Path, output_dir: &Path) -> Result<Option<String>, String> {
    let tagged_file = Probe::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?
        .read()
        .map_err(|e| format!("Failed to read tags: {}", e))?;

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let tag = match tag {
        Some(t) => t,
        None => return Ok(None),
    };

    let picture = match tag.pictures().first() {
        Some(p) => p,
        None => return Ok(None),
    };

    let ext = match picture.mime_type() {
        Some(lofty::picture::MimeType::Png) => "png",
        Some(lofty::picture::MimeType::Jpeg) => "jpg",
        Some(lofty::picture::MimeType::Bmp) => "bmp",
        _ => "jpg",
    };

    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

    // Use a hash of the file path as the cover art filename
    let hash = simple_hash(path.to_string_lossy().as_ref());
    let cover_filename = format!("{:x}.{}", hash, ext);
    let cover_path = output_dir.join(&cover_filename);

    if !cover_path.exists() {
        std::fs::write(&cover_path, picture.data()).map_err(|e| e.to_string())?;
    }

    Ok(Some(cover_path.to_string_lossy().to_string()))
}

fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wma", "aiff", "ape", "wv",
];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}
