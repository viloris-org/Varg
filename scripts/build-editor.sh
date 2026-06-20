#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/../editor"

# Tauri runs build.beforeBuildCommand, which builds the Vite frontend.
echo "Building Aster Editor bundle..."
bun run tauri build

echo "Done: editor built to src-tauri/target/release/"
