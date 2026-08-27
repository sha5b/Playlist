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
    pub total_tracks: Option<u32>,
    pub total_discs: Option<u32>,
}


/// Read tags and extract cover art in a single file read (avoids reading the file twice).
pub fn read_tags_and_cover(path: &Path, cover_output_dir: &Path) -> Result<(TagData, Option<String>), String> {
    let tagged_file = Probe::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?
        .read()
        .map_err(|e| format!("Failed to read tags: {}", e))?;

    // --- Extract tag data ---
    let properties = tagged_file.properties();
    let duration = properties.duration();
    let format = path.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase());

    let mut data = TagData {
        duration_ms: Some(duration.as_millis() as u64),
        bitrate: properties.audio_bitrate(),
        sample_rate: properties.sample_rate(),
        channels: properties.channels(),
        format,
        ..Default::default()
    };

    let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());

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
        data.total_tracks = tag.track_total();
        data.total_discs = tag.disk_total();
        data.has_cover_art = !tag.pictures().is_empty();

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

    if data.title.is_none() {
        data.title = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
    }

    // --- Extract cover art from the same parsed file ---
    let cover_path = extract_cover_from_tagged(&tagged_file, path, cover_output_dir);

    Ok((data, cover_path))
}

/// Extract cover art from an already-parsed TaggedFile (no second file read).
fn extract_cover_from_tagged(tagged_file: &lofty::file::TaggedFile, source_path: &Path, output_dir: &Path) -> Option<String> {
    let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag())?;

    // Prefer the front cover: the first embedded picture is often a back
    // cover, leaflet, or artist photo instead.
    let picture = tag
        .pictures()
        .iter()
        .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
        .or_else(|| tag.pictures().first())?;

    let ext = match picture.mime_type() {
        Some(lofty::picture::MimeType::Png) => "png",
        Some(lofty::picture::MimeType::Jpeg) => "jpg",
        Some(lofty::picture::MimeType::Bmp) => "bmp",
        _ => "jpg",
    };

    std::fs::create_dir_all(output_dir).ok()?;

    let hash = simple_hash(source_path.to_string_lossy().as_ref());
    let cover_filename = format!("{:x}.{}", hash, ext);
    let cover_path = output_dir.join(&cover_filename);

    if !cover_path.exists() {
        std::fs::write(&cover_path, picture.data()).ok()?;
    }

    Some(cover_path.to_string_lossy().to_string())
}


/// Authoritative tag values to write to a downloaded file so it's self-describing
/// offline (correct title/artist/album/track number, not the noisy YouTube title).
#[derive(Default)]
pub struct TagWrite {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
}

/// Write the given tags into the audio file, preserving any existing tag/artwork.
pub fn write_tags(path: &Path, w: &TagWrite) -> Result<(), String> {
    use lofty::config::WriteOptions;
    use lofty::tag::{ItemKey, Tag, TagExt};

    let mut tagged = Probe::open(path)
        .map_err(|e| format!("open failed: {}", e))?
        .read()
        .map_err(|e| format!("read failed: {}", e))?;

    let tag_type = tagged.primary_tag_type();
    if tagged.primary_tag().is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged
        .primary_tag_mut()
        .ok_or_else(|| "no writable tag".to_string())?;

    if let Some(v) = &w.title {
        tag.set_title(v.clone());
    }
    if let Some(v) = &w.artist {
        tag.set_artist(v.clone());
    }
    if let Some(v) = &w.album {
        tag.set_album(v.clone());
    }
    if let Some(v) = &w.genre {
        tag.set_genre(v.clone());
    }
    if let Some(v) = w.track_number {
        tag.set_track(v);
    }
    if let Some(v) = w.disc_number {
        tag.set_disk(v);
    }
    if let Some(v) = w.year {
        tag.insert_text(ItemKey::Year, v.to_string());
    }
    if let Some(v) = &w.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, v.clone());
    }

    tag.save_to_path(path, WriteOptions::default())
        .map_err(|e| format!("save failed: {}", e))
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
