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

**Product network is Signet only.** The GUI has no network picker and will not create or import mainnet works. Regtest remains for the CLI, catalog scripts, and unit tests — not the product window.

## Cross-compile from Linux (optional)

```bash
rustup target add x86_64-pc-windows-gnu
# needs mingw-w64
cargo build -p hbp-app --release --target x86_64-pc-windows-gnu
```

This environment did **not** produce a tested `.exe`. Treat the GNU target as a convenience; MSVC on a real Windows box is the supported path.

## Tor (required for Windows v1 networking)

Point-to-point and DHT RPCs to `.onion` go through SOCKS5. The crate does not vendor Tor; it drives a local `tor.exe`.

**Workable approach (Expert Bundle)**

1. Download the [Tor Expert Bundle](https://www.torproject.org/download/tor/) (Windows). Unzip `tor.exe` + `geoip`.
2. Place them either next to `home_builder_pay.exe`, or `%LOCALAPPDATA%\Tor\tor.exe`, or set `TOR_BINARY`.
3. In the GUI: one button, **Conectar red (Tor + DHT)**. The app probes SOCKS **9050** (Expert Bundle) and **9150** (Tor Browser). If nothing is listening it writes `%USERPROFILE%\Documents\home_builder_pay\tor\torrc`:

```
SocksPort 127.0.0.1:9050
ControlPort 127.0.0.1:9051
CookieAuthentication 1
HiddenServiceDir …\tor\hidden_service
HiddenServicePort 80 127.0.0.1:3848
```

   and tries to spawn `tor.exe -f torrc`. When `hidden_service\hostname` appears, that onion is the advertised DHT address (`xxx.onion:80`).
4. **Tor Browser** is enough for outbound (Felipe’s case: Browser up on 9150, app used to time out on 9050). Publishing your own onion still wants the Expert Bundle or a control-port `ADD_ONION`. Advanced: paste the other laptop’s `.onion`.
5. If you already run the Expert Bundle yourself, keep SOCKS on `9050` or let the app find 9150. Optional control-port `ADD_ONION` (`HBP_TOR_CONTROL` / `HBP_TOR_COOKIE`). There is no public bootstrap list yet.

Manual torrc (if you refuse the spawn):

```
# hbp-product.torrc
SocksPort 9050
HiddenServiceDir C:\Users\YOU\AppData\Local\Tor\hbp_hs
HiddenServicePort 80 127.0.0.1:3848
```

Then paste the hostname into the GUI “onion propio” field.

File-passing remains fallback if Tor is down. See [NETWORK.md](NETWORK.md).

## What the GUI does in this PR

- Named works, one secp256k1 identity per work
- Create fee-burn offer with t1/t2 and **stage amount = bond** (10% → N equal stages)
- Accept an offer from a file path
- Stage board
- Signet locked (no mainnet, no network picker)
- One-button Tor + DHT (9050 Expert Bundle **or** 9150 Tor Browser); advanced onion/bootstrap collapsed
- Human local date/time for t1/t2 (no raw unix)
- Dark / light theme toggle (saved in `ui.json`)
- Import / export backup JSON (secret hex, not BIP39; Signet only)
- Arbiter hidden (`ARBITER_ENABLED = false`)

**Not in the window yet:** PSBT funding, MuSig2 reception, signed fee-burn arming.

**Not verified here:** two Windows PCs over a live Tor circuit (this VM has no Expert Bundle / no Signet laptops). The overlay is verified with 2- and 3-node localhost tests.

`cargo run -p hbp-ui` (http://127.0.0.1:3847) is the throwaway test wizard. Do not ship it as the product.
