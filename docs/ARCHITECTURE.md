# Architecture

Back to the [README](../README.md). Money flow, backups and offer withdrawal are in [PROTOCOL.md](PROTOCOL.md); building in [BUILDING.md](BUILDING.md).

Working notes for AI-assisted sessions (Spanish) are in [`CONTEXTO.md`](../CONTEXTO.md).

Peer-to-peer construction escrow on Monero **stagenet**. One monorepo:

| Path | What it is |
|---|---|
| `crates/konstruado` | Desktop app (Dioxus). Clone and `cargo run`. |
| `crates/konstruado-ffi` | UniFFI facade: same caja/persist/i18n motor for Android. |
| `crates/konstruado-motor` | Shared motor: box (`caja`), USD quotes (`cotizacion`), ES/EN texts (`i18n`), profile (`persist`), full backup (`respaldo`). |
| `android/` | Jetpack Compose APK (Spanish / English UI). Tor via **Orbot** (SOCKS), not bundled. |
| `crates/konstruado-core`, `konstruado-net`, `xmr-joint` | Shared deal logic, Tor rendezvous, stagenet wallet / 2-of-2 box. |

The deal, the meeting room, and a stagenet wallet live in the desktop window (and the same flow on the phone). Deal and money logic is shared Rust (`konstruado-core`, `konstruado-net`, `xmr-joint`, `crates/konstruado-motor`); desktop and Android differ only in UI, Tor (bundled `tor` vs Orbot) and who hosts the room.

The two people do not see each other like a chat. Roles:

1. **Mandante** (pays) publishes a job: name, work amount, suggested guarantee.
2. **Contratista** (builds) sees that offer on the board and accepts, or proposes another guarantee.

Guarantee must divide the job amount exactly: 10 000 / 2 000 → 5 stages. In each stage both sides lock the same amount.

Rendezvous is hardcoded (`konstruado-red-1` plus a baked Tor v3 onion). Each desktop node starts its own `tor` process, publishes a personal hidden service, and also hosts/dials that shared onion so two machines meet without exchanging addresses. Two copies on one PC still find each other on port 17432 without waiting for Tor. A phone joins in *celular* mode: outbound live session to the room through Orbot; it does not host the onion.


## Status of each piece

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

## Language (ES / EN)

Both apps are Spanish or English, switchable at any time; the deal itself does not change.

- **Desktop:** **ES / EN** in the top bar or in the account screen. Texts are pairs in place, `Idioma::t(es, en)` (`konstruado-motor/src/i18n.rs`); motor functions take `es: bool`.
- **Android:** **Cuenta → Idioma / Language**. Compose texts are `tr(es, en)` pairs (`ui/I18n.kt`); texts that come from Rust through UniFFI use the same pairs (`tr` / `tf!` in `konstruado-ffi`, a process-wide language flag). The change applies immediately, no restart.
- **Profile:** the choice is stored in `estado.json` (`idioma`, plus `idioma_fijo` once a person picked one). On a phone that never chose, the app follows the device language if it is Spanish or English, otherwise Spanish. A full backup carries the language.
- **No missing translations:** a pair cannot exist without its English half. Tests (`I18nTest` on Android, `textos_del_motor_tienen_ingles` in `konstruado-ffi`) fail if a Spanish user-facing literal is left outside `tr` / `tf!`, or if a pair has an empty side or different `$`/`{}` placeholders.

## Source link

Desktop (**Cuenta → Source** and **Help → About**) and Android (**Cuenta → About**) show **Source on GitHub** with the Octicons `mark-github` mark (`assets/icon/mark-github.svg`, MIT, drawn in the current text colour so it follows the theme). The URL comes from `CARGO_PKG_REPOSITORY` and opens in the system browser.
