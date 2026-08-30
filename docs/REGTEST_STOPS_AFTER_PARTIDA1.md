# Demo: partida 1 cobrada, el proyecto no sigue

Script: [`scripts/regtest_stops_after_partida1.sh`](../scripts/regtest_stops_after_partida1.sh).  
Corrida: `/tmp/hbp-regtest-stop-p1`. Contrato `02301170e0ad82660e5ab6bf68332e9a219d8dc68147be13536729977c86ec14`.

El contratista **sí cumplió** la primera partida (5 s + recepción). El mandante no fondea la segunda y no coopera para soltar la boleta. Tras `T_proyecto` el contratista la barre **solo**.

## Pasos

1. Fondeo atómico: `132867eb…8953` (0,20 boleta + 0,30 partida 1).
2. Sleep 5 s. MuSig2 paga al contratista: `7337fedf…0043` (witness 64 B, 0,299998 BTC).
3. Partida 2 **no** se fondea.
4. `setmocktime` + 12 bloques.
5. Unwind de boleta por el contratista: `9dbd6e3d…c42d` (witness 3 piezas, locktime `1788122363`).

## Resultado

Estado: `cancelled` / boleta `unwound` / `(1, paid), (2, amount_agreed)`.

| Wallet | Antes | Después | Lectura |
|---|---:|---:|---|
| mandante | 19,399663 | **19,099563** | pagó la partida 1 (~0,30) y no siguió |
| contratista | 20,59969 | **20,899586** | cobró 0,30 de obra y recuperó 0,20 de boleta |

El contratista no queda atrapado: si el mandante desaparece después de una recepción, la boleta sale por timeout. La partida 2 simplemente no existe.
