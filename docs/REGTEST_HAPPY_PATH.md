# Demo camino feliz en Bitcoin regtest

Ejecución real, on-chain, del protocolo home_builder_pay. No es un mock: las transacciones están en el nodo local (`/home/felipe/projects/btc_clients`, chain `regtest`).

Fecha: 2026-08-30.  
Script reproducible: [`scripts/regtest_happy_path.sh`](../scripts/regtest_happy_path.sh).  
Directorio de trabajo de esa corrida: `/tmp/hbp-regtest-demo`.

---

## 1. Qué se simuló

Obra de **60 000 USD** en dos partidas de **30 000 USD**. El contratista pone una boleta de **20 000 USD** (no el 10 % por defecto: es un tercio del proyecto; la CLI avisa que es alta).

Tipo de cambio de la demo: **100 000 USD / BTC** (número redondo, no un oracle).

| Concepto | USD | BTC | sats |
|---|---:|---:|---:|
| Partida 1 (cimentación) | 30 000 | 0,30 | 30 000 000 |
| Partida 2 (muros) | 30 000 | 0,30 | 30 000 000 |
| Boleta global | 20 000 | 0,20 | 20 000 000 |

En producción los plazos serían semanas. Aquí cada partida se “ejecuta” en **5 segundos** (`sleep 5` entre fondeo confirmado y recepción). Los CLTV del contrato quedaron ~20–60 s por delante del `now` de la máquina, solo como margen para armar las txs; el camino feliz **no usa** el timeout, usa MuSig2.

Hay **dos capas de claves**, a propósito:

1. **Hot wallets** de Bitcoin Core (`hbp_mandante`, `hbp_contratista`): tienen los UTXO de bitcoin-cli. Pagan el fondeo.
2. **Identidades HBP** (`.m/identity.json`, `.c/identity.json`): claves del 2-de-2 Taproot. Firman recepción y liberación de boleta. No son las mismas que las hot wallets.

---

## 2. Cómo repetirlo

```bash
/home/felipe/projects/btc_clients/start-bitcoind.sh
cd /home/felipe/projects/home_builder_pay
./scripts/regtest_happy_path.sh
```

Requisitos: `bitcoind` de `btc_clients`, Python 3, `jq` no hace falta. El script crea las wallets si no existen, madura coinbases del wallet `miner` (101 bloques) y envía 10 BTC a cada hot wallet.

`coop-close --peer-dir` es un atajo **misma máquina**: carga las dos `identity.json` y firma MuSig2 en un proceso. En dos laptops el mismo cierre será un intercambio de nonces/archivos (aún no cableado como comandos separados).

---

## 3. Paso a paso (corrida registrada)

Nodo: `getblockcount` terminó en **489**. Contrato:

`bc4a47b83cd2dcc4a0420c8a0f210f60664c2556bc359c6355dd487c67818b4a`

### 3.1 Arranque del nodo y wallets

1. `start-bitcoind.sh` — Core 31.1, RPC `127.0.0.1:18443`, datadir `btc_clients/data/bitcoind`.
2. Wallets: `miner` (coinbase), `hbp_mandante`, `hbp_contratista`.
3. `generatetoaddress 101` al miner para madurar coinbase (había ~1550 BTC *immature*).
4. `sendtoaddress` 10 BTC a cada hot wallet + 1 bloque.

Saldos tras el fondeo de esta corrida: mandante **20 BTC**, contratista **20 BTC** (esta máquina ya tenía 10 BTC de un intento anterior).

Addresses de recepción hot (P2WPKH, no son las del escrow):

- mandante: `bcrt1q5xqq3zne3w097cyz3qk0sxyqguqsdws4ltj0cy`
- contratista: `bcrt1q6smnnd3ujy0qud89q3smvsnwrepy8ls9jrhngu`

### 3.2 Contrato HBP (off-chain)

```text
hbp --dir .m init --network regtest --role mandante
hbp --dir .c init --network regtest --role contratista
hbp --dir .m new --unit USD --bond-bps 3333 --t-project 1788121368
hbp --dir .m add-partida --desc Cimentacion --amount 30000 --plazo 1788121328
hbp --dir .m add-partida --desc Muros --amount 30000 --plazo 1788121348
hbp --dir .m offer
hbp --dir .c accept .m/00-offer.json
hbp --dir .m commit .c/01-accepted.pending.json
hbp --dir .c import .m/contracts/<id>/01-accepted.json
hbp --dir .m quote --btc-price 100000 --bond-sats 20000000 --fx-note "demo 100000 USD/BTC"
hbp --dir .c accept-quote …
hbp --dir .m accept-quote …
```

`bond-bps 3333` ≈ 19 998 USD sobre 60 000; los **20 000 USD exactos** se fijan con `--bond-sats 20000000`. La CLI avisó: *boleta over 30% of the project*.

Pubkeys de contrato (comprimidas):

- mandante `026b591107dcfd38132297e88b351f81df1453b86e674b3c71f52e39d012b58a8d`
- contratista `0220e55c7b7b3c37a6a86e690d24bd444b7146823d5ad501735c8b2d69d1f1c69d`

### 3.3 Addresses Taproot (idénticas en ambas puntas)

