# Konstruado

For a new Grok session, read `CONTEXTO.md` first.

Peer-to-peer construction escrow on Monero **stagenet**. One monorepo:

| Path | What it is |
|---|---|
| `crates/konstruado` | Desktop app (Dioxus). Clone and `cargo run`. |
| `crates/konstruado-ffi` | UniFFI facade: same caja/persist/i18n motor for Android. |
| `android/` | Jetpack Compose APK (Spanish UI). Tor via **Orbot** (SOCKS), not bundled. |
| `crates/konstruado-core`, `konstruado-net`, `xmr-joint` | Shared deal logic, Tor rendezvous, stagenet wallet / 2-of-2 box. |

The deal, the meeting room, and a stagenet wallet live in the desktop window (and the same flow on the phone). Deal and money logic is shared Rust (`konstruado-core`, `konstruado-net`, `xmr-joint`, `crates/konstruado/src/caja.rs`); desktop and Android differ only in UI, Tor (bundled `tor` vs Orbot) and who hosts the room.

The two people do not see each other like a chat. Roles:

1. **Mandante** (pays) publishes a job: name, work amount, suggested guarantee.
2. **Contratista** (builds) sees that offer on the board and accepts, or proposes another guarantee.

Guarantee must divide the job amount exactly: 10 000 / 2 000 → 5 stages. In each stage both sides lock the same amount.

Rendezvous is hardcoded (`konstruado-red-1` plus a baked Tor v3 onion). Each desktop node starts its own `tor` process, publishes a personal hidden service, and also hosts/dials that shared onion so two machines meet without exchanging addresses. Two copies on one PC still find each other on port 17432 without waiting for Tor. A phone joins in *celular* mode: outbound live session to the room through Orbot; it does not host the onion.

## Status

| Piece | Where it stands |
|---|---|
| Deal, board, Tor rendezvous, two data dirs | In the desktop window. |
| Android Compose + Orbot | Same deal/caja flow via UniFFI. Needs a desktop (or `konstruado-sala`) hosting the room. |
| Notes and extra text between the two parties | Sealed. A third person on the swarm can see the box, not the words. A live job leaves their board. |
| Personal stagenet wallet | **Billetera** in the top bar, or the account screen. Create it there. The 25-word seed is `xmr/semilla.txt` (mode 0600), not `estado.json`. |
| 2-of-2 box | Built when the job is agreed. Messages go to the other person, not the DHT. Each side keeps `xmr/{obra}.share`. |
| Stage funding and payout | The buttons build the transaction. Encerrada and Pagada flip only after a local scan sees it in a block. A live publish can still be rejected by the node. |
| Custom stagenet daemon | Optional URL (LAN / Tailscale) for scan, balance, funding and payout. **Use default** falls back to the public HTTPS daemon. |
| Backups | Seed (with block height), per-job share, and job profile (obras/ofertas JSON) are three separate files. Reinstall order: job backup → seed → share. |
| Leaving a job | Archive hides a joint job on this device only (no funds moved, other side not cut off). Leave stage cancels local funding/proposal only. |

The default daemon is `https://stagenet.xmr.kernal.eu:38089`. Oxide is vendored under `third_party/monero-oxide` with the CLSAG patches this crate needs.

`cargo test --workspace` does not talk to that daemon.

## How Monero pays a stage

One personal wallet per machine, then one shared box per job, then one transaction in and one transaction out. The box is an ordinary stagenet address. Its spend key exists only as two FROST shares.

**1. Personal wallet.** Cuenta or Billetera runs **Crear billetera de stagenet**. The address to fund is on Billetera. A fresh wallet scans from 40 blocks back; a restored seed scans from the block height stored in its backup; **Mirar 200 bloques más atrás** walks further. Spent outputs are pruned by key image, so the balance matches after a restore. The mempool does not count. Outputs stay locked for about 10 blocks. The other PC creates its own seed. Do not copy `semilla.txt`.

**2. One 2-of-2 box when the two match.** The client is FROST index 1 and pays the funding fee. The contractor is index 2. The DKG context includes the job id. Neither side holds the full spend scalar. The shared view is sent once and stored in the share file.

**3. Funding one stage.** **Confirmar y fondear** builds one transaction whose output is `2 × guarantee` to the box. Both personal wallets contribute. Encerrada is set when the scan sees that transaction in a block. If the node rejects the tx, **Empezar el fondeo de nuevo** clears the stuck session and picks fresh outputs.

**4. Paying the stage.** The pot is `2 × guarantee`. The contractor receives the agreed percent of the payment plus their own guarantee. The client receives the rest. The fee comes from the client's remainder first. At 100% the contractor receives the pot minus the fee.

Both shares have to sign. One share is not a transaction. 1 domain unit = 0.00002 XMR, so a guarantee of 2000 is 0.04 XMR per side.

Atomic spending and the multisig/FROST box path stay separate in the code; do not mix them.

## Build desktop from source

Needs Rust 1.89 or newer (`monero-wallet` 0.2), GTK3, WebKitGTK 4.1, `libxdo-dev`, and the `tor` package.

```bash
sudo apt install tor libgtk-3-0 libwebkit2gtk-4.1-0 libxdo-dev
cargo test --workspace
cargo run
```

The UI is Spanish by default. Switch to English with **ES / EN** in the top bar (or in the account screen). The deal itself does not change.

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
cd android && ./build-apk.sh
```

APKs are **not** stored in git (`*.apk` is ignored). Published builds go on [Releases](https://github.com/felipebrunet/home_builder_pay/releases).

## Version and releases

The version lives once, in `[workspace.package] version` of the root `Cargo.toml`. The desktop window title and **Help → About**, `konstruado-ffi` (Android About) and the APK `versionName` all read it. Bump it there, then tag `vX.Y.Z`.

Each release on [Releases](https://github.com/felipebrunet/home_builder_pay/releases) carries two assets: `konstruado-X.Y.Z-android-arm64-debug.apk` and `konstruado-X.Y.Z-linux-x86_64`.

## Linux binary (not production)

On Debian/Ubuntu:

```bash
sudo apt install tor libgtk-3-0 libwebkit2gtk-4.1-0 libxdo3
chmod +x konstruado-*-linux-x86_64*
./konstruado-*-linux-x86_64*
```

To rebuild it:

```bash
cargo build -p konstruado --release
VERSION=$(cargo pkgid -p konstruado | sed 's/.*[#@]//')
mkdir -p dist
cp target/release/konstruado "dist/konstruado-$VERSION-linux-x86_64"
```

How to use the app (roles, two machines, the deal) is **Help → README** inside the window, not this file.
