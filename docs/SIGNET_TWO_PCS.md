# Dos PCs en Signet (Sparrow + `hbp`)

Signet **global** (la de Core, no un signet privado). No hace falta minar ni ASICs: los bloques los firman los operadores; los sats salen de un **faucet**.

Hay **dos capas de claves**. No las mezcles.

| Dónde | Qué | Para qué |
|---|---|---|
| Sparrow (cada PC) | Hot wallet Signet (BIP86 Taproot o SegWit) | Pagar el **fondeo** y **cobrar** la recepción |
| `hbp` (cada PC) | `identity.json` (mitad del 2-de-2 MuSig2) | Offer, quote, `coop-sign`, unwind. **Sparrow no firma esto** |

Red: `init --network signet`. Addresses de escrow: `tb1p…`. Addresses de Sparrow: `tb1q…` o `tb1p…`.

El faucet da ~1 000–10 000 sats. **No** uses el demo de 60 k USD (serían 0,3 BTC). Abajo hay montos que caben.

---

## 0. En cada PC

1. Abrir Sparrow en Signet, no mainnet ni testnet4:

```bash
Sparrow -n signet
# o: export SPARROW_NETWORK=signet
```

2. Server: público **mempool.space** Signet (`ssl://mempool.space:60602`). Test Connection.

3. File → New Wallet → software hot wallet (Taproot o Native SegWit). Backup de **esa** seed de Sparrow es aparte del backup de `hbp`.

4. Recibir: una address `tb1…` → faucet, p.ej. [signetfaucet.com](https://signetfaucet.com) o [alt.signetfaucet.com](https://alt.signetfaucet.com). Confirmar en [mempool.space/signet](https://mempool.space/signet).

5. En el mismo PC, `hbp` (binario o `cargo build -p hbp-cli`).

Objetivo: cada uno ~10 000 sats confirmados. Si el faucet está corto, dos pedidos o montos más chicos abajo.

---

## 1. Contrato (`hbp`, archivos por Signal/USB)

Plazos en unix (Signet usa tiempo real). Ejemplo: T1 = ahora+7d, T2 = +14d, T proyecto = +21d:

```bash
python3 -c "import time; n=int(time.time()); print(n+7*86400, n+14*86400, n+21*86400)"
```

**PC mandante**

```bash
hbp --dir .m init --network signet --role mandante   # opcional: --passphrase …
hbp --dir .m new --unit USD --bond-bps 1000 --t-project <T_PROY>
hbp --dir .m add-partida --desc Cimentacion --amount 5 --plazo <T1>
hbp --dir .m add-partida --desc Muros --amount 5 --plazo <T2>
hbp --dir .m offer
```

Pasa `00-offer.json`.

**PC contratista**

```bash
hbp --dir .c init --network signet --role contratista
hbp --dir .c accept 00-offer.json
```

Pasa `01-accepted.pending.json`. Mandante: `commit`. Contratista: `import` del `01-accepted.json`.

Quote (precio redondo; 5 USD @ 100 000 USD/BTC = **5 000 sats** por partida; boleta 10 % = **1 000 sats**):

```bash
hbp --dir .m quote --btc-price 100000 --fx-note "signet demo"
```

Se pasan `02-quote.json` hasta las dos firmas. `hbp --dir .m addresses` → `tb1p` de boleta y partidas.

---

## 2. Fondeo (Sparrow firma; `hbp` solo arma el PSBT)

En cada Sparrow, pestaña UTXOs: anota `txid:vout`, **sats**, address de **ese** UTXO. Una address de cambio nueva.

**Mandante paga** partida 1 (5 000) + mitad del fee. **Contratista paga** boleta (1 000) + mitad del fee. Fee p.ej. 500.

```bash
hbp --dir .m fund --fee 500 \
  --m-outpoint TXID:VOUT --m-sats N --m-prev ADDR --m-change ADDR_CAMBIO_M \
  --c-outpoint TXID:VOUT --c-sats N --c-prev ADDR --c-change ADDR_CAMBIO_C
```

Stdout = PSBT base64.

1. Sparrow mandante: File → Open Transaction / paste PSBT → Sign (solo su input).
2. Pasa el PSBT al contratista → Sign.
3. Broadcast (cualquiera).
4. 1 confirmación. Hex crudo (Sparrow o mempool) → ambos:

```bash
hbp --dir .m verify-funding --tx-hex HEX --partida 1
hbp --dir .c verify-funding --tx-hex HEX --partida 1
```

---

## 3. Recepción (solo `hbp`, no Sparrow)

El contratista copia una address de **cobro** de su Sparrow.

```bash
# mandante
hbp --dir .m coop-propose --kind partida --partida 1 \
  --outpoint TXID_FONDEO:VOUT_P1 --sats 5000 --dest <tb1 del contratista>
# pasa 04-coop.json

# contratista
hbp --dir .c coop-sign 04-coop.json
# devuelve su 04-coop.json

# mandante
hbp --dir .m coop-finish 04-coop-del-c.json    # tx hex
```

Broadcast el hex (Sparrow “broadcast transaction” o `bitcoin-cli sendrawtransaction` si tienes nodo Signet). La plata cae en Sparrow del contratista. La **boleta no se mueve**.

Partida 2: `hbp fund --partida 2 --partida-only` (solo mandante firma en Sparrow) → otra recepción igual.

Boleta al cerrar: `coop-propose --kind bond` hacia address Sparrow del contratista.

---

## Si algo falla

- Address `tb1` en el faucet pero Sparrow en testnet4/mainnet → otra chain, no llega.
- PSBT: cada Sparrow solo firma **su** UTXO; si “no hay nada que firmar”, el `--m-prev` / `--c-prev` no es la address de ese coin.
- `coop-sign` no es un PSBT de Sparrow. Si abres `04-coop.json` en Sparrow, no va a servir.
- Montos: `verify-funding` exige **exactos** al quote. No mandes un pago suelto a la `tb1p` de la partida “a ojo”.
