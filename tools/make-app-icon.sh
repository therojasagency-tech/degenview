#!/bin/bash
# Convert a user-selected square PNG into a native macOS multi-resolution icon.
set -euo pipefail
input="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
output="$2"
[[ -f "$input" ]] || { echo "Icon file not found" >&2; exit 1; }
[[ "$output" == /* ]] || { echo "Use an absolute output path" >&2; exit 1; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
case "$input" in
    *.icns|*.ICNS)
        /usr/bin/iconutil -c iconset "$input" -o "$work/DegenView.iconset"
        ;;
    *.png|*.PNG)
        width="$(/usr/bin/sips -g pixelWidth "$input" | awk '/pixelWidth:/ {print $2}')"
        height="$(/usr/bin/sips -g pixelHeight "$input" | awk '/pixelHeight:/ {print $2}')"
        if [[ "$width" != "$height" || ! "$width" =~ ^[0-9]+$ ]] || (( width < 512 )); then
            echo "Use a square PNG of at least 512 x 512 pixels (1024 x 1024 recommended)." >&2
            exit 1
        fi
        mkdir "$work/DegenView.iconset"
        for size in 16 32 128 256 512; do
            /usr/bin/sips -z "$size" "$size" "$input" --out "$work/DegenView.iconset/icon_${size}x${size}.png" >/dev/null
            double=$((size*2))
            /usr/bin/sips -z "$double" "$double" "$input" --out "$work/DegenView.iconset/icon_${size}x${size}@2x.png" >/dev/null
        done
        ;;
    *) echo "Select a PNG or ICNS file." >&2; exit 1 ;;
esac
/usr/bin/iconutil -c icns "$work/DegenView.iconset" -o "$work/DegenView.icns"
cp "$work/DegenView.icns" "$output"
echo "Custom icon ready: $output"
