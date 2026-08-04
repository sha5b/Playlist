# Changelog

All notable changes to Playlist are recorded in this file.

## [Unreleased]

### Device sync (USB drives, SD cards, players)

- Files are safe to unplug after "Sync complete". Transcodes now write to a temporary file first, and every synced file is flushed to the device before the app reports success. Before, an early unplug could corrupt or lose files.
- A track that is in two synced playlists no longer ping-pongs between them. Each playlist now records its own copy reference, and the file is copied once.
- The same USB stick is recognized again after a re-plug on Windows. Identity now uses the volume ID, not the drive letter, so the app no longer re-copies everything.
- macOS no longer lists internal volumes as sync targets.
- SD cards in built-in readers are now detected on Linux.
- Tracks removed from a playlist are now removed from the device and from the device playlist file. Before, they accumulated forever.
- Replaced or re-downloaded tracks now re-sync. A change of the output format also re-syncs.
- Playlists you unlinked from a device stay unlinked. Before, opening the device re-linked all of them.
- A cancelled sync no longer silently blocks the next sync. Errors during sync no longer freeze the sync buttons until a restart.
- The Windows device scan no longer reports an error when no device is connected.

## [0.8.0] - 2026-08-04

This release fixes more than 70 verified bugs across downloads, playback, the database, and the interface. A full audit of the codebase found them. The largest fix: playlist imports no longer stop at ~100 tracks.

### Playlist imports

- **YouTube playlists now import completely.** Two causes were fixed. First, the app now updates yt-dlp on every launch. An outdated yt-dlp stopped silently after the first 100 entries. Second, watch-page URLs (`watch?v=…&list=…`) are rewritten to the full playlist page. The watch-page sidebar only exposes ~100 entries.
- Spotify playlists no longer truncate when the internal API fails partway. The fallback API now always runs when tracks are missing.
- Playlists added before this release are re-canonicalized on the next sync, so they also import completely.
- A playlist that contains the same track twice no longer reports a wrong "new tracks" count.

### Downloads

- Native Deezer downloads work now. The CDN address derivation used a wrong byte encoding, so every request failed. Missing file info no longer crashes the download task.
- A failed download is no longer marked "completed" against an unrelated library track. A library match must now pass title and artist checks.
- The download progress bar moves again. Progress was read from the wrong output stream, so it always showed 0%.
- "Stop all" and single-track cancel now reset the playlist entries. Before, entries showed "downloading" forever.
- A download that fails before it starts is now recorded as "failed". Before, it vanished from all lists and returned as a permanent "queued" ghost row.
- Cancel now also stops downloads in the post-processing stage. Before, they restarted on the next launch.
- Vinyl releases (track numbers like "A1") no longer overwrite each other on import. Each earlier overwrite also deleted the audio file.
- Album downloads keep their album link. The per-track download call dropped it because of an argument-name mismatch.
- Retried tracks can use URLs that a failed attempt had claimed.

### Library and metadata

- Duplicate cleanup no longer deletes files for distinct recordings. The same song on an album and on a compilation are now two tracks, not duplicates.
- Album cleanup no longer merges same-titled albums from different artists.
- Artist matching no longer files an artist under another artist whose name contains theirs ("Muse" under "Museum of Love").
- Search works with quotes and punctuation, and deleted tracks no longer appear in results. The search index can now update and delete entries. The index is rebuilt once on the first launch after the update.
- All MusicBrainz requests now share one rate limit, so the service no longer throttles the app. Only one metadata scan can run at a time.
- Lyrics search verifies the track and artist before it stores lyrics. Before, the wrong song's lyrics could be stored permanently.
- Album data now comes from the original album release, not from an arbitrary compilation.
- Library export no longer skips tracks that share a file name.
- Playlist reorder no longer corrupts positions after a track removal.

### Playback

- Removal of the playing track no longer skips the next track.
- Seek during pause no longer shows a false "playing" state with silent audio.
- Tracks with an unknown duration now advance to the next track. Before, the queue stalled forever.
- A manual audio-device switch now resumes playback at the same position. Before, it stopped playback permanently.
- A playlist that contains the same track twice now plays the track you clicked.
- The "Previously Played" row now plays the track it shows.

### Interface

- Confirmation dialogs close after you confirm. Before, they stayed open.
- The seek bar no longer snaps back while you drag it.
- The Songs page keeps one shuffle order across pages. Before, each page click reshuffled, which duplicated and skipped tracks.
- Times over one hour show correctly ("1:30:00", not "90:00"). A rounding error that produced "3:60" is fixed.
- Dates now show in the correct timezone. On Linux they showed "Invalid Date".
- The active-downloads badge no longer sticks at a wrong count after large queues.
- Playlist "Play" and "Shuffle" queue the full playlist, not only the visible page.
- Single-track playlists no longer appear twice in the manager.
- A fast click between playlists no longer shows one playlist's tracks under another playlist's header.
- The History tab refreshes when you open it. "Clear history" no longer shows an empty page.
- Bulk retry shows one toast, not one per track.
- Playlist removal now asks for confirmation.
- The Artists page shows all artists, not only the first 200.
- Multi-disc albums no longer show wrong "missing track" placeholders.
- The Settings page shows the real application version.
- Many smaller fixes: broken-image icons, progress bars at 99.5% shown as complete, missing keyboard access, wrong plurals, stale page states, and event-listener leaks.

## [0.7.0] - 2026-07

- Accurate track matching, organized download folders, scaling and UI fixes.
- Fix for Spotify playlists that stopped at 100 tracks.

## [0.6.0] - 2026-04-05

- Improved metadata accuracy.
- More resilient audio playback.
- Post-download track verification.

## [0.3.1] - 2025-03-01

- Bug fixes and platform compatibility improvements.
