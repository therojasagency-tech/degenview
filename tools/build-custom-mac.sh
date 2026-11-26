#!/bin/bash
set -euo pipefail

if [[ "${1:-}" == --help ]]; then
  printf '%s\n' 'Usage: bash tools/build-custom-mac.sh [--test-bridge]' \
    'Build the native ARM64 application and create dist/DegenView-Custom-arm64.dmg.' \
    '--test-bridge runs the offline JavaScript adapter tests first (Node.js required).'
  exit 0
fi
if [[ $# -gt 1 || ( $# -eq 1 && "$1" != --test-bridge ) ]]; then
  echo 'Unknown arguments. Use --help.' >&2
  exit 2
fi
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  echo 'Run this script in a native Apple Silicon macOS terminal (not Rosetta).' >&2
  exit 1
fi
for command in cargo rustc xcrun hdiutil ditto lipo; do
  command -v "$command" >/dev/null || { echo "Missing prerequisite: $command" >&2; exit 1; }
done
xcrun --find clang >/dev/null
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if [[ "${1:-}" == --test-bridge ]]; then
  command -v node >/dev/null || { echo 'Install Node.js 24 to run bridge tests.' >&2; exit 1; }
  node --test tools/data-bridge/adapter.test.mjs
fi
bash tools/bundle.sh
app='target/DegenView.app'
for binary in "$app/Contents/MacOS/DegenView" "$app/Contents/Library/LoginItems/DegenViewAlertAgent.app/Contents/MacOS/DegenViewAlertAgent"; do
  lipo -verify_arch arm64 "$binary"
done
mkdir -p dist
stage="$(mktemp -d "$root/dist/.custom-build.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/root"
ditto "$app" "$stage/root/DegenView.app"
ln -s /Applications "$stage/root/Applications"
hdiutil create -volname 'DegenView Custom' -srcfolder "$stage/root" -format UDZO "$stage/DegenView-Custom-arm64.dmg"
hdiutil verify "$stage/DegenView-Custom-arm64.dmg"
mv -f "$stage/DegenView-Custom-arm64.dmg" dist/DegenView-Custom-arm64.dmg
printf '\nInstaller: %s\n' "$root/dist/DegenView-Custom-arm64.dmg"
