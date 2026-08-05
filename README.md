<p align="center">
  <img src="src-tauri/icons/icon.png" alt="Playlist" width="128" height="128">
</p>

<h1 align="center">Playlist</h1>

<p align="center">
  <strong>Liberate your music. Own your data.</strong>
</p>

<p align="center">
  <a href="https://github.com/sha5b/Playlist/releases/latest"><img src="https://img.shields.io/github/v/release/sha5b/Playlist?style=flat-square&color=blue" alt="Latest Release"></a>
  <a href="https://github.com/sha5b/Playlist/blob/main/LICENSE"><img src="https://img.shields.io/github/license/sha5b/Playlist?style=flat-square" alt="License"></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square" alt="Platforms">
</p>

<p align="center">
  <img src="docs/screenshot/main.png" alt="Playlist — library view" width="900">
</p>

---

Streaming platforms decide what you hear and lock your library behind a subscription. Playlist starts from a simple idea: **your music collection belongs to you.**

Playlist is a desktop music manager for Windows, macOS, and Linux. Download tracks from most platforms, complete your collection, and play everything locally. Your library lives in a local database on your disk. No account. No subscription.

---

## Features

| Feature | Description |
|---|---|
| **Download** | Get tracks from YouTube, SoundCloud, Bandcamp, and hundreds more sources via yt-dlp |
| **Discover** | Find the tracks that your albums miss, and the albums that your artists miss |
| **Smart playlists** | Build playlists from rules (genre, year, play count, and more). They update themselves |
| **Statistics** | See your listening history: plays per day and your most played tracks, artists, and albums |
| **Last.fm** | Send "now playing" updates and scrobbles. Offline plays queue and submit later |
| **Watched folders** | The app imports new audio files from folders that you select |
| **Device sync** | Copy playlists to USB drives, SD cards, and portable players |
| **Metadata** | MusicBrainz and Last.fm supply album art, genres, and release data. A tag editor writes tags into your files |
| **Playback** | Queue, shuffle, repeat, crossfade, volume normalization, and playback from the system tray |
| **Library** | Playlists, album and artist views, and full-text search (Ctrl+K) |
| **Shortcuts** | Control playback from the keyboard. Press ? for the full list |

---

## Install

Download the installer for your platform from the [Releases](https://github.com/sha5b/Playlist/releases/latest) page.

| Platform | Format |
|----------|--------|
| Windows | `.exe` (NSIS installer) |
| macOS | `.dmg` |
| Linux | `.deb`, `.rpm` |

On the first launch, the app downloads yt-dlp and ffmpeg. On each later launch, the app updates yt-dlp, so downloads continue to work when platforms change.

---

## Build from Source

### Prerequisites

- **Node.js** 20+ and **npm**
- **Rust** 1.77+ (install via [rustup](https://rustup.rs))
- Platform-specific Tauri dependencies — see the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/)

### Development

```sh
npm install          # Install frontend dependencies
npx tauri dev        # Start the app in dev mode (Vite + Tauri window)
```

### Production Build

```sh
npx tauri build      # Writes the installer to src-tauri/target/release/bundle/
```

### Flatpak

The Flatpak manifest is at `com.playlist.app.yml`. Use it to build and to publish to Flathub. The build targets GNOME Platform 48 and bundles yt-dlp as a standalone binary.

<details>
<summary><strong>Flatpak build instructions</strong></summary>

#### Prerequisites

```sh
# Install flatpak-builder
sudo dnf install flatpak-builder    # Fedora
sudo apt install flatpak-builder    # Ubuntu/Debian

# Install the SDK and extensions
flatpak install flathub org.gnome.Sdk//48
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//24.08
flatpak install flathub org.freedesktop.Sdk.Extension.node22//24.08
```

#### Generate offline dependency manifests

Flatpak builds have no network access. Fetch the dependencies first with [flatpak-builder-tools](https://github.com/flatpak/flatpak-builder-tools):

```sh
pip3 install toml aiohttp

# Generate Cargo sources from the lockfile
python3 flatpak-builder-tools/cargo/flatpak-cargo-generator.py \
  src-tauri/Cargo.lock -o cargo-sources.json

# Generate Node.js sources from the lockfile
python3 flatpak-builder-tools/node/flatpak-node-generator.py \
  npm package-lock.json -o node-sources.json
```

#### Build and test locally

```sh
flatpak-builder --force-clean build-dir com.playlist.app.yml
flatpak-builder --user --install --force-clean build-dir com.playlist.app.yml
flatpak run com.playlist.app
```

</details>

---

## Architecture

```
src/                        SvelteKit frontend (Svelte 5 + shadcn-svelte + Tailwind v4)
  routes/                   Pages: home, search, library, stats, manager, settings
  lib/api/                  Typed Tauri invoke() wrappers
  lib/stores/               Svelte 5 rune stores
  lib/components/           UI components
src-tauri/                  Rust backend
  src/db/                   SQLite (rusqlite, WAL mode, FTS5 full-text search)
  src/audio/                Audio playback (rodio + symphonia)
  src/download/             Download engine (yt-dlp + ffmpeg)
  src/metadata/             Metadata enrichment (MusicBrainz, Last.fm)
  src/watch.rs              Watched folders (auto-import)
  src/commands/             Tauri IPC command handlers
```

## Tech Stack

| Layer | Technology |
|-------|------------|
| Framework | Tauri 2 |
| Frontend | SvelteKit + Svelte 5 |
| UI | shadcn-svelte + Tailwind CSS v4 |
| Backend | Rust |
| Database | SQLite (WAL mode, FTS5) |
| Audio | rodio + symphonia |
| Downloads | yt-dlp + ffmpeg |
| Metadata | lofty, MusicBrainz, Last.fm |

---

## License

[MIT](LICENSE)
