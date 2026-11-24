#!/usr/bin/env bash
#
# Builds the macOS (Apple Silicon) release of AutoSign and packages it as a DMG.
#
# The application is ad-hoc signed: it runs, but Gatekeeper will still ask the
# user to confirm the first launch (right-click -> Open, or
# `xattr -dr com.apple.quarantine /Applications/AutoSign.app`).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET="aarch64-apple-darwin"
VERSION="$(awk '/\[workspace.package\]/,/^$/' Cargo.toml | sed -nE 's/version *= *"([^"]*)".*/\1/p')"
STAMP="$(date +%Y%m%d-%H%M%S)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/autosign-macos-$STAMP-XXXX")"
DMG_ROOT="$WORK/dmg"
APP="$DMG_ROOT/AutoSign.app"

echo "==> Building AutoSign $VERSION for $TARGET"
cargo --config 'source.crates-io.replace-with="rsproxy"' \
      --config 'source.rsproxy.registry="sparse+https://rsproxy.cn/index/"' \
      build --bins --workspace --release --target "$TARGET"

echo "==> Assembling the application bundle"
mkdir -p "$DMG_ROOT"
# The template is intentionally not named `*.app`: a bundle without its
# executable inside the source tree would otherwise show up in Spotlight and
# make macOS ask "where is Impactor?" when someone opens it.
cp -R package/macos/app-template "$APP"
mkdir -p "$APP/Contents/MacOS"

cp "target/$TARGET/release/plumeimpactor" "$APP/Contents/MacOS/AutoSign"
cp "target/$TARGET/release/plumesign" "$APP/Contents/MacOS/plumesign"
chmod +x "$APP/Contents/MacOS/AutoSign" "$APP/Contents/MacOS/plumesign"

PLIST="$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleExecutable AutoSign" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName AutoSign" "$PLIST" 2>/dev/null ||
    /usr/libexec/PlistBuddy -c "Add :CFBundleDisplayName string AutoSign" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleName AutoSign" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$PLIST"

# Anisette libraries are deliberately not bundled: the Android slice used for
# local anisette generation crashes on macOS, so the app uses its remote
# fallback there (the same behaviour as the upstream macOS build).

echo "==> Signing (ad-hoc)"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

echo "==> Creating the DMG"
ln -s /Applications "$DMG_ROOT/Applications"
mkdir -p dist
DMG="dist/AutoSign-$VERSION-macos-arm64.dmg"
for attempt in 1 2 3; do
    if hdiutil create -volname "AutoSign" -srcfolder "$DMG_ROOT" -ov -format UDZO "$DMG"; then
        break
    fi

    if [ "$attempt" -eq 3 ]; then
        echo "hdiutil failed after $attempt attempts" >&2
        exit 1
    fi

    echo "hdiutil reported a transient error; retrying ($attempt/3)"
    rm -f "$DMG"
    sleep 5
done

echo
echo "==> Done: $DMG"
ls -lh "$DMG"
echo "Work directory kept at $WORK"
