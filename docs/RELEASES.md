# Versions and releases

Back to the [README](../README.md).

## Version

The version lives once, in `[workspace.package] version` of the root `Cargo.toml`. The desktop window title and **Help → About**, `konstruado-ffi` (Android About) and the APK `versionName` all read it. Bump it there, then tag `vX.Y.Z`.

Each release on [Releases](https://github.com/felipebrunet/konstruado/releases) carries two assets: `konstruado-X.Y.Z-android-arm64.apk` (release-signed, R8) and `konstruado-X.Y.Z-linux-x86_64`. Up to 0.2.9 the APK was `konstruado-X.Y.Z-android-arm64-debug.apk` (debug signature).

`scripts/release-assets.sh` builds both into `dist/`. `[profile.release]` uses `strip`, `lto = "fat"` and `codegen-units = 1`.

The Android `versionCode` is derived from the same version: `major × 10000 + minor × 100 + patch` (0.3.1 → 301).

**Debug → release signature:** Android does not update an app signed with a different key. To move from a debug APK (0.2.9 and earlier) to the release APK, first make a full backup (**Billetera → Respaldos y recuperación → Exportar respaldo completo**, `.kbak`), uninstall the debug app, install the release APK and choose **Restaurar desde respaldo**. Uninstalling deletes the app's private data, including the seed and the job shares.

The project stays on major version 0 (proof of concept).

## Notable versions

| Version | What changed |
|---|---|
| 0.3.1 | USD/XMR price fix on Android (the engine now refreshes it in the background on every screen). Kraken, Bitfinex, CoinGecko and CoinPaprika queried in parallel (8 s each, first answer wins); opt-in clearnet fallback when Tor fails; manual price as last resort; per-source error messages. Compatible with 0.3.0 peers. |
| 0.3.0 | Android in English (all screens, including texts from the Rust motor), language selector in Cuenta; **Source on GitHub** link in both apps; README rewritten, details moved to `docs/`. |
| 0.2.10 | Job amounts and guarantees in USD; each stage's XMR fixed at funding (CoinGecko, Kraken fallback). New app icon. First release-signed Android APK (R8) and release-optimized assets. |
| 0.2.9 | **Show the 25 words**: personal seed, restore height and view key. |
| 0.2.8 | Unlock gate (funding block + 10), 0-amount split instead of 1-piconero dust, one encrypted full backup (`.kbak`). |
| 0.2.7 | Real Orbot / room diagnosis on Android, card layout and AA palette. |
| 0.2.6 | Signed offer withdrawal that gossip cannot revive. |