```text
boleta     bcrt1paxhdddrmjn7lpf52kzcfned4k383cqy5vy09xpzw4xqjhfvlfqtq4csc0j
partida 1  bcrt1pklqweejdj9m694tudhwdaex864jl4mdxje4twzakvxeehwyslczqqqw4mn
partida 2  bcrt1pvzfqe5plswt6n55lcnlj6chvnazkkumca4u657crrdgjet06cyssk6dz8n
```

`witness_v1_taproot` (`bcrt1p…`). Cada partida tiene otra address porque el plazo entra en el tweak.

### 3.4 Fondeo atómico: boleta + partida 1

Una sola transacción, dos inputs (una UTXO de cada hot wallet), cuatro outputs:

txid: `72939018bca2eca4b5d31c8126e453679a796277e5e02f4bea96a259ace17871`

| vout | BTC | Address | Rol |
|---:|---:|---|---|
| 0 | 0,20 | `bcrt1paxhdd…csc0j` | boleta (Taproot 2-de-2) |
| 1 | 0,30 | `bcrt1pklqwe…qw4mn` | partida 1 |
| 2 | 9,6999 | `bcrt1qh47t7…` | change mandante (P2WPKH) |
| 3 | 9,7999 | `bcrt1qkws6r…` | change contratista (P2WPKH) |

Se armó con `createpsbt` + `walletprocesspsbt` en las dos wallets + `finalizepsbt` + `sendrawtransaction`. Fee ~0,0002 BTC repartido en los changes.

`hbp verify-funding --tx-hex … --partida 1` en **ambos** `--dir`: montos y scripts coinciden con el quote.

### 3.5 Obra partida 1 (5 s) y recepción MuSig2

`sleep 5`. Destino del pago: address nueva de la hot wallet del contratista.

```text
hbp --dir .m coop-close --kind partida --partida 1 \
  --outpoint 72939018…7871:1 --sats 30000000 \
  --dest bcrt1qd3vckl2lss4233zz2pw2mt6y0zhwp56s8s3dv5 \
  --fee 200 --peer-dir .c
```

txid de pago: `a0829aed29734b4e648c404ef8e9ee9cc5777fe790d586b04dcb6ff856147898`

Comprobado on-chain:

- gasta `72939018…:1` (la partida 1)
- **1 item de witness, 64 bytes** → key-path Taproot / MuSig2 (parece un pago simple)
- output 0,299998 BTC (`30 000 000 − 200` sats) a `bcrt1qd3vck…`

Se minó 1 bloque. Confirmaciones al documentar: 4.

### 3.6 Fondeo partida 2 (solo mandante)

La boleta ya está locked. El mandante envía 0,30 BTC a la address de la partida 2.

txid: `4d232649ba831d34dc7e6ec6021f9ade8239024bad99154e8b3935c6153f5920`  
vout 0 = 0,30 BTC Taproot.

```text
hbp verify-funding --tx-hex … --partida 2 --partida-only
```

### 3.7 Obra partida 2 (5 s) y recepción

`sleep 5`. Otro `coop-close` de la partida 2.

txid: `13f2bd2908d12705ff38b0a6d7edd89dbb5ee62d50a1ef2551fb33487ef520e6`

### 3.8 Liberar la boleta

Las dos partidas están `paid`. MuSig2 sobre el UTXO de la boleta (`72939018…:0`, 20 000 000 sats) hacia una address del contratista.

txid: `84848099d3ce1e55adc9536fae1912986bed25d1e6f611fb5db6555d61fb4a28`

Estado HBP: `closed` / boleta `released` / partidas `(1, paid), (2, paid)`.

---

## 4. Resultado económico (hot wallets)

| Wallet | Antes (aprox.) | Después | Δ |
|---|---:|---:|---:|
| `hbp_mandante` | 20,000000 | **19,399867** | −0,600133 (dos partidas de 0,30 + fees de fondeo) |
| `hbp_contratista` | 20,000000 | **20,599894** | +0,599894 (cobra 0,60 de obra; recupera 0,20 de boleta; fees de 200 sats × 3 closes + fondeo) |

El mandante pagó ~60 000 USD en BTC. El contratista cobró las dos partidas y recuperó la boleta. Nadie se quedó el colateral del otro.

---

## 5. Qué demuestra esta demo

- Contrato firmado, quote en sats, descriptores deterministas.
- Fondeo **atómico** boleta + partida 1 (dos firmantes, una tx).
- Una partida a la vez: la 2 no se fondea hasta que la 1 está pagada (regla de estado + orden del script).
- Recepción cooperativa **minada**, indistinguible de un Schnorr simple (64 bytes).
- Boleta que rueda en el tiempo (mismo UTXO todo el proyecto) y se libera al final.
- Plazos de obra de **segundos** en vez de semanas, sin tocar el diseño CLTV.

No cubre: unwind por timeout, peer malicioso, Tor, dos máquinas físicas (el MuSig2 se firmó en un proceso con `--peer-dir`).

---

## 6. Comandos Bitcoin usados

```text
bitcoin-cli -conf=… -datadir=…  \
  createwallet | loadwallet | getnewaddress | sendtoaddress |
  generatetoaddress | listunspent | createpsbt | walletprocesspsbt |
  finalizepsbt | sendrawtransaction | getrawtransaction | getbalances
```

Cookie auth en el datadir; no hay `rpcuser` en `bitcoin.conf`.
