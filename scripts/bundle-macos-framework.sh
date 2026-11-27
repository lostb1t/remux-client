#!/usr/bin/env bash
# Puts the player's framework (`macos/build/framework/RemuxMpv.framework`, built by
# build-macos-framework.sh) into a packaged Mac app, and signs the app again: `nsh package` does
# not know of it, and the player loads it from the app's Frameworks folder (player/mpv_mac.rs).
#
#   scripts/bundle-macos-framework.sh path/to/Remux.app [signing identity]
#
# Without an identity the app is signed ad hoc, as `nsh package` signs it without one.
set -euo pipefail

app="$1"
identity="${2:--}"
framework="$(cd "$(dirname "$0")/.." && pwd)/macos/build/framework/RemuxMpv.framework"
[ -d "$framework" ] || { echo "no $framework: run scripts/build-macos-framework.sh first" >&2; exit 1; }

mkdir -p "$app/Contents/Frameworks"
rm -rf "$app/Contents/Frameworks/RemuxMpv.framework"
cp -R "$framework" "$app/Contents/Frameworks/"
# Inside out: the framework first, then the app around it.
codesign --force --sign "$identity" --timestamp=none "$app/Contents/Frameworks/RemuxMpv.framework"
codesign --force --sign "$identity" --timestamp=none --preserve-metadata=entitlements,flags "$app"
codesign --verify --deep "$app" && echo "$app: framework in, signed"
