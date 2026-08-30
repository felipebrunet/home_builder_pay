# Demo: el contratista no cumple y se va antes de la partida 2

Ejecución on-chain en Bitcoin **regtest**. Complementa el camino feliz de [REGTEST_HAPPY_PATH.md](REGTEST_HAPPY_PATH.md).

Fecha: 2026-08-30.  
Script: [`scripts/regtest_contractor_abandons.sh`](../scripts/regtest_contractor_abandons.sh).  
Directorio de esa corrida: `/tmp/hbp-regtest-abandon`.

---

## 1. Qué se prueba

Misma obra de **60 000 USD** (2 × 30 000, boleta **20 000**, 100 000 USD/BTC).

El contratista:

1. Firma el contrato y pone la boleta junto con la partida 1.
2. **No trabaja.** No hay recepción MuSig2.
3. Se va **antes de que exista** la partida 2 on-chain (nunca se fondea).

El mandante, pasado el plazo (aquí segundos, no semanas):

- Recupera los **30 000 USD** de la partida 1 (unwind script-path, solo su clave).
- **No puede quedarse la boleta.** `hbp unwind --kind bond` con la identidad del mandante falla: *only the contratista can unwind the bond; timeout is not a bank boleta*.

La boleta, después de `T_proyecto`, la puede barrer **el contratista** (aunque haya abandonado la obra). Eso es el unwind que acordamos: no es ejecución de boleta bancaria. El daño al contratista es no cobrar y haber tenido 0,20 BTC inmovilizados; el mandante no pierde el pago de la partida impaga.

---

## 2. Código que hizo falta (además del MVP-0)

Para más E2E de este tipo, sí hay que tocar `hbp` un poco. En esta corrida:

| Cambio | Por qué |
|---|---|
| `unwind` actualiza `state.json` (`unwound` / `cancelled`) | Si no, el CLI decía que la partida seguía `locked` con la tx ya minada. |
| `--peer-dir` en `unwind` | Copia el estado al otro `--dir` en demos de una máquina. |
| Rechazar unwind de boleta si quien firma es el mandante | Evita marcar estado a partir de una firma que el script no aceptaría. |
| `setmocktime` + 12 bloques en el script | CLTV compara contra el **median time** de la cadena, no contra el reloj del `sleep`. |

No hizo falta un `coop-close` (no hay recepción). El fondeo atómico reutiliza el mismo PSBT de dos wallets.

Para el **siguiente** E2E ( mandante que no firma la recepción a pesar de obra hecha, o disputa a mitad de partida 2) el hueco que sigue es el mismo: CLI de MuSig2 **por archivos** y, si se quiere, `hbp fund` sin Python.

---

## 3. Cómo repetirlo

```bash
/home/felipe/projects/btc_clients/start-bitcoind.sh
cd /home/felipe/projects/home_builder_pay
./scripts/regtest_contractor_abandons.sh
```

Los plazos del contrato son `now+8s` / `now+16s` / `now+24s`. Tras 5 s de espera el script adelanta el tiempo de la cadena (`setmocktime`) para que el unwind sea final.

---

## 4. Paso a paso (corrida registrada)

Contrato: `1f94f3d4ed9ef1ac4e326c01b4c3f8ffb42d298d735da91fcac53199261eafa3`

Saldos al partir (después del camino feliz): mandante **19,399867 BTC**, contratista **20,599894 BTC**.

### 4.1 Contrato y addresses

Igual que el feliz: offer → accept → commit → quote 20 000 000 / 30 000 000 sats.

```text
boleta     bcrt1phvkg4mgqd94qf9stnyfrsyr9q68ks8arzqjkw4fwjkxspns0k8qq2rdu50
partida 1  bcrt1pmdwaj8y0tsz4qsh4wez76zl7qgjqhr48alk9x8ce7ywx5r899a7qt2dege
partida 2  bcrt1p6pht9978fthrpqzzusd50z3xgdwqckqrgcqqn6hyl3ryml6wlrkq2zcsnn
           ↑ nunca recibe un satoshi
```

CLTV: t1=`1788121979`, t2=`1788121987`, t_proyecto=`1788121995`.

### 4.2 Fondeo atómico (sí hubo compromiso)

txid `d77cebda3f0ed63b3dd34b1a89c5c1d56ed236fc405c247b3016cb27e49a4cee`

- vout 0: **0,20 BTC** boleta  
- vout 1: **0,30 BTC** partida 1  

`verify-funding` OK en ambos `--dir`. **No** hay `sendtoaddress` a la partida 2.

### 4.3 El contratista no aparece (5 s)

No se llama `coop-close`. El mandante no paga. La obra no se “recibe”.

### 4.4 Tiempo de cadena

`setmocktime 1788122596` y 12 bloques para que el median time pase ambos CLTV.

### 4.5 Unwind de la partida 1 (mandante)

```text
hbp --dir .m unwind --kind partida --partida 1 \
  --outpoint d77cebda…4cee:1 --sats 30000000 \
  --dest bcrt1qzyanm4uspx7d57403z99f84vzvertcjqwamk5y --fee 200 --peer-dir .c
```

txid `25493403efcfedab5c5b06bb50209df786d3bb10da67d0955984ca8a6bd825d6`

On-chain, distinto al camino feliz:

- **3 items de witness** (64 + 40 + 33 bytes) = script path (firma + hoja + control block), no el Schnorr suelto de 64 bytes del MuSig2.
- `locktime` = 1788121979 = plazo de la partida 1.
- Output 0,299998 BTC de vuelta al mandante.

### 4.6 El mandante no ejecuta la boleta

```text
hbp --dir .m unwind --kind bond --outpoint d77cebda…4cee:0 …
→ error: only the contratista can unwind the bond; timeout is not a bank boleta
```

### 4.7 El contratista barre su boleta (después de T)

Aunque abandonó el trabajo, la hoja `pk(C) && after(T_proyecto)` es suya.

txid `c38984af78488dda29792ed7f7590770c11cad28122f2cb5be5b7e910264cef7`  
Witness de 3 items, locktime 1788121995, 0,199998 BTC al contratista.

---

## 5. Resultado

Estado HBP: **`cancelled`**, boleta **`unwound`**, partidas **`(1, unwound), (2, amount_agreed)`**.

| Wallet | Antes | Después | Lectura |
|---|---:|---:|---|
| mandante | 19,399867 | **19,399765** | recuperó los 0,30 de la partida; solo perdió fees |
| contratista | 20,599894 | **20,599792** | recuperó los 0,20 de boleta; **no cobró** obra; fees |

La partida 2 no existió on-chain. Nadie se quedó el dinero del otro. El protocolo hizo lo que dice el diseño: timeout ≠ boleta de banco.

---

## 6. Contraste con el camino feliz

| | Feliz | Contratista se va |
|---|---|---|
| Partida 1 | MuSig2, 1 witness de 64 B, cobra el contratista | Unwind, 3 witness, recupera el mandante |
| Partida 2 | fondeada y pagada | **nunca fondeada** |
| Boleta | MuSig2 al contratista al cierre | unwind al contratista tras T; el mandante no puede tomarla |
| Estado | `closed` / `released` / `paid, paid` | `cancelled` / `unwound` / `unwound, amount_agreed` |
