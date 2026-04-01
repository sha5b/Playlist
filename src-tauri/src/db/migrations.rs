use rusqlite::Connection;

const MIGRATION_001: &str = "
CREATE TABLE IF NOT EXISTS artists (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    sort_name       TEXT,
    musicbrainz_id  TEXT,
    image_path      TEXT,
    bio             TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_artists_name ON artists(name);

CREATE TABLE IF NOT EXISTS albums (
    id              INTEGER PRIMARY KEY,
    title           TEXT NOT NULL,
    artist_id       INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    album_artist    TEXT,
    year            INTEGER,
    genre           TEXT,
    total_tracks    INTEGER,
    total_discs     INTEGER,
    musicbrainz_id  TEXT,
    cover_art_path  TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_albums_artist ON albums(artist_id);

CREATE TABLE IF NOT EXISTS tracks (
    id              INTEGER PRIMARY KEY,
    title           TEXT NOT NULL,
    artist_id       INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    album_id        INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    album_artist    TEXT,
    duration_ms     INTEGER,
    track_number    INTEGER,
    disc_number     INTEGER DEFAULT 1,
    genre           TEXT,
    year            INTEGER,
    file_path       TEXT NOT NULL UNIQUE,
    file_size       INTEGER,
    format          TEXT,
    bitrate         INTEGER,
    sample_rate     INTEGER,
    channels        INTEGER DEFAULT 2,
    cover_art_path  TEXT,
    lyrics          TEXT,
    isrc            TEXT,
    musicbrainz_id  TEXT,
    acoustid        TEXT,
    source_platform TEXT,
    source_url      TEXT,
    play_count      INTEGER NOT NULL DEFAULT 0,
    last_played_at  TEXT,
    date_added      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist_id);
CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album_id);
CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
CREATE INDEX IF NOT EXISTS idx_tracks_date_added ON tracks(date_added DESC);

CREATE TABLE IF NOT EXISTS playlists (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT,
    cover_art_path  TEXT,
    source_platform TEXT,
    source_url      TEXT,
    source_id       TEXT,
    track_count     INTEGER NOT NULL DEFAULT 0,
    total_duration_ms INTEGER NOT NULL DEFAULT 0,
    is_synced       INTEGER NOT NULL DEFAULT 0,
    last_synced_at  TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS playlist_tracks (
    playlist_id     INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id        INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    added_at        TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (playlist_id, track_id)
);
CREATE INDEX IF NOT EXISTS idx_pt_playlist_pos ON playlist_tracks(playlist_id, position);

CREATE TABLE IF NOT EXISTS downloads (
    id              INTEGER PRIMARY KEY,
    url             TEXT NOT NULL,
    title           TEXT,
    artist          TEXT,
    platform        TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'queued',
    progress        REAL DEFAULT 0,
    error_message   TEXT,
    file_path       TEXT,
    track_id        INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
    playlist_id     INTEGER REFERENCES playlists(id) ON DELETE SET NULL,
    format          TEXT DEFAULT 'opus',
    quality         TEXT DEFAULT 'best',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    started_at      TEXT,
    completed_at    TEXT
);
CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);

CREATE TABLE IF NOT EXISTS enrichments (
    id              INTEGER PRIMARY KEY,
    track_id        INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    source          TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    data_json       TEXT,
    error_message   TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at    TEXT
);
CREATE INDEX IF NOT EXISTS idx_enrichments_track ON enrichments(track_id);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

const MIGRATION_002: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
    title, artist_name, album_title, album_artist, genre,
    content='',
    tokenize='unicode61 remove_diacritics 2'
);
";

const MIGRATION_003: &str = "
CREATE TABLE IF NOT EXISTS monitored_playlist_entries (
    id              INTEGER PRIMARY KEY,
    playlist_id     INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    source_url      TEXT NOT NULL,
    title           TEXT,
    artist          TEXT,
    duration_seconds REAL,
    thumbnail       TEXT,
    status          TEXT NOT NULL DEFAULT 'new',
    download_id     INTEGER REFERENCES downloads(id) ON DELETE SET NULL,
    track_id        INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
    position        INTEGER NOT NULL DEFAULT 0,
    first_seen_at   TEXT NOT NULL DEFAULT (datetime('now')),
    downloaded_at   TEXT,
    UNIQUE(playlist_id, source_url)
);
CREATE INDEX IF NOT EXISTS idx_mpe_playlist ON monitored_playlist_entries(playlist_id);
CREATE INDEX IF NOT EXISTS idx_mpe_status ON monitored_playlist_entries(playlist_id, status);
";

const MIGRATION_004: &str = "
ALTER TABLE tracks ADD COLUMN description TEXT;
ALTER TABLE tracks ADD COLUMN label TEXT;
ALTER TABLE tracks ADD COLUMN release_date TEXT;
ALTER TABLE tracks ADD COLUMN composer TEXT;
ALTER TABLE tracks ADD COLUMN language TEXT;
ALTER TABLE tracks ADD COLUMN metadata_completeness INTEGER NOT NULL DEFAULT 0;

ALTER TABLE albums ADD COLUMN label TEXT;
ALTER TABLE albums ADD COLUMN release_date TEXT;
ALTER TABLE albums ADD COLUMN description TEXT;
ALTER TABLE albums ADD COLUMN album_type TEXT;

ALTER TABLE artists ADD COLUMN country TEXT;
ALTER TABLE artists ADD COLUMN begin_year INTEGER;
ALTER TABLE artists ADD COLUMN artist_type TEXT;
";

const MIGRATIONS: &[&str] = &[MIGRATION_001, MIGRATION_002, MIGRATION_003, MIGRATION_004];

pub fn run(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY)",
        [],
    )?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for (i, migration) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i64;
        if version > current_version {
            conn.execute_batch(migration)?;
            conn.execute("INSERT INTO _migrations (version) VALUES (?1)", [version])?;
            log::info!("Applied migration {}", version);
        }
    }

    Ok(())
}
