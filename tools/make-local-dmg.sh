#!/bin/bash
# Local ARM64 installer. Packages the already-built autonomous app without rebuilding it.
set -euo pipefail
cd "$(dirname "$0")/.."
app="$PWD/target/DegenView.app"
out="$PWD/target/DegenView-arm64.dmg"
if [[ "$(uname -s)" != Darwin ]]; then
    echo "This installer must be built on macOS." >&2
    exit 1
fi
main="$app/Contents/MacOS/DegenView"
bridge="$app/Contents/MacOS/OpenChartBridge"
if [[ ! -x "$main" || ! -x "$bridge" ]]; then
    echo "Build the autonomous app first: DEGENVIEW_EMBED_OPENCHART=1 bash tools/bundle.sh" >&2
    exit 1
fi
if [[ "$(lipo -archs "$main")" != arm64 || "$(lipo -archs "$bridge")" != arm64 ]]; then
    echo "Both executables must be native ARM64." >&2
    exit 1
fi
test -f "$app/Contents/Resources/Licenses/OpenChart-MIT.txt"
work="$(mktemp -d "$PWD/target/.local-dmg.XXXXXX")"
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir "$work/stage" "$work/empty"
# Preserve app bundle metadata and signatures, if any; package no user data or source checkout.
/usr/bin/ditto "$app" "$work/stage/DegenView.app"
ln -s /Applications "$work/stage/Applications"
# Test the actual copy being packaged, with a clean environment and no external Bun on PATH.
(cd "$work/empty" && /usr/bin/env -i PATH=/usr/bin:/bin HOME="$work/empty" \
    "$work/stage/DegenView.app/Contents/MacOS/OpenChartBridge" --self-test)
/usr/bin/hdiutil create -srcfolder "$work/stage" -volname "DegenView" \
    -format UDZO "$work/DegenView.dmg"
/usr/bin/hdiutil verify "$work/DegenView.dmg"
mv -f "$work/DegenView.dmg" "$out"
echo "Built and verified $out"
echo "Local installer only: not Developer ID signed or notarized by this script."
