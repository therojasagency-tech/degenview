#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
CARGO_BIN="${CARGO_BIN:-cargo}"
identity="${DEGENVIEW_CODESIGN_IDENTITY:-}"
access_group="${DEGENVIEW_KEYCHAIN_ACCESS_GROUP:-}"
# keychain-access-groups is a restricted entitlement: macOS kills a signed build at launch unless
# a provisioning profile that grants it is embedded in each bundle.
app_profile="${DEGENVIEW_APP_PROFILE:-}"
helper_profile="${DEGENVIEW_HELPER_PROFILE:-}"
# iCloud Sync is opt-in: set DEGENVIEW_ICLOUD=1 to compile the CloudKit bridge and sign the app with
# the iCloud entitlements. The app profile must then grant them (see docs/icloud-sync.md); without
# the profile macOS kills the signed app at launch. The helper never gets iCloud entitlements.
icloud="${DEGENVIEW_ICLOUD:-}"
icloud_environment="${DEGENVIEW_ICLOUD_ENVIRONMENT:-Production}"
aps_environment=production
# A development build (DEGENVIEW_DEV_BUILD=1, signed with an Apple Development identity and a Mac
# Development profile) talks to CloudKit's Development environment, where record types can be
# created. Debug assertions make the app expect that environment; a release never has them.
if [ -n "${DEGENVIEW_DEV_BUILD:-}" ]; then
    icloud=1
    icloud_environment=Development
    aps_environment=development
    export CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true
fi
# Cargo.toml is the one place the version lives; tools/release.sh bumps it before tagging.
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
if [ -z "$version" ]; then
    echo "could not read the version from Cargo.toml" >&2
    exit 1
fi

signing_vars=0
for value in "$identity" "$access_group" "$app_profile" "$helper_profile"; do
    [ -n "$value" ] && signing_vars=$((signing_vars + 1))
done
if [ "$signing_vars" -ne 0 ] && [ "$signing_vars" -ne 4 ]; then
    echo "DEGENVIEW_CODESIGN_IDENTITY, DEGENVIEW_KEYCHAIN_ACCESS_GROUP, DEGENVIEW_APP_PROFILE and DEGENVIEW_HELPER_PROFILE must be supplied together" >&2
    exit 1
fi

features=""
[ -n "$icloud" ] && features="--features icloud"
# The release workflow compiles first, before the signing certificate is imported, so the build
# scripts of the dependency graph never run with it available; it then sets DEGENVIEW_SKIP_BUILD.
# --locked: the build must use Cargo.lock as committed, never resolve anything new.
if [ -z "${DEGENVIEW_SKIP_BUILD:-}" ]; then
    "$CARGO_BIN" build --release --locked $features --bin degenview --bin degenview-alert-agent
elif [ ! -x target/release/degenview ] || [ ! -x target/release/degenview-alert-agent ]; then
    echo "DEGENVIEW_SKIP_BUILD is set but target/release has no built binaries" >&2
    exit 1
fi

# Local builds embed the pinned OpenChart client automatically if the sibling checkout is present.
# CI/upstream builds without that checkout retain the source launcher. Set 1 to require embedding.
checkout="${DEGENVIEW_OPENCHART_CHECKOUT:-../openchart}"
embed="${DEGENVIEW_EMBED_OPENCHART:-auto}"
case "$embed" in
    auto) if [ -z "$identity" ] && [ -d "$checkout/node_modules" ]; then embed=1; else embed=0; fi ;;
    0|1) ;;
    *) echo "DEGENVIEW_EMBED_OPENCHART must be auto, 0 or 1" >&2; exit 2 ;;
esac
if [ "$embed" = 1 ]; then
    # Developer ID / hardened runtime needs a separate signing and licensing audit.
    if [ -n "$identity" ]; then
        echo "Embedded OpenChart is currently supported in local unsigned builds only." >&2
        exit 1
    fi
    export PATH="$HOME/.bun/bin:/opt/homebrew/bin:$PATH"
    command -v bun >/dev/null || { echo "Bun is required to BUILD the embedded bridge." >&2; exit 1; }
    checkout=$(cd "$checkout" && pwd)
    mkdir -p target
    build_root=$(mktemp -d "$PWD/target/.openchart-build.XXXXXX")
    trap 'rm -rf "$build_root"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    bridge_bin="$build_root/OpenChartBridge"
    /bin/bash tools/data-bridge/run-openchart.sh "$checkout" --compile "$bridge_bin"
    test -x "$bridge_bin"
    # Require the same native architecture as the Rust app.
    [ "$(lipo -archs "$bridge_bin")" = "$(lipo -archs target/release/degenview)" ] || {
        echo "OpenChartBridge architecture does not match DegenView" >&2; exit 1;
    }
    mkdir "$build_root/empty"
    # Test the actual executable without Bun on PATH or access to the source working directory.
    (cd "$build_root/empty" && /usr/bin/env -i PATH=/usr/bin:/bin HOME="$build_root/empty" "$bridge_bin" --self-test)
    echo "Embedded OpenChartBridge offline test passed"
fi

