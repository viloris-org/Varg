#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root/editor"

"$repo_root/scripts/check-linux-inotify-capacity.py" --startup-only

if "$repo_root/scripts/check-linux-inotify-capacity.py" --inotify-only --quiet; then
  exec bunx tauri dev "$@"
fi

"$repo_root/scripts/check-linux-inotify-capacity.py" --inotify-only
exit 1
