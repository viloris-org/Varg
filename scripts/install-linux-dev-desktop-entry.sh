#!/usr/bin/env bash
set -euo pipefail

case "$(uname -s)" in
  Linux) ;;
  *) exit 0 ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
desktop_source="$repo_root/editor/src-tauri/linux/com.varg.editor.desktop"
icon_source="$repo_root/editor/src-tauri/icons/icon.png"

desktop_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icon_dir="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/512x512/apps"

mkdir -p "$desktop_dir" "$icon_dir"

desktop_target="$desktop_dir/com.varg.editor.desktop"
icon_target="$icon_dir/com.varg.editor.png"

cp "$icon_source" "$icon_target"

sed \
  -e "s|^Exec=.*|Exec=$repo_root/scripts/dev-editor.sh|" \
  "$desktop_source" > "$desktop_target"
chmod 0644 "$desktop_target"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" >/dev/null 2>&1 || true
fi

if command -v kbuildsycoca6 >/dev/null 2>&1; then
  kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
elif command -v kbuildsycoca5 >/dev/null 2>&1; then
  kbuildsycoca5 --noincremental >/dev/null 2>&1 || true
fi
