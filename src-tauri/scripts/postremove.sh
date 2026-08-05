#!/bin/bash
# On upgrade this runs AFTER the new package's postinstall has already renamed
# the desktop file — removing it here would delete the new package's launcher
# entry. Only remove on a real uninstall: rpm passes $1=0 on erase (>=1 on
# upgrade), deb passes "remove"/"purge" on uninstall ("upgrade" on upgrade).
case "${1:-0}" in
  0|remove|purge)
    rm -f /usr/share/applications/com.playlist.app.desktop
    ;;
esac
update-desktop-database -q /usr/share/applications 2>/dev/null || true
gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || true
