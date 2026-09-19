#!/usr/bin/env bash
#
# Builds the Windows x64 build of AutoSign plus the one-click installer.
#
# Requirements (Debian/Ubuntu):
#   apt-get install -y mingw-w64 nasm nsis
#   rustup target add x86_64-pc-windows-gnu
#
# The script is intentionally the same shape as the upstream Impactor release
# workflow (Ubuntu + mingw-w64 + NSIS), so the produced installer matches the
# binaries the project already ships.

set -euo pipefail

TARGET="x86_64-pc-windows-gnu"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(awk '/\[workspace.package\]/,/^$/' Cargo.toml | sed -nE 's/version *= *"([^"]*)".*/\1/p')"
if [ -z "$VERSION" ]; then
    echo "Could not determine version from Cargo.toml" >&2
    exit 1
fi

echo "==> Building AutoSign $VERSION for $TARGET"
cargo build --bins --workspace --release --target "$TARGET"

echo "==> Assembling installer"
rm -rf dist/nsis
mkdir -p dist/nsis

cp "target/$TARGET/release/plumeimpactor.exe" dist/nsis/AutoSign.exe
cp "target/$TARGET/release/plumesign.exe" "dist/AutoSign-$VERSION-windows-x64-cli.exe"
cp package/windows/installer.nsi package/windows/deps.ps1 package/windows/icon.ico dist/nsis/
cp -r package/windows/libs dist/nsis/libs
cp "target/$TARGET/release/plumeimpactor.exe" "dist/AutoSign-$VERSION-windows-x64-portable.exe"
mkdir -p dist/portable
cp "target/$TARGET/release/plumeimpactor.exe" "dist/portable/AutoSign.exe"
cp -r package/windows/libs dist/portable/libs

(
    cd dist/nsis
    makensis -DAPPVERSION="$VERSION" installer.nsi
)

mv dist/nsis/installer.exe "dist/AutoSign-$VERSION-windows-x64-setup.exe"

echo
echo "==> Done"
ls -lh dist/*.exe
