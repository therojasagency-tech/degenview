#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
app="$PWD/target/DegenView.app/Contents/MacOS/DegenView"
[[ -x "$app" ]] || { echo 'Build with bash tools/bundle.sh first.' >&2; exit 1; }
read -r -p 'OANDA environment [practice/live, default practice]: ' environment
environment="${environment:-practice}"
[[ "$environment" == practice || "$environment" == live ]] || { echo 'Invalid environment' >&2; exit 1; }
read -r -p 'OANDA v20 account ID: ' account
read -r -s -p 'OANDA API token (hidden): ' token
echo
[[ -n "$token" && -n "$account" ]] || { echo 'Account and token required' >&2; exit 1; }
# Only the app process gets the token; never save it or print it.
export DEGENVIEW_OANDA_ENV="$environment" DEGENVIEW_OANDA_ACCOUNT="$account" DEGENVIEW_OANDA_TOKEN="$token"
unset token
exec "$app"
