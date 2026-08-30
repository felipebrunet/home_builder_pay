# Demo: el contratista no pone boleta

Script: [`scripts/regtest_no_bond.sh`](../scripts/regtest_no_bond.sh).

Sin input del contratista **no hay obra**. El fondeo atómico no existe. Si el mandante, aun así, manda sats a la address de la partida 1:

1. `verify-funding` → `missing bond output`
2. `--partida-only` → `bond must be funded before any partida`
3. El proyecto sigue `accepted` / boleta `unfunded`
4. Esos sats **sí** cayeron en el Taproot (el mandante pagó a una address conocida). Tras T, unwind: `92c12d05…31be`

No hay boleta locked. El contratista no arriesgó nada y no puede cobrar.
