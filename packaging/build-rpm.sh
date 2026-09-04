#!/usr/bin/env bash
# Build the Playlist RPM locally.
#
# Why this exists: `npx tauri build --bundles rpm` fails on machines without
# the appindicator -devel files (the bundler panics with "Can't detect any
# appindicator library"), and this host also lacks the GTK/ALSA dev packages —
# they come from the Flatpak GNOME SDK instead. This script builds the release
# binary with the SDK toolchain, then packages it with rpmbuild, replicating
# the official Tauri RPM layout and scriptlets exactly.
set -euo pipefail
cd "$(dirname "$0")/.."

SDK=$(ls -d /var/lib/flatpak/runtime/org.gnome.Sdk/x86_64/*/*/files 2>/dev/null | head -1)
if [ -z "$SDK" ]; then
  echo "Flatpak GNOME SDK not found (needed for GTK/ALSA dev files)" >&2
  exit 1
fi
export PKG_CONFIG_PATH="$SDK/lib/x86_64-linux-gnu/pkgconfig"
export LIBRARY_PATH="$SDK/lib/x86_64-linux-gnu"
export RUSTFLAGS="-C link-arg=-lxml2 -C link-arg=-lxslt -C link-arg=-Wl,--allow-shlib-undefined"

npx tauri build --no-bundle

VERSION=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
BUILD=rpmbuild
rm -rf "$BUILD"; mkdir -p "$BUILD"/{BUILD,RPMS,SOURCES,SPECS}
cp packaging/playlist.spec "$BUILD/SPECS/"
sed -i "s/^Version:.*/Version:        $VERSION/" "$BUILD/SPECS/playlist.spec"

PAYLOAD=/tmp/playlist-payload
rm -rf "$PAYLOAD"; mkdir -p "$PAYLOAD/icons"
cp src-tauri/target/release/playlist "$PAYLOAD/"
cp src-tauri/resources/com.playlist.app.desktop src-tauri/resources/com.playlist.app.metainfo.xml "$PAYLOAD/"
for size in 32x32 48x48 64x64 128x128 256x256; do
  cp "src-tauri/icons/${size}.png" "$PAYLOAD/icons/${size}.png"
done
# The source 512px icon is stored as icon.png; normalize its payload name for
# the RPM spec's hicolor installation loop.
cp src-tauri/icons/icon.png "$PAYLOAD/icons/512x512.png"
cp src-tauri/icons/128x128@2x.png src-tauri/icons/com.playlist.app.svg "$PAYLOAD/icons/"
tar -C /tmp -cf "$BUILD/SOURCES/payload.tar" playlist-payload

rpmbuild -bb --define "_topdir $PWD/$BUILD" "$BUILD/SPECS/playlist.spec"
echo "RPM ready: $BUILD/RPMS/x86_64/"
