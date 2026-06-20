#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root/editor"

echo "Starting Aster Editor (Tauri) dev mode..."
bun run dev:tauri
