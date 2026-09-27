# Escenarios de sanidad en regtest

Red local: `monerod --regtest --offline --fixed-difficulty 1`. No es stagenet ni mainnet. `generateblocks` mina en segundos. El coinbase se puede gastar a los 60 bloques. Un output normal, a los 10. El anillo es de 16, así que el arranque mina un colchón de señuelos antes de la primera firma.

No corre con `cargo test --workspace` ni con `cargo run`. Hace falta el binario `monerod` (variable `MONEROD` si no está en el `PATH`).

```bash
MONEROD=/ruta/monerod ./scripts/sanidad.sh
```

Ese script corre primero los tests del proyecto y después el pago al 100%. Si `monerod` ya está en el `PATH`, `MONEROD` no hace falta.

`scripts/sanidad.sh` corre todos los casos de abajo en un solo regtest, en serie. El encierre atómico de una sola transacción (los dos hot wallets en el mismo CLSAG) no entra: `monero-wallet` 0.2 firma todos los inputs con una sola spend. Cada fondeo son dos transferencias. El gasto de la caja sí es FROST 2-de-2.

Además del corte y los bricks de esta lista, el archivo `tests/regtest_pago.rs` prueba el 1%, el resto de la división entera, la salida de cero, el encierre a medias, el share cruzado, el doble gasto, la partida extra, lo que llegó de más, que Bob puede publicar y que la view de un tercero no ve el monto.

## Cadena

1. **Pago al 100%.** Alice y Bob arman la caja. Cada uno manda la misma garantía. Se mina hasta desbloquear. Los dos firman el gasto. El contratista recibe el pot menos el fee. El mandante no recibe nada de esa caja.
2. **Pago al 80% y al 50%.** Misma caja. El fee se resta del pot antes del corte. 80% deja `1.8 × garantía` al contratista y `0.2 × garantía` al mandante.
3. **Dos partidas.** Las dos están fondeadas. Pagar la primera no gasta el output de la segunda.
4. **Un lado no firma el encierre.** No se publica transacción. Los saldos de las hot wallets no cambian.
5. **Trato al 80%, un lado arma un 100% hacia sí.** El otro no firma. La plata sigue en la caja.
6. **Carol.** Su clave no completa la firma de esa caja.
7. **Antes de tiempo.** Gastar un output con menos de 10 bloques falla.
8. **Destino distinto.** La transacción no paga a la caja, o el gasto no vuelve a una de las dos hot wallets. Se rechaza antes de soltar el share.
9. **Abandono con plata adentro.** Los dos firman la devolución de cada garantía. Si uno no firma, sigue trabada.
10. **Share de otra obra.** El contexto del DKG incluye el id de la obra. Un share de otra caja no firma esta.

## Sin cadena

Contra, extra, abandono antes de encerrar y notas cifradas siguen en `konstruado-core`. No se minan bloques para repetirlos.
