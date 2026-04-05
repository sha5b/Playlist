#!/bin/bash
# GNOME Shell and GNOME Software require the desktop file to be named after
# the app ID (com.playlist.app.desktop). Tauri names it after productName
# (Playlist.desktop), so rename it to the correct name.
if [ -f /usr/share/applications/Playlist.desktop ]; then
  mv -f /usr/share/applications/Playlist.desktop /usr/share/applications/com.playlist.app.desktop
fi
update-desktop-database -q /usr/share/applications 2>/dev/null || true
gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || true
