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

**Quote.** Use the local xpub/watch if present (never sent). Price: **Yadio → CoinGecko → CoinMarketCap**, or a manual BTC price both sides confirm. GUI drafts `Quote` (all partidas, because the protocol requires it) but the banner only highlights boleta + P1 sats. Each side signs (`sign_quote`); both signatures required before `set_quote`. Wired as `NetMessage::Quote` over Tor, same as offer/accept.

**Funding.** Paste Signet outpoints (txid:vout, sats, address, change) for **both** sides — watch-only cannot spend. **Armar PSBT** calls `hbp-bitcoin` `build_funding_psbt` for **bond + P1 only** (no `--partida-only`, no MAD). Each laptop signs its input in Sparrow / Electrum. **Combinar** (`combine_psbts` + `extract_signed_funding_tx`) then **Verificar fondeo** (`validate_funding_tx` + `note_bond_funding` + `note_partida_funding`). Coins / unsigned PSBT / signed PSBT / tx hex can also travel as `NetMessage::Artifact`.

**Reception.** When P1 is locked: dest address + MuSig2 file/network rounds (`coop-propose` / `coop-sign` / `coop-finish`). Finish marks P1 `Paid` and unlocks the P2 row. P2 funding itself is **not** on this screen (CLI `--partida-only`).

Persisted under `{obra}/pay/` (`state.json`, `02-quote.json`, `coins.json`, `08-coop.json`, `nonces.json`, `session.json`).

## What this slice verified vs two Signet laptops

**Covered by `cargo test --workspace`:** quote both-sign + lock; Spanish stages; P2 stays blocked until P1 is terminal; `build_funding_psbt` + `validate_funding_tx` + `note_bond_funding`/`note_partida_funding` on a constructed P1 tx; fee-burn bond and partida share one MuSig2 script (verify matches by quoted amounts); existing bitcoin tests (`quote_signatures_roundtrip`, `funding_partida_only_mandante_pays_fee`, `cannot_fund_segunda_before_primera`).

**Needs two Signet laptops (not claimed here):** Tor delivery of Quote/Artifact, Sparrow/Electrum signing the real PSBT, broadcasting the funding tx, live Esplora scan of the watch-only xpub, and a live MuSig2 close against a mined P1 UTXO.

## Retest (two obras, same mandante)

Both online. No Avanzado for catalog.

1. Mandante: Felipe → crear **casa2** → Preparar → Firmar → Red **Conectarme** / Publicar.
2. Mandante: ＋ Nueva obra **casa3** → Preparar → Firmar → Publicar. Do **not** expect the contratista to jump to Aceptar.
3. Contratista: Buscar **`Felipe`**. Cards: **casa2** and **casa3**. No trato auto-opened.
4. **Ver propuesta** on casa2 → Trato review (total, plazos, partidas) → **Aceptar trato**.
5. Buscar again (or same list) → **Ver propuesta** on casa3 → review → accept.
6. After casa2 is signed: **Pago** enables. Status “acordar cotización…” then, once both signed the quote, “Ahora: fondear boleta + partida 1”. Partida 2 stays grey.

## Out of this slice

Full fee-burn arming wizard, MAD, all 10 partidas at once, mainnet, Android, P2 `--partida-only` GUI, Esplora auto-scan in the window.
