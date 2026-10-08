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
| Backups | One encrypted full backup (`.kbak`): seed + restore height, the whole profile (jobs, offers, withdrawals, name/role, theme/language), every FROST share and `daemon.url`. Restore is all-or-nothing and restarts the app. 0.2.7 files (seed / share / job JSON) still import under **Advanced**. See below. |
| Unlock gate | A stage funding is spendable at *funding block + 10*. Until then **Avisar que terminé** / **Aceptar X% y pagar** are off and both apps show a block countdown (`caja::traba_partida`). |
| Leaving a job | Archive hides a joint job on this device only (no funds moved, other side not cut off). Leave stage cancels local funding/proposal only. |
| Removing an offer | The client can withdraw an offer nobody took. A signed withdrawal is gossiped under its own DHT key, so peers drop the offer and gossip cannot bring it back. |
| Actions per stage | `caja::acciones_partida` decides which buttons a stage shows. Desktop and Android both use it, so a payment in flight never offers **Accept and pay** again. |

The default daemon is `https://stagenet.xmr.kernal.eu:38089`. Oxide is vendored under `third_party/monero-oxide` with the CLSAG patches this crate needs.

`cargo test --workspace` does not talk to that daemon.

## How Monero pays a stage

One personal wallet per machine, then one shared box per job, then one transaction in and one transaction out. The box is an ordinary stagenet address. Its spend key exists only as two FROST shares.

**1. Personal wallet.** Cuenta or Billetera runs **Crear billetera de stagenet**. The address to fund is on Billetera. A fresh wallet scans from 40 blocks back; a restored seed scans from the block height stored in its backup; **Mirar 200 bloques más atrás** walks further. Spent outputs are pruned by key image, so the balance matches after a restore. The mempool does not count. Outputs stay locked for about 10 blocks. The other PC creates its own seed. Do not copy `semilla.txt`.

**2. One 2-of-2 box when the two match.** The client is FROST index 1 and pays the funding fee. The contractor is index 2. The DKG context includes the job id. Neither side holds the full spend scalar. The shared view is sent once and stored in the share file.

**3. Funding one stage.** **Confirmar y fondear** builds one transaction whose output is `2 × guarantee` to the box. Both personal wallets contribute. Encerrada is set when the scan sees that transaction in a block. If the node rejects the tx, **Empezar el fondeo de nuevo** clears the stuck session and picks fresh outputs.

**4. Paying the stage.** The pot is `2 × guarantee`. The contractor receives the agreed percent of the payment plus their own guarantee. The client receives the rest. The fee comes from the client's remainder first. At 100% the contractor receives the pot minus the fee. The client's output is still there (Monero needs two outputs), but with 0 XMR: up to 0.2.7 it carried 1 piconero of dust. The same applies to personal sends: an exact send or **Usar el máximo** (free balance minus the fee) leaves a 0-amount change output.

**5. Unlock gate.** Monero only spends an output 10 blocks after the block that holds it. The box learns the funding block from its scan; `caja::traba_partida` (funding block + 10 vs the node tip) decides whether **Avisar que terminé** and **Aceptar X% y pagar** are enabled. While locked both apps show “Podés marcarla terminada en ~N bloques (~M min, bloque X)”, or “esperando que el fondeo entre en un bloque” if the funding is not in a block yet. The tip is refreshed every minute, so the gate opens without a restart.

Peers on 0.2.7 still build the old 1-piconero split and cannot co-sign a 0.2.8 payment proposal (they reject the 0-amount split); 0.2.8 still co-signs the old split. Upgrade both sides.

Both shares have to sign. One share is not a transaction. 1 domain unit = 0.00002 XMR, so a guarantee of 2000 is 0.04 XMR per side.

Atomic spending and the multisig/FROST box path stay separate in the code; do not mix them.

## Personal seed (view words)

**Billetera → Respaldos y recuperación → Ver las 25 palabras** shows the personal single-sig mnemonic after a confirmation. The words are the standard Monero English seed (same derivation as Feather / monero-wallet-cli: spend from the 25 words, view = keccak(spend)). Job boxes are **not** recovered from this seed — they need the FROST shares in the full `.kbak`. The restore height is shown next to the words. Copy is allowed; the clipboard is cleared after 60 s if it still holds the seed (on Android the clip is marked `EXTRA_IS_SENSITIVE`). Android sets `FLAG_SECURE` while the words are on screen. The same block also shows the personal address and private view key for a view-only check without exposing the seed.

## Full backup

**Billetera → Respaldos y recuperación → Exportar respaldo completo** writes one file, `konstruado-respaldo-<YYYY-MM-DD>.kbak`:

| Bytes | Field |
|---|---|
| 8 | magic `KSTRBAK\0` |
| 1 | format version (1) |
| 1 | KDF id (1 = Argon2id v1.3) |
| 4 + 4 + 1 | memory KiB, passes, lanes (default 64 MiB, 3, 1) |
| 16 | salt |
| 24 | XChaCha20-Poly1305 nonce |
| rest | ciphertext of a JSON document; the 59-byte header is the AAD |

The JSON holds the full `estado.json` profile, the seed backup text and its height, each job's share (`xmr::ShareBackup`) with the box's scan start, and the node URL. Password: at least 8 characters, nothing stored. Code: `xmr_joint::sobre` (envelope) and `crates/konstruado/src/respaldo.rs` (contents, restore), shared by desktop and Android.

Restore (welcome screen **Restaurar desde respaldo**, or the same section in Billetera): decrypt, check the seed, check every share against its job and role (same checks as a single share import), show a summary. If the device already has an account, seed or shares, an explicit **Reemplazar lo de este equipo** is required. Then everything is written to `restaurar.tmp/`, renamed to `restaurar.listo/`, and the app restarts; at startup the old files move to `previo-<date>/` and the new ones take their place (resumable if cut). The wallet and each box scan from the stored height. Newer deal progress comes back from the other person through the room.

The app shows when the last full backup was made and reminds you to export again after a new job or a new box (`ultimo-respaldo.json`).

## Withdrawing an offer

Before 0.2.6, **Quitar oferta** only deleted the offer from the local DHT store. The board key merges by union, so the next gossip from the contractor or the room put it back.

Now each offer carries `retiro_hash = SHA-256(secret)`. The secret is derived from the client's X25519 secret and the offer id, so the same device can rebuild it after a restart. Withdrawing publishes a `RetiroOferta` that reveals the secret under the DHT key `clave_retiradas()`. Every peer checks the reveal against the hash and drops the offer from its board, from its store and from what it gossips. A forged withdrawal does not match the hash and is ignored.

Offers published by 0.2.5 have no hash. For those, a withdrawal signed by the same client id is accepted. Withdrawals are capped at 512 and kept newest first. `estado.json` gains a `retiradas` list with a serde default, so older files still load. Older peers store the unknown key unchanged and keep relaying it.

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

APKs are **not** stored in git (`*.apk` is ignored). Published builds go on [Releases](https://github.com/felipebrunet/konstruado/releases).

## Version and releases

The version lives once, in `[workspace.package] version` of the root `Cargo.toml`. The desktop window title and **Help → About**, `konstruado-ffi` (Android About) and the APK `versionName` all read it. Bump it there, then tag `vX.Y.Z`.

Each release on [Releases](https://github.com/felipebrunet/konstruado/releases) carries two assets: `konstruado-X.Y.Z-android-arm64-debug.apk` and `konstruado-X.Y.Z-linux-x86_64`.

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
