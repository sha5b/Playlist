Name:           playlist
Version:        0.10.1
Release:        1%{?dist}
Summary:        Liberate your music. Own your data. Support artists.
License:        MIT
URL:            https://github.com/sha5b/Playlist
Source0:        payload.tar
Requires:       webkit2gtk4.1 gtk3 alsa-lib libappindicator-gtk3
AutoReq:        yes
BuildArch:      x86_64

%description
A cross-platform desktop music manager. Download tracks via yt-dlp, play
everything locally, sync to devices. No account, no subscription.

%prep
tar -xf %{SOURCE0} -C %{_builddir}

%build

%install
mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/share/applications %{buildroot}/usr/share/metainfo
install -m 0755 %{_builddir}/playlist-payload/playlist %{buildroot}/usr/bin/playlist
install -m 0644 %{_builddir}/playlist-payload/com.playlist.app.desktop %{buildroot}/usr/share/applications/com.playlist.app.desktop
install -m 0644 %{_builddir}/playlist-payload/com.playlist.app.metainfo.xml %{buildroot}/usr/share/metainfo/

for size in 32x32 48x48 64x64 128x128 256x256 512x512; do
  install -D -m 0644 %{_builddir}/playlist-payload/icons/${size}.png %{buildroot}/usr/share/icons/hicolor/${size}/apps/com.playlist.app.png
  install -D -m 0644 %{_builddir}/playlist-payload/icons/${size}.png %{buildroot}/usr/share/icons/hicolor/${size}/apps/playlist.png
done
install -D -m 0644 %{_builddir}/playlist-payload/icons/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256@2/apps/com.playlist.app.png
install -D -m 0644 %{_builddir}/playlist-payload/icons/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256@2/apps/playlist.png
install -D -m 0644 %{_builddir}/playlist-payload/icons/com.playlist.app.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/com.playlist.app.svg

%files
%attr(0755,root,root) /usr/bin/playlist
/usr/share/applications/com.playlist.app.desktop
/usr/share/metainfo/com.playlist.app.metainfo.xml
/usr/share/icons/hicolor/*/apps/com.playlist.app.png
/usr/share/icons/hicolor/*/apps/playlist.png
/usr/share/icons/hicolor/scalable/apps/com.playlist.app.svg

%post
#!/bin/bash
update-desktop-database -q /usr/share/applications 2>/dev/null || true
gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || true

%postun
#!/bin/bash
update-desktop-database -q /usr/share/applications 2>/dev/null || true
gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor 2>/dev/null || true
