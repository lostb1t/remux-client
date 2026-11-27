#!/usr/bin/env bash
# Builds the player of the desktop app on a Mac: the `RemuxMpv` framework (`macos/RemuxMpv`, mpv with
# a macOS library), into `macos/build/framework`, where the build of the app finds it. It has to
# be there before the app is built. `release` (the default for a build that is shipped) or `debug`.
set -euo pipefail

cd "$(dirname "$0")/../macos/RemuxMpv"
config="${1:-Release}"
xcodebuild -scheme RemuxMpv -configuration "$config" -destination 'generic/platform=macOS' \
  -derivedDataPath ../build/derived build | tail -3
chmod -R u+w ../build/framework 2>/dev/null || true
rm -rf ../build/framework
mkdir -p ../build/framework
cp -R "../build/derived/Build/Products/$config/PackageFrameworks/RemuxMpv.framework" ../build/framework/
echo "$(cd ../build/framework && pwd)/RemuxMpv.framework"
