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
| No quote yet | Ahora: acordar cotización de boleta + partida 1. |
| Quote waiting a signature | Ahora: firmar la cotización (boleta + partida 1). |
| Quote locked, unfunded | **Ahora: fondear boleta + partida 1** |
| Bond + P1 noted | **Partida 1 en curso** |
| P1 Paid / Unwound / T2 | Partida 1 cerrada. Ahora puedes cotizar / fondear partida 2. |

**Quote.** FX is **contract unit per BTC** (`CLP/BTC`, `USD/BTC`, …), never a leftover USD rate on a CLP trato. Yadio → CoinGecko → CoinMarketCap (pair for that unit). SATS/BTC skip FX. Amounts stay 1/100 of the unit. A signed-but-unfunded quote can **Recotizar**. GUI drafts `Quote` (all partidas) but the banner only highlights boleta + P1 sats. Both signatures required before `set_quote`. Wired as `NetMessage::Quote` over Tor.

**Funding (PSBT handshake).** Primary path is watch-only xpub + Esplora (blockstream → mempool Signet), not paste-both-outpoints. One green button per stage. Prior steps lock (no competing “Armar de nuevo”); **Empezar de nuevo** is behind Avanzado with confirm. If Tor deliver fails, the PSBT stays local and the CTA is **Reenviar**.

1. **Mis monedas** — `Buscar mis monedas` scans the local xpub. App auto-selects the smallest confirmed UTXO that covers share + fee; user can pick another. After a partial exists the picker hides (only “Usando …”).
2. **Armar/enviar parcial** — either side builds a 1-input PSBT (their coin + change) that already has exact **boleta + P1** outputs. Wired as `Artifact` `06-funding.partial.json`.
3. **Completar** — peer picks their UTXO the same way, adds the second input, sends back the complete unsigned 2-in PSBT (`06-funding.unsigned.json`). Incoming older partials cannot overwrite a complete PSBT.
4. **Firmar** — **Exportar archivo** (`.psbt`) and **Copiar / ver texto** (base64). Sign in Electrum/Sparrow. **Importar archivo** or paste the **1-signature** PSBT; app sends `07-onesig.psbt.json` (Reenviar if that fails). Peer exports that 1-sig (file + text), signs the second input in Electrum, **broadcasts there**.
5. **En cadena** — **Comprobar transacción** (plus background Esplora poll). Status in Spanish with txid (mempool / confirmada). Manual tx-hex verify stays as rescue.

Boleta and partida 1 are **distinct** Taproot addresses (fee-burn key-path + unique tagged merkle tweak per output id). Later partidas stay grey until P1 is terminal.

**Reception.** When P1 is locked: dest address + MuSig2 file/network rounds (`coop-propose` / `coop-sign` / `coop-finish`). Finish marks P1 `Paid` and unlocks the P2 row. P2 funding itself is **not** on this screen (CLI `--partida-only`).

Persisted under `{obra}/pay/` (`state.json`, `02-quote.json`, `coins.json`, `08-coop.json`, `nonces.json`, `session.json`).

## What this slice verified vs two Signet laptops

**Covered by `cargo test --workspace`:** quote both-sign + lock; Spanish stages; P2 stays blocked until P1 is terminal; boleta ≠ P1 ≠ P2 Taproot addresses; partial→complete PSBT balances and verifies; constructed P1 fund/verify; complete mark never reopens “Armar”; prefer-PSBT never downgrades; chain status Spanish; Esplora JSON helpers; existing bitcoin tests (`quote_signatures_roundtrip`, `funding_partida_only_mandante_pays_fee`, `cannot_fund_segunda_before_primera`).

**Needs two live Signet wallets (not claimed here):** Esplora listing real xpub UTXOs, Tor partial-PSBT exchange, Electrum/Sparrow signing + broadcast, chain detect on a mined funding tx, live MuSig2 close.

## Retest (two obras, same mandante)

Both online. No Avanzado for catalog.

1. Mandante: Felipe → crear **casa2** → Preparar → Firmar → Red **Conectarme** / Publicar.
2. Mandante: ＋ Nueva obra **casa3** → Preparar → Firmar → Publicar. Do **not** expect the contratista to jump to Aceptar.
3. Contratista: Buscar **`Felipe`**. Cards: **casa2** and **casa3**. No trato auto-opened.
4. **Ver propuesta** on casa2 → Trato review (total, plazos, partidas) → **Aceptar trato**.
5. Buscar again (or same list) → **Ver propuesta** on casa3 → review → accept.
6. After casa2 is signed: **Pago** enables. Status “acordar cotización…” then, once both signed the quote, “Ahora: fondear boleta + partida 1”. Partida 2 stays grey.

## Out of this slice

Full fee-burn arming wizard, MAD, all 10 partidas at once, mainnet, Android, P2 `--partida-only` GUI.