app="target/DegenView.app"
helper="$app/Contents/Library/LoginItems/DegenViewAlertAgent.app"
executable="$app/Contents/MacOS/DegenView"
helper_executable="$helper/Contents/MacOS/DegenViewAlertAgent"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
mkdir -p "$helper/Contents/MacOS" "$helper/Contents/Resources"
cp target/release/degenview "$executable"
# Ship the bridge source, not the user checkout, node_modules or secrets.
mkdir -p "$app/Contents/Resources/data-bridge"
cp tools/data-bridge/run-openchart.sh tools/data-bridge/adapter.mjs tools/data-bridge/openchart-entry.ts tools/data-bridge/browser-login.ts tools/data-bridge/browser-login-smoke.ts tools/data-bridge/native-smoke.ts tools/data-bridge/live-server.ts tools/data-bridge/openchart-runtime.ts "$app/Contents/Resources/data-bridge/"
if [ "$embed" = 1 ]; then
    cp "$bridge_bin" "$app/Contents/MacOS/OpenChartBridge"
    chmod 755 "$app/Contents/MacOS/OpenChartBridge"
    mkdir -p "$app/Contents/Resources/Licenses"
    cp "$checkout/LICENSE" "$app/Contents/Resources/Licenses/OpenChart-MIT.txt"
    echo "OpenChartBridge included in $app"
fi
cp target/release/degenview-alert-agent "$helper_executable"
cp assets/brand/DegenView.icns "$app/Contents/Resources/DegenView.icns"
# The fonts compiled into the app are under the SIL OFL, which wants its text and the copyright
# notice to travel with every copy of them.
mkdir -p "$app/Contents/Resources/Licenses"
cp assets/fonts/OFL-JetBrainsMono.txt assets/fonts/OFL-NotoSansSymbols2.txt assets/fonts/OFL-Inter.txt "$app/Contents/Resources/Licenses/"
cp assets/fonts/README.md "$app/Contents/Resources/Licenses/FONTS.md"
# The helper shares the app icon: Login Items settings and its notifications show the bundle icon.
cp assets/brand/DegenView.icns "$helper/Contents/Resources/DegenView.icns"

cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleLocalizations</key><array><string>en</string><string>de</string><string>es</string><string>zh-Hans</string></array>
  <key>CFBundleName</key><string>DegenView</string>
  <key>CFBundleDisplayName</key><string>DegenView</string>
  <key>CFBundleIdentifier</key><string>es.nico.degenview</string>
  <key>CFBundleExecutable</key><string>DegenView</string>
  <key>CFBundleIconFile</key><string>DegenView.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>__VERSION__</string>
  <key>CFBundleShortVersionString</key><string>__VERSION__</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

cat > "$helper/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleLocalizations</key><array><string>en</string><string>de</string><string>es</string><string>zh-Hans</string></array>
  <key>CFBundleName</key><string>DegenView Alert Agent</string>
  <key>CFBundleDisplayName</key><string>DegenView Alert Agent</string>
  <key>CFBundleIdentifier</key><string>es.nico.degenview.alert-agent</string>
  <key>CFBundleExecutable</key><string>DegenViewAlertAgent</string>
  <key>CFBundleIconFile</key><string>DegenView.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>__VERSION__</string>
  <key>CFBundleShortVersionString</key><string>__VERSION__</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
PLIST

# Info.plist versions are numeric: a pre-release tag (0.2.0-rc.1) ships as 0.2.0.
sed -i '' "s/__VERSION__/${version%%-*}/" "$app/Contents/Info.plist" "$helper/Contents/Info.plist"

if [ -n "$identity" ]; then
    # The access group is `<TEAMID>.<name>`; each bundle's application identifier is its own
    # `<TEAMID>.<bundle id>`, which its profile must grant.
    team_id="${access_group%%.*}"
    cp "$app_profile" "$app/Contents/embedded.provisionprofile"
    cp "$helper_profile" "$helper/Contents/embedded.provisionprofile"
    sign_bundle() {
        bundle="$1"
        bundle_id="$2"
        template="tools/entitlements.plist"
        # Only the app carries iCloud; the login item has no CloudKit and its profile cannot grant it.
        if [ -n "$icloud" ] && [ "$bundle_id" = "es.nico.degenview" ]; then
            template="tools/entitlements-app.plist"
        fi
        entitlements="target/$bundle_id.entitlements"
        sed -e "s|__KEYCHAIN_ACCESS_GROUP__|$access_group|g" \
            -e "s|__TEAM_ID__|$team_id|g" \
            -e "s|__ICLOUD_ENVIRONMENT__|$icloud_environment|g" \
            -e "s|__APS_ENVIRONMENT__|$aps_environment|g" \
            -e "s|__APPLICATION_IDENTIFIER__|$team_id.$bundle_id|g" \
            "$template" > "$entitlements"
        codesign --force --options runtime --timestamp --entitlements "$entitlements" \
            --sign "$identity" "$bundle"
    }
    sign_bundle "$helper" es.nico.degenview.alert-agent
    sign_bundle "$app" es.nico.degenview
    codesign --verify --deep --strict --verbose=2 "$app"
    if [ -n "$icloud" ]; then
        ./tools/verify-icloud.sh "$app" "$icloud_environment"
    fi
    echo "Built and signed $app (background agent available)"
else
    if [ -n "$icloud" ]; then
        echo "Built with the iCloud bridge but unsigned: iCloud Sync will report that it is unavailable" >&2
    fi
    echo "Built $app unsigned; background alerts use the foreground fallback"
fi
