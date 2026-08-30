# Demo: cancelación cooperativa (sin timeout)

Script: [`scripts/regtest_coop_cancel.sh`](../scripts/regtest_coop_cancel.sh).  
Corrida: `/tmp/hbp-regtest-coop-cancel`. Contrato `d2a9fe1d860c63b682f8c65c942bbab3b6cac9edc386b6174c111e244bce59fe`.

Ambos acuerdan abortar **antes** del plazo. No hay `setmocktime`. Es el “nos echamos para atrás” de Copec, no una ejecución de boleta.

## Pasos

1. Fondeo atómico boleta 0,20 + partida 1 0,30: `0f357334…7426`.
2. El contratista **no puede** unwind de la partida (`only the mandante can unwind a partida`).
3. `coop-close --refund` de la partida 1 hacia el mandante: `a3a1beb4…2e6f`.  
   Witness **1 × 64 bytes**, `locktime 0` (key-path MuSig2, no script-path).
4. `coop-close --refund` de la boleta hacia el contratista: `d90334b5…7a37`. Misma forma.

Partida 2 nunca se fondea.

## Resultado

Estado: `cancelled` / boleta `unwound` / partidas `(1, unwound), (2, amount_agreed)`.

Saldos: mandante 19,399765 → **19,399663**; contratista 20,599792 → **20,59969**. Solo fees. Nadie cobró obra.

Diferencia con el abandono (#2): allí hay que esperar T y el witness es de **3** piezas (CLTV). Aquí las dos puntas firman ya.
