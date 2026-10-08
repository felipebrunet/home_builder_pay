#!/usr/bin/env bash
# Arma los dos assets de un release en dist/:
#   konstruado-X.Y.Z-linux-x86_64        (cargo --release: strip + LTO)
#   konstruado-X.Y.Z-android-arm64.apk   (android/build-apk-release.sh)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=/dev/null
source "$HOME/.cargo/env" 2>/dev/null || true
cd "$ROOT"
VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' Cargo.toml)"
mkdir -p dist
cargo build -p konstruado --release
cp -f target/release/konstruado "dist/konstruado-$VERSION-linux-x86_64"
SALIDA="$ROOT/dist/konstruado-$VERSION-android-arm64.apk" bash android/build-apk-release.sh
( cd dist && sha256sum "konstruado-$VERSION-linux-x86_64" "konstruado-$VERSION-android-arm64.apk" )
