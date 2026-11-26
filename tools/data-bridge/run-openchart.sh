#!/bin/bash
set -euo pipefail
bridge="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "${1:?Usage: bash run-openchart.sh OPENCHART_CHECKOUT [--smoke|--auth-smoke|--serve]}" && pwd)"
mode="${2:---smoke}"
pin="01b0ad35f44fc886855356da1e7281b7320fbaac"
test "$(git -C "$root" rev-parse HEAD)" = "$pin" || {
  echo "OpenChart checkout does not match the tested revision: $pin" >&2
  exit 1
}
if ! test -d "$root/node_modules"; then
  echo "Install OpenChart dependencies using its just install --frozen-lockfile command first." >&2
  exit 1
fi
case "$mode" in
  --smoke) entry=native-smoke.ts ;;
  --auth-smoke) entry=browser-login-smoke.ts ;;
  --serve) entry=openchart-entry.ts ;;
  *) echo "Unknown mode" >&2; exit 2 ;;
esac
stage="$(mktemp -d "$root/.degenview-bridge.XXXXXX")"
child=""
cleanup() {
  if test -n "$child"; then kill "$child" 2>/dev/null || true; fi
  rm -f "$stage/adapter.mjs" "$stage/openchart-entry.ts" "$stage/native-smoke.ts" "$stage/browser-login.ts" "$stage/browser-login-smoke.ts" "$stage/live-server.ts"
  rmdir "$stage"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cp "$bridge/adapter.mjs" "$bridge/openchart-entry.ts" "$bridge/native-smoke.ts" "$bridge/browser-login.ts" "$bridge/browser-login-smoke.ts" "$bridge/live-server.ts" "$stage/"
cd "$root"
bun "$stage/$entry" &
child=$!
wait "$child"
child=""
