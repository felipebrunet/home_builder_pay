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

## Tor (one click — you should be findable)

Point-to-point and DHT RPCs to `.onion` go through SOCKS5. The app does **not** vendor Tor in git. On **Conectar red (Tor + DHT)** it:

1. Looks for `tor.exe` next to `home_builder_pay.exe`, in `%LOCALAPPDATA%\home_builder_pay\tor\`, on `PATH`, or inside a Tor Browser install (binary only).
2. If nothing is there, **downloads** the official [Tor Expert Bundle](https://dist.torproject.org/torbrowser/) (`tor-expert-bundle-windows-x86_64-*.tar.gz`) into `%LOCALAPPDATA%\home_builder_pay\tor\`. Tor is © The Tor Project, BSD-3-Clause; we do not relicense it.
3. Writes a private `torrc` under the works directory and **spawns Tor with a Hidden Service** (`HiddenServicePort 80 → 127.0.0.1:<DHT port>`), on ports around `19050` so it does not fight Tor Browser’s `9150`.
4. Waits for `hidden_service\hostname` and shows **Conectado. Ya puedes ser encontrado** plus your `.onion` on the main Red panel (copy button). No trip into Avanzado for the happy path.

Tor Browser’s SOCKS (`9150`) is **outbound fallback only** if spawn/download fails. That lets you talk; it does not make you findable.

You do not need to know “Expert Bundle” vs “Tor Browser”. One button.

File-passing remains fallback if Tor is down. See [NETWORK.md](NETWORK.md).

## What the GUI does in this PR

- Named works, one secp256k1 identity per work
- Create fee-burn offer with t1/t2 and **stage amount = bond** (10% → N equal stages)
- Accept an offer from a file path
- Stage board
- Signet locked (no mainnet, no network picker)
- One-button Tor + DHT that **spawns a Hidden Service** (download official bundle if needed). Status: *Ya puedes ser encontrado*. Avanzado is optional.
- Human local date/time for t1/t2 (no raw unix)
- Dark / light theme toggle (saved in `ui.json`)
- Import / export backup JSON (secret hex, not BIP39; Signet only)
- Arbiter hidden (`ARBITER_ENABLED = false`)

**Not in the window yet:** PSBT funding, MuSig2 reception, signed fee-burn arming.

**Not verified here:** two Windows PCs over a live Tor circuit (this VM has no Expert Bundle / no Signet laptops). The overlay is verified with 2- and 3-node localhost tests.

`cargo run -p hbp-ui` (http://127.0.0.1:3847) is the throwaway test wizard. Do not ship it as the product.
