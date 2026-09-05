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
4. **Qué hacer ahora** drives one button: Preparar partidas → Firmar propuesta → Conectarme → Publicar obra → **Enviar**.
5. Contratista accepts; mandante auto-confirms on the wire. Card: “Trato cerrado… el pago en cadena viene después.”

**Contratista (builds)**

1. First open: “Yo construyo — Contratista” → tu nombre.
2. Home is not “crear obra”. Big actions: **Conectarme** then **Buscar** the mandante by *their* name (Don José). Obra name still works as fallback.
3. A hit becomes a **trato** in “Mis tratos” (`Casa Norte — con Don José`), never a random folder as the identity.
4. **Qué hacer ahora**: Espera la propuesta → **Aceptar** when it arrives → espera confirmación → trato cerrado.

**Cambiar perfil** (header) is how you switch role on the same PC. Each role keeps its own name.

## Discovery (partial in step 1)

Ideal: find **mandante**, then see their published obras.

Shipped now: publish stores the obra *and* the mandante display name (DHT + ntfy topic). Contratista search box is “nombre del mandante”. Lookup tries person name, then obra name. Full mandante directory / several obras per person is a later step. Onion paste stays under **Avanzado**.

## Notes panel

Collapsed by default. **Ver / Ocultar**, height resizable when open, **Limpiar**, scroll. Repeated errors collapse to `×N`.

## Out of step 1

PSBT / funding wizard, fee-burn arming, Tor rewrite, complete mandante directory.

## What a tester clicks

See the PR / agent report: Mandante path vs Contratista path after this slice.
