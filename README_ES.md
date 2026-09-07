# home_builder_pay

[English](README.md)

Custodia Bitcoin entre pares para **partidas de obra** más una **boleta de garantía**. Dos partes — el mandante y el contratista — bloquean fondos en un 2-de-2 P2WSH. No hay servidor. `hbp` no guarda seeds: las firmas salen de Blue, Electrum, Sparrow, Ledger o Trezor.

Esto es un MVP: CLI de escritorio, **regtest/signet**, archivos y PSBT pasados a mano. Todavía no hay Tor ni DHT. El mandante elige **hold** (UTXO indefinido) o **burn** (quema a fee en T).

Protocolo nuevo: [docs/P2WSH.md](docs/P2WSH.md). Checkpoint: [docs/PROJECT.md](docs/PROJECT.md) §0. Taproot/MuSig2 queda en la rama `musig-mode`.

Hito actual: coordinador P2WSH **hold / burn**. UI de prueba: `cargo run -p hbp-ui` → http://127.0.0.1:3847.

## Protocolo (resumen)

Un UTXO P2WSH, partida = boleta:

```
wsh(sortedmulti(2, A/*, B/*))
```

- **Hold:** 1 firma cada uno al fondear (`m/84'`). Sin acuerdo el UTXO queda indefinido.
- **Burn:** primero firman la quema (`m/48'`, `nLockTime=T`, OP_RETURN + 100 % fee), después el funding.
- **Coop:** las dos `m/48'` pagan a la address acordada.

## Compilación

```bash
cargo test --workspace
cargo run -p hbp-cli -- --help
# UI de pruebas (localhost): cargo run -p hbp-ui
# abrir http://127.0.0.1:3847
```

Nombre del ejecutable: `hbp`.

## Esquema de la CLI

`hbp` no tiene seeds. Dos directorios, uno por parte:

```bash
# mandante
hbp --dir .mh init --network signet --role mandante
hbp --dir .mh cosigner Vpub...          # m/48' — esto SÍ se comparte
hbp --dir .mh watch-import --xpub vpub...  # m/84' — solo local
hbp --dir .mh new --mode hold --sats 5000 --fee 500
# burn:  hbp --dir .mh new --mode burn --sats 5000 --fee 500 --t-unix $(date -d '+1 hour' +%s)
hbp --dir .mh offer                     # .mh/00-offer.json

# contratista
hbp --dir .ch init --network signet --role contratista
hbp --dir .ch cosigner Vpub...
hbp --dir .ch watch-import --xpub vpub...
hbp --dir .ch accept .mh/00-offer.json  # .ch/01-accepted.json

hbp --dir .mh import .ch/01-accepted.json
hbp --dir .mh addresses

hbp --dir .mh coins
hbp --dir .mh offer-coin --outpoint TXID:VOUT   # .mh/05-coin.json
# igual en .ch, después:
hbp --dir .mh fund --mine .mh/05-coin.json --peer .ch/05-coin.json

# hold: cada hot wallet firma funding.psbt (su input)
# burn: primero burn.psbt con m/48', combine-burn; después funding
hbp --dir .mh combine-fund mio.signed.psbt el-del-otro.signed.psbt

hbp --dir .mh coop --dest tb1q... --fee 200
hbp --dir .mh combine-coop mio.signed.psbt el-del-otro.signed.psbt

# solo burn, pasado T:
hbp --dir .mh publish-burn
```

La UI hace este flujo: `cargo run -p hbp-ui` → http://127.0.0.1:3847

## Crates

| crate | función |
|---|---|
| `hbp-core` | términos hold/burn y estado |
| `hbp-bitcoin` | `wsh(sortedmulti(2))`, PSBT de funding/quema/coop |
| `hbp-cli` | coordinador (xpubs in, PSBT out) |

## Licencia

[MIT](LICENSE)

## Fuera de este MVP

Tor, DHT, Android, boleta que rueda al siguiente 2-de-2 sin devolverse, watchtowers, mainnet. Lo siguiente: dos laptops en signet con el protocolo de archivos.
