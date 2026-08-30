# Demo: partida 2 aceptada al 80 % (pintura fallida)

Script: [`scripts/regtest_split_80.sh`](../scripts/regtest_split_80.sh).  
Corrida: `/tmp/hbp-regtest-split80`. Contrato `3e81c6f7adc13f166036627eb700d46139500bd8d7d204660555db478d011cc7`.

Bitcoin **no sabe** que la mano de pintura falló. Lo que hay es un **acuerdo**: las dos claves firman un split. El 2-de-2 puede pagar 100 %, 80 %, 1 sat + el resto, lo que sea, mientras ambos firmen.

## Números

Partida 2 estaba locked en **0,30 BTC** (30 000 USD).

| Destino | Sats | BTC |
|---|---:|---:|
| Contratista (80 %) | 24 000 000 | 0,24 |
| Mandante (vuelto) | 5 999 800 | 0,059998 |
| Fee | 200 | 0,000002 |

## Pasos

1. Partida 1 al 100 %: `4d636016…8d01`.
2. Fondeo partida 2: `a8730ad4…9be8`.
3. Sleep 5 s. `coop-close --pay-sats 24000000 --refund-dest <mandante>`:  
   tx `57274797…e797`  
   Witness **1 × 64 bytes** (MuSig2).  
   vout 0: **0,24** al contratista.  
   vout 1: **0,059998** al mandante.
4. Boleta liberada por acuerdo: `b54666c6…2ccd`.

Estado: `closed` / `released` / `(1, paid), (2, paid)` — la partida 2 se considera recibida, pero no al 100 % del monto locked.

## Saldos

Mandante 19,099563 → **18,559428** (−0,30 de P1 −0,24 netos de P2).  
Contratista 20,899586 → **21,439482** (+0,30 +0,24; boleta devuelta).

El mandante pagó **54 000 USD** de 60 000. La boleta no se tocó: el trabajo se aceptó con descuento, no hubo incumplimiento total.

## CLI

```text
hbp coop-close --kind partida --partida 2 \
  --outpoint <txid>:<vout> --sats 30000000 \
  --dest <addr_contratista> --pay-sats 24000000 \
  --refund-dest <addr_mandante> --fee 200 --peer-dir .c
```
