# UI flow — personas, obras, tratos

Approved product map (2026-09-05). Step 1 is identity + chrome + next action. **No PSBT.**

## Words

| Word | Meaning |
|---|---|
| **Persona** | Display name (e.g. “Felipe”). Header switcher. **Not** a folder slug. |
| **Obra** | Job the mandante creates (`casa2`). Only the mandante creates obras. |
| **Trato** | Contratista engaging that obra: find → wait → accept → confirm. |
| **Propuesta** | Signed offer (`00-offer.json` / `NetMessage::Offer`). |

## Shell (Electrum-like)

One top row: **profile/obra switcher** (`Felipe · Mandante  ·  casa2`) plus network dot and theme. Tabs, not a long scroll:

| Mandante | Contratista |
|---|---|
| **Obra** — monto, plazos, partidas; one green CTA (Preparar / Firmar) | **Buscar** — persona name + tratos |
| **Red** — Conectarme / Publicar / status | **Trato** — Qué hacer ahora |
| **Trato** — Qué hacer ahora + Enviar | **Red** — Conectarme / Avanzado |
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

**Handshake:** after find, contratista DELIVERs `Hello` with their onion; mandante stores it and Enviar works without Avanzado paste.

## Retest (persona search)

Both windows online. No Avanzado onion paste.

1. **Mandante:** Yo pago → name **Felipe** → Seguir. Switcher: `Felipe · Mandante`. Tab **Obra** → crear **casa2**. Preparar → Firmar. Tab **Red** → **Conectarme** (auto-publica). Notas must mention tema `hbpn-felipe`, not only `casa2`.
2. **Contratista:** Yo construyo → nombre → Seguir. Tab **Buscar** → **Conectarme** → type **`Felipe`** (not casa2) → **Buscar**.
3. Trato `casa2 — con Felipe` appears; switcher and tab **Trato** open it.
4. Mandante **Trato** → **Enviar**. Contratista **Aceptar**.

If step 2 fails with only `casa2` working, persona board is still broken.

## Out of step 1

PSBT / funding wizard, fee-burn arming, Tor rewrite, several obras per persona.
