# UI flow — personas, obras, tratos

Approved product map (2026-09-05). Step 1 is identity + chrome + next action. **No PSBT.**

## Words

| Word | Meaning |
|---|---|
| **Persona** | Display name (e.g. “Felipe”). Header switcher. **Not** a folder slug. |
| **Obra** | Job the mandante creates (`casa2`). Only the mandante creates obras. |
| **Trato** | Contratista picks one obra, requests the proposal, reviews it, then accepts. |
| **Propuesta** | Signed offer (`00-offer.json` / `NetMessage::Offer`). |

## Shell (Electrum-like)

One top row: **profile/obra switcher** (`Felipe · Mandante  ·  casa2`) plus network dot and theme. Tabs, not a long scroll:

| Mandante | Contratista |
|---|---|
| **Obra** — monto, plazos, partidas; one green CTA (Preparar / Firmar) | **Buscar** — persona name + **obra cards** |
| **Red** — Conectarme / Publicar / status | **Trato** — review (total, plazos, partidas) then Aceptar |
| **Trato** — Enviar only after they asked for that obra | **Red** — Conectarme / Avanzado |
| **Pago** — grey, “próximamente” | **Pago** — grey stub |

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

## Retest (two obras, same mandante)

Both online. No Avanzado. No PSBT.

1. Mandante: Felipe → crear **casa2** → Preparar → Firmar → Red **Conectarme** / Publicar.
2. Mandante: ＋ Nueva obra **casa3** → Preparar → Firmar → Publicar. Do **not** expect the contratista to jump to Aceptar.
3. Contratista: Buscar **`Felipe`**. Cards: **casa2** and **casa3**. No trato auto-opened.
4. **Ver propuesta** on casa2 → Trato review (total, plazos, partidas) → **Aceptar trato**.
5. Buscar again (or same list) → **Ver propuesta** on casa3 → review → accept.

If a second publish lands as Aceptar-only with no list/review, this slice is wrong.

## Out of step 1

PSBT / funding wizard, fee-burn arming, Tor rewrite, several mandantes in one directory.
