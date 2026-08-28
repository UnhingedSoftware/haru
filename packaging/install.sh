#!/bin/sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}
binary=${1:-}

apps=$prefix/share/applications
icons=$prefix/share/icons/hicolor
mkdir -p "$apps" "$icons/scalable/apps"

if [ -n "$binary" ]; then
	mkdir -p "$prefix/bin"
	install -m 755 "$binary" "$prefix/bin/haru"
fi

install -m 644 "$here/haru.desktop" "$apps/haru.desktop"
install -m 644 "$here/haru.svg" "$icons/scalable/apps/haru.svg"

for size in 48 64 128 256; do
	mkdir -p "$icons/${size}x${size}/apps"
	install -m 644 "$here/haru-$size.png" "$icons/${size}x${size}/apps/haru.png"
done

if command -v update-desktop-database >/dev/null 2>&1; then
	update-desktop-database "$apps" || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
	gtk-update-icon-cache -f -t "$icons" || true
fi

printf 'installed haru.desktop into %s\n' "$apps"
