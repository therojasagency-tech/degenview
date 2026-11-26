#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "${DEGENVIEW_TV_AUTHORIZED:-}" != 1 ]]; then
    echo 'TradingView requires prior express written authorization for programmatic access to undocumented interfaces.' >&2
    echo 'Enable this integration only if your actual authorization covers this use.' >&2
    exit 1
fi
export PATH="$HOME/.bun/bin:$PATH"
command -v bun >/dev/null || { echo 'Bun is required.' >&2; exit 1; }
app="$PWD/target/DegenView.app/Contents/MacOS/DegenView"
[[ -x "$app" ]] || { echo 'Build with bash tools/bundle.sh first.' >&2; exit 1; }
checkout="${1:-../openchart}"
[[ -d "$checkout" ]] || { echo 'The pinned OpenChart checkout is missing.' >&2; exit 1; }
export DEGENVIEW_BRIDGE_APP="$app"
exec bash tools/data-bridge/run-openchart.sh "$checkout" --serve
