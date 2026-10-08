#!/usr/bin/env bash
# APK release: Rust en release (arm64-v8a, sin símbolos), R8 y firma release.
# La firma sale de KONSTRUADO_RELEASE_ENV (por defecto ~/.config/konstruado-release.env,
# fuera del repo). Sin ese archivo, Gradle firma con la clave debug.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-/workspace/android-sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/26.3.11579264}"
export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk-amd64}"
# shellcheck source=/dev/null
source "$HOME/.cargo/env" 2>/dev/null || true
cd "$ROOT"
VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' Cargo.toml)"
SALIDA="${SALIDA:-$ROOT/dist/konstruado-$VERSION-android-arm64.apk}"
FIRMA="${KONSTRUADO_RELEASE_ENV:-$HOME/.config/konstruado-release.env}"
if [[ -r "$FIRMA" ]]; then
  set -a
  # shellcheck source=/dev/null
  source "$FIRMA"
  set +a
else
  echo "aviso: no hay $FIRMA; el APK release queda firmado con la clave debug" >&2
fi
# 1. Bindings Kotlin desde la lib debug (la release va sin símbolos y pierde la metadata de UniFFI).
cargo build -p konstruado-ffi --lib --bin uniffi-bindgen
TMP="$(mktemp -d)"
./target/debug/uniffi-bindgen generate target/debug/libkonstruado_ffi.so --library -l kotlin -o "$TMP" --no-format
cp -f "$TMP/uniffi/konstruado_ffi/konstruado_ffi.kt" android/app/src/main/java/uniffi/konstruado_ffi/konstruado_ffi.kt
rm -rf "$TMP"
# 2. Lib nativa release, solo arm64-v8a ([profile.release]: strip + LTO).
cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build -p konstruado-ffi --release --lib
# 3. APK release (R8 + firma).
cd android
./gradlew :app:assembleRelease -Pabis=arm64-v8a
mkdir -p "$(dirname "$SALIDA")"
cp -f app/build/outputs/apk/release/app-release.apk "$SALIDA"
ls -lh "$SALIDA"
