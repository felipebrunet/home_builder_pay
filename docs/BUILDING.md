# Building

Back to the [README](../README.md).

## Build desktop from source

Needs Rust 1.89 or newer (`monero-wallet` 0.2), GTK3, WebKitGTK 4.1, `libxdo-dev`, and the `tor` package.

```bash
sudo apt install tor libgtk-3-0 libwebkit2gtk-4.1-0 libxdo-dev
cargo test --workspace
cargo run
```

The UI is Spanish by default. Switch to English with **ES / EN** in the top bar (or in the account screen). The deal itself does not change. The Android app has the same switch in **Cuenta → Idioma / Language** and starts in the phone's language (Spanish or English).

State is saved in `~/.konstruado/estado.json` (override with `KONSTRUADO_DATOS`). The seed and the per-job share are under `xmr/` in that same directory, mode 0600, and are not inside `estado.json`.

Two users on one PC need two data dirs:

```bash
KONSTRUADO_DATOS=.konstruado-dinero cargo run
KONSTRUADO_DATOS=.konstruado-chasquilla cargo run
```

Window 1: José, **Pago la obra**, Publicar. Window 2: Juan, **La construyo** — the job appears on his board.

## Build Android APK

`android/` is a Jetpack Compose app on top of `crates/konstruado-ffi` (UniFFI bindings to the same Rust motor). Tor comes from Orbot. See [`android/README.md`](android/README.md). Short path (needs Android SDK + NDK, JDK 17+, `cargo-ndk`):

```bash
android/build-apk.sh          # debug APK (arm64-v8a + x86_64), for development and the emulator
android/build-apk-release.sh  # release APK: Rust in release (arm64-v8a, stripped, LTO), R8, release signature
```

The release APK is signed with the key named by `KONSTRUADO_RELEASE_ENV` (default `~/.config/konstruado-release.env`, outside the repo: `KONSTRUADO_RELEASE_STORE_FILE`, `_STORE_PASSWORD`, `_KEY_ALIAS`, `_KEY_PASSWORD`; the same names also work as Gradle properties). Without it, Gradle signs the release build with the debug key, so anyone can build it.

**Debug → release signature:** Android does not update an app signed with a different key. To move from a debug APK (0.2.9 and earlier) to the release APK, first make a full backup (**Billetera → Respaldos y recuperación → Exportar respaldo completo**, `.kbak`), uninstall the debug app, install the release APK and choose **Restaurar desde respaldo**. Uninstalling deletes the app's private data, including the seed and the job shares.

APKs are **not** stored in git (`*.apk` is ignored). Published builds go on [Releases](https://github.com/felipebrunet/konstruado/releases).

## Icon

`assets/icon/konstruado.svg` is the hand-made source (crossed shovel and pickaxe, app palette: red `#b8321f`, beige `#f4e4cc`, ink `#1c120c`). `python3 assets/icon/generar.py` (needs `rsvg-convert`) regenerates every derived file: `assets/icon/png/konstruado-{16…512}.png`, the desktop window icon `crates/konstruado/assets/konstruado-256.png` (embedded with `include_bytes!`), `assets/linux/konstruado.png`, and the Android adaptive icon (vector foreground, background colour, monochrome layer for themed icons, round icon, PNG fallbacks in `mipmap-*`).

## Linux binary (not production)

On Debian/Ubuntu:

```bash
sudo apt install tor libgtk-3-0 libwebkit2gtk-4.1-0 libxdo3
chmod +x konstruado-*-linux-x86_64*
./konstruado-*-linux-x86_64*
```

Menu entry and icon (per user, no root): `assets/linux/instalar.sh path/to/konstruado-X.Y.Z-linux-x86_64` installs the binary in `~/.local/bin`, `assets/linux/konstruado.desktop` in `~/.local/share/applications` and the icon in `~/.local/share/icons/hicolor/256x256/apps`.

To rebuild it:

```bash
scripts/release-assets.sh     # dist/konstruado-X.Y.Z-linux-x86_64 + dist/konstruado-X.Y.Z-android-arm64.apk
# or only the desktop binary:
cargo build -p konstruado --release
VERSION=$(cargo pkgid -p konstruado | sed 's/.*[#@]//')
mkdir -p dist
cp target/release/konstruado "dist/konstruado-$VERSION-linux-x86_64"
```


How to use the app (roles, two machines, the deal) is **Help → README** inside the window (`crates/konstruado/HELP.md`).
