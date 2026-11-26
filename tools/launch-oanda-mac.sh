#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode="${1:---oanda-only}"
case "$mode" in
  --oanda-only) ;;
  --with-openchart) [[ "${DEGENVIEW_TV_AUTHORIZED:-}" == 1 ]] || { echo "Written data access authorization is required for OpenChart." >&2; exit 1; } ;;
  *) echo "Usage: bash tools/launch-oanda-mac.sh [--oanda-only|--with-openchart]" >&2; exit 2 ;;
esac
app="$PWD/target/DegenView.app/Contents/MacOS/DegenView"
[[ -x "$app" ]] || { echo 'Build with bash tools/bundle.sh first.' >&2; exit 1; }
command -v python3 >/dev/null || { echo 'Python 3 is required to parse the account list.' >&2; exit 1; }
read -r -p 'OANDA environment [practice/live, default practice]: ' environment
environment="${environment:-practice}"
case "$environment" in
  practice) host=https://api-fxpractice.oanda.com ;;
  live) host=https://api-fxtrade.oanda.com ;;
  *) echo 'Invalid environment' >&2; exit 1 ;;
esac
read -r -s -p 'OANDA v20 API token (hidden): ' token
echo
[[ "$token" =~ ^[A-Za-z0-9._~-]+$ ]] || { echo 'Invalid or empty token.' >&2; exit 1; }
# Pass the header on stdin, not in curl's command line or a file.
if ! accounts=$(printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --disable --silent --show-error --fail --max-time 20 --proto '=https' --config - "$host/v3/accounts" |
    python3 -c 'import json,re,sys
try:
    rows = json.load(sys.stdin)["accounts"]
    ids = list(dict.fromkeys(row["id"] for row in rows))
    if not ids or any(not re.fullmatch(r"[0-9]+(?:-[0-9]+)+", i) for i in ids):
        raise ValueError("No valid v20 accounts")
    print("\n".join(ids))
except (ValueError, KeyError, TypeError):
    sys.exit("No valid OANDA v20 accounts returned.")'); then
    echo 'Account discovery failed. Check the token, practice/live environment and v20 API access.' >&2
    exit 1
fi
ids=()
while IFS= read -r id; do ids+=("$id"); done <<< "$accounts"
if [[ ${#ids[@]} -eq 1 ]]; then
    account="${ids[0]}"
    echo 'One authorized account detected.'
else
    echo 'Select an authorized OANDA account:'
    PS3='Account number: '
    select account in "${ids[@]}"; do
        if [[ -n "$account" ]]; then break; fi
        echo 'Invalid selection.'
    done
    [[ -n "${account:-}" ]] || exit 1
fi
export DEGENVIEW_OANDA_ENV="$environment" DEGENVIEW_OANDA_ACCOUNT="$account" DEGENVIEW_OANDA_TOKEN="$token"
unset token
if [[ "$mode" == --with-openchart ]]; then
    exec bash tools/launch-openchart-mac.sh
fi
exec "$app"
