# Code review (hbp-app GUI)

Short notes after the overnight extract. Signet product, Spanish UI, Windows `hbp-app` first.

## What is fine as-is

- **`hbp-core` / `hbp-bitcoin` / `hbp-net` size.** Escrow + MuSig2 + fee-burn + Tor/DHT + Esplora belong in those crates. ~crate line counts are reasonable for that surface.
- **Pay state machine in `pay.rs`.** Handshake steps, coop merge/nonce reuse, delete/lane/split helpers, and tests live here. Keep protocol out of the window.
- **`work.rs` store.** Folder-per-obra + `delete_work` is the right persistence boundary.

## What this slice refactored

Extracted window chrome from the `main.rs` monolith into the lib (bin still owns jobs/inbox/tabs):

| File | Role |
|---|---|
| `crates/hbp-app/src/theme.rs` | Light/dark accents, panel/edit colors |
| `crates/hbp-app/src/widgets.rs` | `primary_btn`, `panel_card`, fields, `apply_theme`, **obra stepper** + pot badges |
| `crates/hbp-app/src/pay.rs` | Delete gates, lane, `%` split, funding “fully signed → Publicar” |
| `crates/hbp-app/src/flow.rs` | Both roles: **Obra \| Red \| Trato \| Pago** |
| `crates/hbp-app/src/work.rs` | `delete_work` wipes slug folder + index |

Killed Electrum-as-only-broadcast copy on the funding path. Publish cards stay two (funding vs coop) but share the same Esplora POST + Copiar/Exportar/Comprobar pattern.

## What remains in `main.rs`

Still a large bin (~jobs, Tor inbox, tab bodies, Pago wiring). Incremental next splits, **not** a rewrite:

- `tabs/` — `show_tab_obra` / `red` / `trato` / `buscar` as `App` methods or free functions taking a context
- `pay/` UI — fund handshake + coop/stop wizard (logic already in `pay.rs`)
- Inbox/`JobEvent` loop stays next to the window until a thin `jobs.rs` is justified

Do not move bitcoin/Tor protocol into the GUI modules.

## Tests

`cargo test --workspace` is the gate. Two live Signet laptops are still required for Esplora POST + Tor handshake.
