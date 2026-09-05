# UI flow — personas, obras, tratos, pago

Approved product map (2026-09-05). Signet only. No arbiter. Fee-burn if no agreement.

## Words

| Word | Meaning |
|---|---|
| **Persona** | Display name (e.g. “Felipe”). Header switcher. **Not** a folder slug. |
| **Obra** | Job the mandante creates (`casa2`). Only the mandante creates obras. |
| **Trato** | Contratista picks one obra, requests the proposal, reviews it, then accepts. |
| **Propuesta** | Signed offer (`00-offer.json` / `NetMessage::Offer`). |
| **Pago** | After the trato is closed: quote, fund **boleta + partida 1**, then coop-close. |

## Shell (Electrum-like)

One top row: **profile/obra switcher** (`Felipe · Mandante  ·  casa2`) plus network dot and theme. Tabs, not a long scroll:

| Mandante | Contratista |
|---|---|
| **Obra** — monto, plazos, partidas; one green CTA (Preparar / Firmar) | **Buscar** — persona name + **obra cards** |
| **Red** — Conectarme / Publicar / status | **Trato** — review (total, plazos, partidas) then Aceptar |
| **Trato** — Enviar only after they asked for that obra | **Red** — Conectarme / Avanzado |
| **Pago** — enabled once the trato is signed | **Pago** — enabled once the trato is signed |

Finished steps are muted checks. Waiting cards are amber; success green; primary actions green. Help is behind **¿Qué es esto?**. **Notas** stay collapsed (Ver / Limpiar).

**Cambiar perfil** lives in the switcher, not a second header row.

## Discovery (persona is primary)

Publish writes the same announce JSON to:

1. literal **`hbpn-felipe`** (normalized persona)
2. hashed `hbp` + SHA-256(`hbp-rendezvous-v1:felipe`)[:12]
3. literal **`hbpn-casa2`** + hashed obra topic
4. directory **`hbpn-dir-v1`** (scan `person_name` / `work_name` in the payload)

DHT keys stay `hbp-person:felipe` (primary) and `hbp-work:casa2` (secondary). Isolated onions hit the board **before** remote DHT. Buscar `Felipe` must return the casa2 announce without typing the obra title.

**Catalog handshake (not auto-accept):**

1. Mandante **Publicar** = catalog only. No offer on the wire.
2. Contratista **Buscar** `Felipe` → list of that person’s published obras (cards). Does **not** open a trato or send Accept.
3. Contratista picks one → **Ver propuesta** DELIVERs `Request { work_name, onion }` (+ `Hello` for the dest).
4. Mandante inbox `Request` binds the peer **to that obra** and DELIVERs `Offer`.
5. Contratista **Trato** shows the full proposal (total, moneda, boleta 10%, partidas, t1/t2). Then **Aceptar trato** → `Accept` → mandante `Commit`.

Mandante **Enviar** is a resend after a request, not a dump into Accept. A second obra stays in the catalog until they ask for it.

## Pago (partida 1 only)

Hard rule: **only one partida at a time**. First funding is atomic **boleta + partida 1**. Partida 2 (and its payments) stay grey until partida 1 is terminal (`Paid` / `Unwound` / `FeeBurnT2`). The state machine already rejects `note_partida_funding(2)` while P1 is open; the Pago tab greys later rows the same way.

After **Trato cerrado**, the next-step card is **Ir a Pago**. Status lines:

| Stage | Spanish |
|---|---|
| No quote yet | Ahora: acordar la plata de la boleta y de la partida 1. |
| Quote waiting a signature | Ahora: firmar la plata de la boleta y de la partida 1. |
| Quote locked, unfunded | **Ahora: juntar la plata de la boleta y de la partida 1** |
| Bond + P1 noted | **Partida 1 en curso** |
| P1 Paid / Unwound / T2 | Partida 1 cerrada. Ahora puedes ver la partida 2. |

**Pago layout.** Full-width **vertical stack** (Electrum density): “qué hacer ahora” + one large primary action, then the CLP summary under it. Cards use the panel width; they must not collapse into a top ribbon. Finished stages collapse. After **Confirmada en Signet**, PSBT export/import/reenviar/empezar de nuevo leave the main path (Avanzado rescue only). Primary then is recepción/cierre or “Partida 1 en curso”.

**Quote.** Main numbers stay in the **contract currency** (CLP on a CLP trato). Sats are small print or hidden. FX is the **quoted snapshot** (“tipo de cambio acordado: … CLP/BTC”), not a replacement for the partida list. When the quote arrives/locks, partida rows show the completed CLP amounts. Pair is **contract unit per BTC** (`CLP/BTC`), never a leftover USD rate. Yadio → CoinGecko → CoinMarketCap. SATS/BTC skip FX. Amounts stay 1/100 of the unit. A signed-but-unfunded quote can **Recotizar** (Avanzado). Both signatures required before `set_quote`. Wired as `NetMessage::Quote` over Tor.

**Wallet before Buscar.** Mandante saves the Signet vpub on Obra (“Billetera y respaldo”). Contratista gets the same local-only step on **Buscar**, before searching. Stored at the persona (`watch.json` at the works root) and copied into the trato folder when one exists. Never sent to the peer.

**Funding (PSBT handshake).** Primary path is watch-only vpub + Esplora, not paste-both-outpoints. Copy is obra language (“plata de la boleta”, “comisión de red (~250)”). Addresses, txid, UTXO sit behind **Detalle técnico**. One green button per stage. If Tor deliver fails, the PSBT stays local and the CTA is **Reenviar**. A second Esplora confirm of an already-noted bond is success (no red `bond already funded`).

