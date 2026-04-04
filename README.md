# Playlist

**Liberate your music. Own your data. Support artists.**

Streaming platforms decide what you hear, lock your library behind a subscription, and pay artists fractions of a cent. Playlist is built on a simple idea: your music collection belongs to you, not to a corporation.

This is a cross-platform desktop music manager built with Tauri 2, SvelteKit, and Svelte 5. Download tracks from most platforms, discover missing albums and tracks, sync your library across devices, and play everything locally — all offline, all yours.

> **Support artists directly.** Buy their albums. Back them on Patreon. Buy them a coffee. Go to their shows. Give them your attention, not just your streams. This tool gives you autonomy over your data — use that freedom to support the people who make the music you love.

## Features

- **Download from almost anywhere** — YouTube, SoundCloud, Bandcamp, and hundreds more sources via yt-dlp
- **Discover missing music** — Search for missing tracks in albums and find albums you don't have for artists in your library
- **Sync across devices** — Keep your library in sync between your machines
- **Rich metadata** — Automatic enrichment from MusicBrainz and Last.fm with album art, genres, and more
- **Full local playback** — Built-in audio player with queue, shuffle, repeat, and system tray background playback
- **Library management** — Playlists, album views, artist browsing, and full-text search
- **Cross-platform** — Windows, macOS, and Linux (deb, rpm, AppImage, Flatpak)

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

### Flatpak

A Flatpak manifest is provided at `com.playlist.app.yml` for building and publishing to Flathub. The build targets GNOME Platform 48 and bundles yt-dlp as a standalone binary. System tray support is provided via libayatana-appindicator built as a Flatpak module.

#### Prerequisites

```sh
# Install flatpak-builder and pip
sudo dnf install flatpak-builder python3-pip   # Fedora
sudo apt install flatpak-builder python3-pip   # Ubuntu/Debian

# Install the SDK and extensions
flatpak install flathub org.gnome.Sdk//48
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//24.08
flatpak install flathub org.freedesktop.Sdk.Extension.node22//24.08
```

#### Generate offline dependency manifests

Flatpak builds have no network access, so all Rust and Node.js dependencies must be pre-fetched. Use the [flatpak-builder-tools](https://github.com/nicknisi/nicknisi) generators:

```sh
# Clone the generator tools
git clone https://github.com/nicknisi/nicknisi /tmp/flatpak-builder-tools
pip3 install toml aiohttp

# Generate Cargo sources from lockfile
python3 /tmp/flatpak-builder-tools/cargo/flatpak-cargo-generator.py \
  src-tauri/Cargo.lock -o cargo-sources.json

# Generate Node.js sources from lockfile
python3 /tmp/flatpak-builder-tools/node/flatpak-node-generator.py \
  npm package-lock.json -o node-sources.json
```

#### Build and test locally

```sh
# Build the Flatpak
flatpak-builder --force-clean build-dir com.playlist.app.yml

# Test run
flatpak-builder --run build-dir com.playlist.app.yml playlist

# Install locally for full testing
flatpak-builder --user --install --force-clean build-dir com.playlist.app.yml
flatpak run com.playlist.app
```

#### Flathub submission

To submit to Flathub, follow the [Flathub submission guide](https://docs.flathub.org/docs/for-app-authors/submission):

1. Ensure the local Flatpak build works end-to-end
2. Add screenshots to `src-tauri/resources/com.playlist.app.metainfo.xml`
3. Validate metainfo with `flatpak-builder-lint`
4. Fork the [Flathub repository](https://github.com/nicknisi/nicknisi) on GitHub
5. Clone the `new-pr` branch and add the manifest + source files
6. Open a PR against the `new-pr` branch (never `master`)

**Remaining Flathub checklist:**
- [ ] Add app screenshots to metainfo.xml
- [ ] Generate `cargo-sources.json` and `node-sources.json`
- [ ] Evaluate app ID rename to `io.github.sha5b.Playlist` (Flathub requires this for GitHub-hosted projects without a custom domain)
- [ ] Run `flatpak-builder-lint` and fix any issues
- [ ] Test full Flatpak build locally

## Project Structure

```
src/                        # SvelteKit frontend (Svelte 5 + shadcn-svelte + Tailwind v4)
  routes/                   # Pages: home, search, library, manager, settings
  lib/api/                  # Typed Tauri invoke() wrappers
  lib/stores/               # Svelte 5 rune stores
  lib/components/           # UI components
src-tauri/                  # Rust backend
  src/db/                   # SQLite database (rusqlite, FTS5 search)
  src/audio/                # Audio playback (rodio + symphonia)
  src/download/             # Download engine (yt-dlp + ffmpeg)
  src/metadata/             # Tag reading (lofty)
  src/commands/             # Tauri IPC command handlers
  resources/                # Desktop entry and AppStream metainfo for packaging
  icons/                    # App icons (all sizes for hicolor theme)
com.playlist.app.yml        # Flatpak manifest
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
