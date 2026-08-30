# Demo: el mandante cancela el resto tras cobrar la partida 1 (ambos acuerdan)

Script: [`scripts/regtest_cancel_after_p1.sh`](../scripts/regtest_cancel_after_p1.sh).  
Contrato `93abca19…6687`.

Distinto de “el mandante desaparece” ([REGTEST_STOPS_AFTER_PARTIDA1.md](REGTEST_STOPS_AFTER_PARTIDA1.md)): aquí el contratista **acepta** parar. La boleta se suelta **ya** con MuSig2, sin `setmocktime`.

1. P1 pagada 100 %: `c9797b72…7f98`
2. Boleta liberada ahora: `f1d569b0…0219` (witness 64 B)

Estado: `closed` / `released` / `(1, paid), (2, amount_agreed)`.

Mandante −0,30 (pagó P1). Contratista +0,30 y recupera la boleta al toque.