1. **Buscar mi plata** — scans the local vpub. App auto-selects the smallest confirmed coin that covers share + fee; user can pick another (labeled Plata 1 / 2, not raw outpoints).
2. **Armar y enviar mi parte** — either side builds a 1-input PSBT that already has exact **boleta + P1** outputs. Wired as `Artifact` `06-funding.partial.json`.
3. **Completar con mi parte** — peer adds the second input, sends back the complete unsigned 2-in PSBT (`06-funding.unsigned.json`). Incoming older partials cannot overwrite a complete PSBT.
4. **Exportar para firmar** — `.psbt` file + copiar. Sign in Electrum. **Traer lo firmado**; app sends `07-onesig.psbt.json`. Peer signs the second input in Electrum and **broadcasts there**.
5. **Comprobar en la red** — Esplora poll. Status in Spanish without a txid on the primary line (`Confirmada en Signet`). Manual tx-hex stays in Avanzado.

Boleta and partida 1 are **distinct** Taproot addresses (fee-burn key-path + unique tagged merkle tweak per output id). Later partidas stay grey until P1 is terminal.

**Reception (P1).** When P1 is locked: dest + cooperative close. One green button: **Proponer cobro** → wait → **Firmar cobro** → **Marcar cobrada**. Finish marks P1 `Paid` and keeps the **signed tx on screen**. Incoming CoopFiles **merge** both sides’ parts. Each laptop **reuses** its nonce seed for that sighash (a second Proponer/Firmar does not rotate it). That was the `invalid signer index 0` / `falta nonce del mandante` bug when both proposed over Tor.

**Two publish paths (do not mix them):**

1. **Funding PSBT** (singlesig inputs from Electrum): **Exportar para firmar** → Electrum sign → peer signs the other input → **broadcast in Electrum** → **Comprobar en la red**. That path stays Electrum.
2. **Coop MuSig2** (cobro partida 1, devolución boleta): the tx is **fully signed inside the app**. Do **not** tell the user to publish in Electrum.

**Click order after Terminar / Marcar cobrada / Terminar y devolver**

Both sides must see this card (the peer gets `09-coop-tx.json` over Tor; **Enviar al otro** resends it). Status must not say “Obra detenida / Listo para publicar en Electrum” without a control.

1. Green **Publicar en Signet** — POSTs the raw hex to Blockstream Signet Esplora (`/tx`), then mempool.space Signet. Same endpoint family as the coin scan.
2. Or **Copiar texto** (tx hex) / **Exportar archivo** (`.hex` / `.txt`, fallback under `{obra}/pay/`).
3. **Comprobar en la red** (also auto-polls) until **Ya está en Signet.**
4. If the obra is stopping and that was the cobro: destino de la boleta → same three buttons for the devolución.

The hex sits **under** Publicar en Signet, not only in Avanzado.

**Detener obra y devolver boleta.** Visible on Pago (and Trato) once boleta+P1 are confirmed. Wizard: confirm → if P1 still locked, pay P1 first (including **Publicar en Signet**) → destino boleta (cuenta Signet del contratista) → Proponer devolución → Firmar devolución → Terminar y devolver → **Publicar en Signet** → Comprobar. Bond close is `kind: bond` (`08-coop-bond.json`), same merge/reuse rules. `mark_bond_released` sets the project **Closed** (if P1 was paid) or **Cancelled**; later partidas stay closed. PSBT funding stays hidden after confirm.

Persisted under `{obra}/pay/` (`state.json`, `02-quote.json`, `coins.json`, `08-coop.json`, `08-coop-bond.json`, `09-coop-tx.json` on the wire, `nonces.json`, `session.json`).

## What this slice verified vs two Signet laptops

**Covered by `cargo test --workspace`:** quote both-sign + lock; Spanish stages; P2 stays blocked until P1 is terminal; boleta ≠ P1 ≠ P2 Taproot addresses; partial→complete PSBT balances and verifies; constructed P1 fund/verify; complete mark never reopens “Armar”; prefer-PSBT never downgrades; chain status Spanish; Esplora JSON helpers; **broadcast URL is `{base}/tx`**; **both-propose then sign reuses seed and finishes with hex + wire kind**; rotating the seed is rejected; **bond coop after P1 paid closes later partidas**; `apply_finished_coop_hex` is idempotent on the peer; existing bitcoin tests (`quote_signatures_roundtrip`, `funding_partida_only_mandante_pays_fee`, `cannot_fund_segunda_before_primera`).

**Needs two live Signet wallets (not claimed here):** Esplora listing real xpub UTXOs, Tor partial-PSBT exchange, Electrum/Sparrow signing + funding broadcast, live Esplora **POST** of a coop hex, chain detect on a mined funding tx, live P1 close and bond-return close.

## Retest (two obras, same mandante)

Both online. No Avanzado for catalog.

1. Mandante: Felipe → crear **casa2** → Preparar → Firmar → Red **Conectarme** / Publicar.
2. Mandante: ＋ Nueva obra **casa3** → Preparar → Firmar → Publicar. Do **not** expect the contratista to jump to Aceptar.
3. Contratista: Buscar **`Felipe`**. Cards: **casa2** and **casa3**. No trato auto-opened.
4. **Ver propuesta** on casa2 → Trato review (total, plazos, partidas) → **Aceptar trato**.
5. Buscar again (or same list) → **Ver propuesta** on casa3 → review → accept.
6. After casa2 is signed: **Pago** enables. Status “acordar la plata…” then, once both signed the quote, “Ahora: juntar la plata de la boleta y de la partida 1”. Partida 2 stays grey. After Signet confirm, left column switches to cobro — no Armar/export on the main path.

## Out of this slice

Full fee-burn arming wizard, MAD, all 10 partidas at once, mainnet, Android, P2 `--partida-only` GUI.
