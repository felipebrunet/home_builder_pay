# Escenarios E2E en regtest

Catálogo exhaustivo (142, con ticks PASS/NO TEST): [SCENARIOS.md](SCENARIOS.md). Este archivo es el índice **unwind** minado. MAD/árbitro: `scripts/regtest_catalog.sh`.

Todos usan la misma obra de demo: **60 000 USD**, 2 partidas de **30 000**, boleta **20 000**, 100 000 USD/BTC. Plazos de obra en **segundos**.

| # | Escenario | On-chain | Doc | Script |
|---|---|---|---|---|
| 1 | Camino feliz (2 partidas cobradas) | MuSig2 × 3 (P1, P2, boleta) | [REGTEST_HAPPY_PATH.md](REGTEST_HAPPY_PATH.md) | `scripts/regtest_happy_path.sh` |
| 2 | Contratista no trabaja y se va **antes** de la partida 2 | unwind P1 → mandante; boleta → contratista; P2 no existe | [REGTEST_CONTRACTOR_ABANDONS.md](REGTEST_CONTRACTOR_ABANDONS.md) | `scripts/regtest_contractor_abandons.sh` |
| 3 | Obra hecha y el mandante **no firma** la recepción | **Igual que #2 en la cadena.** Bitcoin no ve el muro. Off-chain el contratista pierde el trabajo hundido. | esta tabla | el de #2 |
| 4 | Cancelación **cooperativa** (antes de T) | MuSig2 reembolsa P1 al mandante + boleta al contratista; witness 64 B; locktime 0 | [REGTEST_COOP_CANCEL.md](REGTEST_COOP_CANCEL.md) | `scripts/regtest_coop_cancel.sh` |
| 5 | Partida 1 **cobrada**, el mandante no sigue | MuSig2 P1 → contratista; P2 no se fondea; unwind boleta → contratista | [REGTEST_STOPS_AFTER_PARTIDA1.md](REGTEST_STOPS_AFTER_PARTIDA1.md) | `scripts/regtest_stops_after_partida1.sh` |
| 6 | P1 ok, P2 aceptada al **80 %** (p.ej. pintura) | MuSig2 split 0,24 / 0,06; boleta se libera | [REGTEST_SPLIT_80.md](REGTEST_SPLIT_80.md) | `scripts/regtest_split_80.sh` |
| 7 | Contratista **nunca pone boleta** | no hay 2-de-2 válido; si el mandante paga igual, unwind tras T | [REGTEST_NO_BOND.md](REGTEST_NO_BOND.md) | `scripts/regtest_no_bond.sh` |
| 8 | Mandante cancela el resto **con acuerdo** tras P1 cobrada | MuSig2 suelta la boleta ahora (no timeout) | [REGTEST_CANCEL_AFTER_P1.md](REGTEST_CANCEL_AFTER_P1.md) | `scripts/regtest_cancel_after_p1.sh` |
| 9 | Ambos se enojan y **nadie firma** | igual que #2: cada uno barre lo suyo tras T (unwind). **No hay MAD** hoy | esta sección | el de #2 |

Robos que el CLI rechaza (minados como chequeo en #2 y #4):

- mandante `unwind --kind bond` → *timeout is not a bank boleta*
- contratista `unwind --kind partida` → *only the mandante can unwind a partida*

Política de disputa (offeror): [DISPUTE.md](DISPUTE.md). Los E2E de esta tabla usan el default `unwind`.

## Si nadie firma: unwind, no MAD (salvo cláusula `mad`)

Hoy, si el pozo está locked y ambos se niegan a cooperar:

- tras `T_partida` el mandante recupera **el pago** de esa partida
- tras `T_proyecto` el contratista recupera **la boleta**
- nadie se queda con la plata del otro; nadie quema

Eso **no** presiona a negociar un 80 %: el mandante puede esperar y llevarse el 100 % de la partida (y el muro, si ya está en su terreno). Por eso el contratista no debería adelantar obra grande sin recepción.

**MAD** es otra cláusula, opcional (`hbp new --dispute mad`). El **código** está (tercera salida, hoja NUMS). **No hay E2E minado todavía.** Reglas ya cerradas:

- **No** a una wallet del autor del software. Eso convierte el programa en un negocio de escrow, un imán legal y un punto de confianza que el 2-de-2 quería evitar.
- Stake chico y **simétrico** (bps de la partida 1, cada parte). Tras T, si nadie coopera, quema NUMS. No se queman los 20k de boleta.

El 80 % de la pintura sigue siendo mejor que MAD: las dos firmas, split, listo. MAD es el palo para cuando **no** hay split.
