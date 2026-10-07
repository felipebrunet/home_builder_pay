#!/usr/bin/env bash
# Compila konstruado-ffi para Android, regenera los bindings Kotlin y arma el APK debug.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-/workspace/android-sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/26.3.11579264}"
export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk-amd64}"
SALIDA="${SALIDA:-/workspace/konstruado-android-debug.apk}"
# shellcheck source=/dev/null
source "$HOME/.cargo/env" 2>/dev/null || true
cd "$ROOT"
# 1. Bindings Kotlin desde la lib debug (la release va con strip y pierde la metadata de UniFFI).
cargo build -p konstruado-ffi --lib --bin uniffi-bindgen
TMP="$(mktemp -d)"
./target/debug/uniffi-bindgen generate target/debug/libkonstruado_ffi.so --library -l kotlin -o "$TMP" --no-format
cp -f "$TMP/uniffi/konstruado_ffi/konstruado_ffi.kt" android/app/src/main/java/uniffi/konstruado_ffi/konstruado_ffi.kt
rm -rf "$TMP"
# 2. Libs nativas release.
cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs \
  build -p konstruado-ffi --release --lib
# 3. APK.
cd android
./gradlew :app:assembleDebug
cp -f app/build/outputs/apk/debug/app-debug.apk "$SALIDA"
ls -lh "$SALIDA"
