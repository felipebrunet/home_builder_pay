# Windows build (product GUI)

The product is a **native Windows executable**, not Electron and not `hbp-ui`.

Binary name: `home_builder_pay.exe` (crate `hbp-app`). CLI remains `hbp.exe`.

## Build on Windows

Needs Rust **1.88+** (`rust-toolchain.toml`). On Windows:

```bat
rustup target add x86_64-pc-windows-msvc
cargo test --workspace
cargo build -p hbp-app --release
cargo build -p hbp-cli --release
:: target\release\home_builder_pay.exe
:: target\release\hbp.exe
```

Works directory default: `%USERPROFILE%\Documents\home_builder_pay` (override `HBP_WORKS`).

## Cross-compile from Linux (optional)

```bash
rustup target add x86_64-pc-windows-gnu
# needs mingw-w64
cargo build -p hbp-app --release --target x86_64-pc-windows-gnu
```

This environment did **not** produce a tested `.exe`. Treat the GNU target as a convenience; MSVC on a real Windows box is the supported path.

## Tor (required for Windows v1 networking)

Point-to-point traffic goes through SOCKS5. The app does not ship Tor inside the Rust crate; it expects a local `tor.exe`.

**Workable approach**

1. Download the [Tor Expert Bundle](https://www.torproject.org/download/tor/) (Windows).
2. Place `tor.exe` (and `geoip`) either:
   - next to `home_builder_pay.exe`, or
   - in `%LOCALAPPDATA%\Tor\tor.exe`, or
   - set `TOR_BINARY` to the full path.
3. Run Tor so it listens on `127.0.0.1:9050` (default). Override with `HBP_TOR_SOCKS=127.0.0.1:9050`.
4. In the GUI: “Probar SOCKS Tor”. Green means something accepted TCP on that port — not that a hidden service exists.

The app **connects** to a peer `.onion` already written in the offer / DHT announce. It does **not** yet spawn `HiddenServiceDir` for you. File-passing remains the fallback.

See [NETWORK.md](NETWORK.md).

## What the GUI does in this PR

- Named works, one secp256k1 identity per work
- Create fee-burn offer with t1/t2 and **stage amount = bond** (10% → N equal stages)
- Accept an offer from a file path
- Stage board
- Tor status + local DHT announce
- Import / export backup JSON (secret hex, not BIP39)
- Arbiter hidden (`ARBITER_ENABLED = false`)

**Not in the window yet:** PSBT funding, MuSig2 reception, signed fee-burn arming, WAN DHT.

`cargo run -p hbp-ui` (http://127.0.0.1:3847) is the throwaway test wizard. Do not ship it as the product.
