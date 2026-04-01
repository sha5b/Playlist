# Playlist

A cross-platform desktop music manager built with Tauri 2, SvelteKit, and Svelte 5. Download music from YouTube, SoundCloud, Bandcamp and more, manage your library, track playlists for new releases, and play everything with the built-in audio player.

## Prerequisites

- **Node.js** 20+ and **npm**
- **Rust** 1.77+ (install via [rustup](https://rustup.rs))
- Platform-specific Tauri dependencies — see the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/)

## Getting Started

```sh
# Install frontend dependencies
npm install

# Launch the desktop app in development mode
npx tauri dev
```

This starts the Vite dev server and opens the native Tauri window. On first launch, yt-dlp and ffmpeg are downloaded automatically.

## Building

```sh
# Create a production installer for your platform
npx tauri build
```

Output (`.msi` on Windows, `.dmg` on macOS, `.deb`/`.AppImage` on Linux) is in `src-tauri/target/release/bundle/`.

## Project Structure

```
src/                  # SvelteKit frontend (Svelte 5 + shadcn-svelte + Tailwind v4)
  routes/             # Pages: home, search, library, manager, settings
  lib/api/            # Typed Tauri invoke() wrappers
  lib/stores/         # Svelte 5 rune stores
  lib/components/     # UI components
src-tauri/            # Rust backend
  src/db/             # SQLite database (rusqlite, FTS5 search)
  src/audio/          # Audio playback (rodio + symphonia)
  src/download/       # Download engine (yt-dlp + ffmpeg)
  src/metadata/       # Tag reading (lofty)
  src/commands/       # Tauri IPC command handlers
```

## Tech Stack

| Layer | Technology |
|-------|------------|
| Framework | Tauri 2.x |
| Frontend | SvelteKit + Svelte 5 |
| UI | shadcn-svelte + Tailwind CSS v4 |
| Backend | Rust |
| Database | SQLite (rusqlite, WAL mode, FTS5) |
| Audio | rodio + symphonia |
| Downloads | yt-dlp + ffmpeg (auto-installed) |
| Tags | lofty |
