# Fondeo atómico con Blue Wallet (las dos puntas)

Watch-only **local**. La chain se consulta con Esplora. Blue o Electrum solo firman el PSBT. El xpub no sale de la laptop.

Las dos contrapartes hacen lo mismo: mandante y contratista, cada uno con su hot wallet Signet + su `hbp --dir`. En Signet las carpetas de la UI son `.ms` (quien paga) y `.cs` (quien trabaja). `.m` / `.c` son regtest.

No relaja la atomicidad: sigue siendo **una tx, dos inputs**. Hasta que las dos firmas no están, nadie locked.

## Dos capas de claves (no mezclar)

| Dónde | Qué | Para qué |
|---|---|---|
| Blue / Electrum | Seed BIP39, Native SegWit o Taproot | Gastar el UTXO de fondeo y **cobrar** la recepción |
| `hbp` (`identity.json`) | Un secp256k1 | Offer, quote, MuSig2, unwind. Blue **no** firma esto |

Al peer: pubkey comprimido (ya va en el offer) y, al fondear, **un** `05-coin.json` (outpoint, sats, address, change). Nunca `watch.json` ni el xpub.

## 0. En cada punta

1. Blue o Electrum en **Signet**. Wallet Native SegWit. Recibir faucet ([signetfaucet.com](https://signetfaucet.com), [alt.signetfaucet.com](https://alt.signetfaucet.com)).
2. `hbp init --network signet --role mandante|contratista` en esa laptop (UI: `.ms` / `.cs`).
3. Wallet details → copiar **xpub** (zpub/vpub o tpub). Pegarlo **solo** en tu `hbp`:

```bash
hbp --dir .ms watch-import --xpub vpub...          # mandante
hbp --dir .cs watch-import --xpub vpub...          # contratista
```

`--kind wpkh` es el default. Taproot: `--kind tr`. El primer `receive/0` tiene que coincidir con una address de esa Blue.

`watch.json` queda 0600 en `--dir`. No se manda.

Contrato + quote: igual que siempre (`offer` / `accept` / `commit` / `quote`). Ver [SIGNET_TWO_PCS.md](SIGNET_TWO_PCS.md) §1.

## 1. Elegir moneda (habla con la chain, no con Blue)

Default Signet: prueba `https://blockstream.info/signet/api`, si no `https://mempool.space/signet/api` (a veces timeout). Otro: `--esplora` o `HBP_ESPLORA`. El scan recorre el gap (20 receive + 20 change); no es solo la primera address.

```bash
hbp --dir .m coins
hbp --dir .c coins
```

Elegí **una** moneda que cubra tu parte (partida+fee/2 el mandante; boleta+fee/2 el contratista). El vuelto lo sugiere el scan (primera change libre).

```bash
hbp --dir .m offer-coin --outpoint TXID:VOUT
hbp --dir .c offer-coin --outpoint TXID:VOUT
```

Se escribe `contracts/<id>/05-coin.json`. Ese archivo **sí** se pasa por Signal. No lleva el xpub.

## 2. PSBT atómico (cualquiera lo arma)

Con los dos `05-coin.json`:

```bash
hbp --dir .m fund --mine contracts/<id>/05-coin.json --peer 05-coin-del-otro.json
# el contratista, simétrico:
hbp --dir .c fund --mine contracts/<id>/05-coin.json --peer 05-coin-del-otro.json
```

Sale `05-funding.unsigned.psbt` (y base64 en stdout). Misma tx en las dos puntas si los coins coinciden.

## 3. Firmar en Blue — no transmitir

1. Pasá el `.psbt` al teléfono (archivo / QR si cabe).
2. Blue: importar PSBT → firmar **tu** input.
3. **No** pulses Send / Broadcast.
4. Exportá el PSBT firmado y devolvelo.

Si el otro no firma, tu UTXO sigue en Blue. Podés gastarlo y el PSBT muere.

## 4. Combinar y transmitir

Los **dos** se quedan el hex. Cualquiera transmite.

```bash
hbp --dir .m fund-combine tu-firmado.psbt el-del-otro.psbt
```

Blue: Settings → Tools → Broadcast Transaction (hex). Confirmá 1 bloque.

```bash
hbp --dir .m verify-funding --tx-hex HEX --partida 1
hbp --dir .c verify-funding --tx-hex HEX --partida 1
```

Recepción y unwind: igual que siempre, **solo `hbp`**. Destino de cobro = una address de Receive de Blue.

## Privacidad

Un Esplora público ve las addresses que consultás (todo el gap del xpub). En signet da igual. En mainnet más adelante: Electrum/Esplora propio. No es un servidor de este proyecto.

## UI de prueba

`cargo run -p hbp-ui` → red **Signet** primero, carpetas `.ms` / `.cs`. Paso 4 = watch-only. Paso 5 carga el UTXO de `status` (tras `verify-funding`); la address de cobro se pega a mano.

Camino feliz minado: [SIGNET_HAPPY_PATH.md](SIGNET_HAPPY_PATH.md).
