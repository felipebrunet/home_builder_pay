# Escenarios E2E en regtest

Todos usan la misma obra de demo: **60 000 USD**, 2 partidas de **30 000**, boleta **20 000**, 100 000 USD/BTC. Plazos de obra en **segundos**.

| # | Escenario | On-chain | Doc | Script |
|---|---|---|---|---|
| 1 | Camino feliz (2 partidas cobradas) | MuSig2 × 3 (P1, P2, boleta) | [REGTEST_HAPPY_PATH.md](REGTEST_HAPPY_PATH.md) | `scripts/regtest_happy_path.sh` |
| 2 | Contratista no trabaja y se va **antes** de la partida 2 | unwind P1 → mandante; boleta → contratista; P2 no existe | [REGTEST_CONTRACTOR_ABANDONS.md](REGTEST_CONTRACTOR_ABANDONS.md) | `scripts/regtest_contractor_abandons.sh` |
| 3 | Obra hecha y el mandante **no firma** la recepción | **Igual que #2 en la cadena.** Bitcoin no ve el muro. Off-chain el contratista pierde el trabajo hundido. | esta tabla | el de #2 |
| 4 | Cancelación **cooperativa** (antes de T) | MuSig2 reembolsa P1 al mandante + boleta al contratista; witness 64 B; locktime 0 | [REGTEST_COOP_CANCEL.md](REGTEST_COOP_CANCEL.md) | `scripts/regtest_coop_cancel.sh` |
| 5 | Partida 1 **cobrada**, el mandante no sigue | MuSig2 P1 → contratista; P2 no se fondea; unwind boleta → contratista | [REGTEST_STOPS_AFTER_PARTIDA1.md](REGTEST_STOPS_AFTER_PARTIDA1.md) | `scripts/regtest_stops_after_partida1.sh` |

Robos que el CLI rechaza (minados como chequeo en #2 y #4):

- mandante `unwind --kind bond` → *timeout is not a bank boleta*
- contratista `unwind --kind partida` → *only the mandante can unwind a partida*

No hay más caminos on-chain distintos con el protocolo actual (2 UTXO, unwind vs MuSig2). Un árbitro tardío sería un árbol Taproot nuevo; no está en el MVP.
