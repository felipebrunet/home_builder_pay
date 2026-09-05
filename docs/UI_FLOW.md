# UI flow — personas, obras, tratos

Approved product map (2026-09-05). Step 1 is identity + chrome + next action. **No PSBT** in this step.

## Words

| Word | Meaning |
|---|---|
| **Persona** | How a human is called. Mandante and contratista each have a display name (e.g. “Don José”). Shown in the header. **Not** a folder slug (`contratista1`). |
| **Obra** | The job the mandante creates and publishes (e.g. “Casa Norte”). Only the mandante creates obras. |
| **Trato** | The contratista engaging that obra: find → wait for proposal → accept → mandante confirms. |
| **Propuesta** | Signed offer (`00-offer.json` / `NetMessage::Offer`). |

## Who does what

**Mandante (pays)**

1. First open: “Yo pago — Mandante” → “¿Cómo te dicen?” → **Seguir**.
2. Header shows *Don José · Mandante*.
3. **Crear obra** (Casa Norte) — this is an obra, not a persona.
4. **Qué hacer ahora** drives one button: Preparar partidas → Firmar propuesta → Conectarme → Publicar obra → espera al contratista → **Enviar** (only when the peer handle is known). Finished steps stay muted with a check; Publicar is never the green CTA after the obra is already published.
5. Contratista accepts; mandante auto-confirms on the wire. Card: “Trato cerrado… el pago en cadena viene después.”

**Contratista (builds)**

1. First open: “Yo construyo — Contratista” → tu nombre.
2. Home is not “crear obra”. Big actions: **Conectarme** then **Buscar** the mandante by *their* name (Don José). Obra name still works as fallback.
3. A hit becomes a **trato** in “Mis tratos” (`Casa Norte — con Don José`), never a random folder as the identity.
4. **Qué hacer ahora**: Espera la propuesta → **Aceptar** when it arrives → espera confirmación → trato cerrado.

**Cambiar perfil** (header) is how you switch role on the same PC. Each role keeps its own name.

## Discovery (partial in step 1)

Ideal: find **mandante**, then see their published obras.

Shipped now: the primary publish/lookup key is the mandante persona (`hbp-person:{normalized}`, e.g. `felipe`); obra title (`hbp-work:casa2`) is secondary. Buscar `Felipe` must open their published obra (`casa2 — con Felipe`). Full mandante directory / several obras per person is a later step. Onion paste stays under **Avanzado**.

**Send / find handshake.** A one-sided find is not enough for **Enviar**. When the contratista finds the mandante, they DELIVER a `Hello` with their own onion (and name). The mandante inbox stores that handle on the matching obra, the card moves from “esperando contratista” to **Enviar**, and Enviar DELIVERs the offer to that onion — no Avanzado paste. The mandante replies `Hello` once so both sides have a usable dest. “Ya estamos en contacto” is reserved for that two-way handle; a one-sided find says “Encontré su señal; esperando que el mandante pueda enviarte”.

## Notes panel

Collapsed by default. **Ver / Ocultar**, height resizable when open, **Limpiar**, scroll. Repeated errors collapse to `×N`.

## Out of step 1

PSBT / funding wizard, fee-burn arming, Tor rewrite, complete mandante directory.

## What a tester clicks

See the PR / agent report: Mandante path vs Contratista path after this slice.
