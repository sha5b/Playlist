# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Playlist exists to liberate users from the streaming dictatorship and give people autonomy over their music data. It is a cross-platform desktop music manager built with **Tauri 2**, **SvelteKit/Svelte 5** frontend, and **Rust** backend. Everything is local — downloads via yt-dlp, library in SQLite, playback with rodio/symphonia. No subscriptions, no lock-in, no algorithmic gatekeeping.

This is not a platform for piracy. The ethos is ownership and direct artist support — buy albums, back creators on Patreon, buy them a coffee, go to shows. The tool gives users control; users should use that freedom responsibly.

## Development Commands

```sh
npm install              # Install frontend dependencies
npx tauri dev            # Launch app in dev mode (Vite + Tauri window)
npx tauri build          # Production build (outputs installer to src-tauri/target/release/bundle/)
npm run check            # Type-check Svelte/TS (svelte-kit sync && svelte-check)
npm run check:watch      # Type-check in watch mode
cargo check --manifest-path src-tauri/Cargo.toml   # Check Rust compilation
cargo clippy --manifest-path src-tauri/Cargo.toml   # Lint Rust code
```

Rust unit tests exist for URL parsing, matching, and the play queue. Run them with `cargo test --manifest-path src-tauri/Cargo.toml --lib`. There is no frontend test runner.

## Architecture

### Frontend → Backend Communication

All frontend-backend communication uses Tauri's `invoke()` IPC. The frontend wrappers live in `src/lib/api/` (downloads.ts, library.ts, manager.ts, player.ts) and call Rust `#[tauri::command]` handlers registered in `src-tauri/src/lib.rs`. When adding a new command: define the Rust handler in `src-tauri/src/commands/`, register it in the `.invoke_handler()` call in `lib.rs`, and add a typed TypeScript wrapper in `src/lib/api/`.

### State Management

Svelte 5 runes (`$state`, `$derived`) are used throughout — stores live in `src/lib/stores/`. The player store (`player.svelte.ts`) is central: it manages playback state, queue, and MediaSession integration. Library stores use a version-based invalidation pattern with a 30-second TTL cache on API calls.

### Backend Data Flow

- **Database**: SQLite with WAL mode and FTS5 full-text search. Schema and migrations in `src-tauri/src/db/migrations.rs` (each migration runs in a transaction). Connection pooling via `db/mod.rs` with a separate connection for downloads. `db/mod.rs` also hosts `escape_like` for safe LIKE searches.
- **Audio engine** (`src-tauri/src/audio/engine.rs`): wraps rodio, manages a sink + queue. Emits events to the frontend via Tauri's event system.
- **Download manager** (`src-tauri/src/download/mod.rs`): worker-thread based queue. Uses yt-dlp for extraction, ffmpeg for transcoding. Auto-installs both binaries on first use (`download/setup.rs`). The pipeline itself lives in `download/pipeline.rs` (fast-path sources, smart search, fallbacks), post-download import/tagging/organization in `download/import.rs`, and search-query building/scoring in `download/search.rs`. Downloads default to the OS music folder (`download::default_download_dir`).
- **Metadata enrichment**: MusicBrainz and Last.fm APIs (`src-tauri/src/metadata/`), driven by the commands in `src-tauri/src/commands/enrichment/` (track/album/artist enrichment, bulk scans, library maintenance).

### UI Components

Uses shadcn-svelte (in `src/lib/components/ui/`) with Tailwind CSS v4. App layout is in `src/lib/components/layout/` (AppShell, Sidebar, NowPlayingBar, Queue). The `+layout.svelte` wraps everything in the AppShell.

### Key Patterns

- Library/player/download command handlers live in `src-tauri/src/commands/`, split by concern (`library/`, `enrichment/`, plus `downloads.rs`, `player.rs`, `manager.rs`, `stats.rs`, `watch.rs`, `devices.rs`, `lastfm.rs`). New commands must also be registered in the `.invoke_handler()` call in `lib.rs`.
- TypeScript interfaces for data models are in `src/lib/types/`.
- The app hides to system tray on window close rather than quitting (configured in `lib.rs`).
- yt-dlp/ffmpeg binaries are resolved from app data directory or PATH (`download/setup.rs`).
- Flatpak sandbox is detected via `is_flatpak()` in `download/setup.rs` (checks `/.flatpak-info`). When running in Flatpak, `pkexec` privilege escalation is skipped and ffmpeg is expected from the runtime or `/app/bin`.

### Packaging / Flatpak

- Flatpak manifest: `com.playlist.app.yml` (GNOME 47 runtime, SDK extensions for Rust and Node.js)
- Desktop entry: `src-tauri/resources/com.playlist.app.desktop`
- AppStream metainfo: `src-tauri/resources/com.playlist.app.metainfo.xml`
- Icons at multiple sizes in `src-tauri/icons/` (32, 48, 64, 128, 256, 512) — installed to hicolor theme paths by the Flatpak build
- The Flatpak bundles yt-dlp as a pre-built binary and builds libayatana-appindicator for system tray support
- Offline dependency manifests (`cargo-sources.json`, `node-sources.json`) must be generated before Flathub submission using `flatpak-builder-tools`

## Prerequisites

- Node.js 20+, npm
- Rust 1.77+ (via rustup)
- Platform-specific Tauri 2 dependencies (see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/))
