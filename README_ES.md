# home_builder_pay

[English](README.md)

Custodia Bitcoin entre pares para **partidas de obra** más una **boleta de garantía**. Dos partes — el mandante y el contratista — bloquean fondos en un 2-de-2 Taproot (MuSig2). No hay servidor.

Esto es un MVP: CLI de escritorio, **regtest/signet**, archivos pasados a mano. Todavía no hay Tor, DHT ni árbitro.

Protocolo, arquitectura, hoja de ruta y **en qué quedó la última sesión**: [docs/PROJECT.md](docs/PROJECT.md) (empezar por la sección 0).

Hito actual: **MVP-0** más un camino feliz **minado** en regtest. Cierre cooperativo en la misma máquina: `hbp coop-close --peer-dir`. Relato: [docs/REGTEST_HAPPY_PATH.md](docs/REGTEST_HAPPY_PATH.md).

## Protocolo (resumen)

Dos salidas Taproot, nunca mezcladas:

```
boleta   = tr(musig(M,C), pk(C) && after(T_proyecto))
partida  = tr(musig(M,C), pk(M) && after(T_partida))
```

- Cierre cooperativo (recepción conforme): ambas partes firman con MuSig2. En cadena parece un pago normal.
- Vencimiento de una partida: el mandante recupera **solo** ese pago.
- Vencimiento del proyecto: el contratista recupera **solo** la boleta.

La boleta es **global** (10 % del proyecto por defecto, configurable en puntos básicos) y permanece bloqueada hasta la última partida. Se fondea una partida a la vez. Los montos del contrato están en fiat/UF; los sats se cotizan al fondear.

El unwind **no** es una boleta bancaria: Bitcoin no puede ver si el muro está construido. La defensa del contratista son partidas pequeñas y detener el trabajo si no hay recepción.

## Compilación

```bash
cargo test --workspace
cargo run -p hbp-cli -- --help
```

Nombre del ejecutable: `hbp`.

## Esquema de la CLI

Dos directorios, uno por parte:

```bash
# mandante
hbp --dir .m init --network regtest --role mandante
hbp --dir .m new --unit USD --bond-bps 1000 --t-project 1800000000
hbp --dir .m add-partida --desc Cimentación --amount 1500 --plazo 1700000000
hbp --dir .m add-partida --desc Muros --amount 500 --plazo 1710000000
hbp --dir .m offer                         # escribe .m/00-offer.json

# contratista
hbp --dir .c init --network regtest --role contratista
hbp --dir .c accept .m/00-offer.json      # escribe .c/01-accepted.pending.json

# el mandante contrafirma
hbp --dir .m commit .c/01-accepted.pending.json

# el contratista importa el contrato firmado
hbp --dir .c import .m/contracts/<id>/01-accepted.json

# ambos firman una cotización en sats (precio de BTC en la unidad del contrato)
hbp --dir .m quote --btc-price 80000 --fx-note "manual"
hbp --dir .c accept-quote .m/contracts/<id>/02-quote.json
hbp --dir .m accept-quote .c/contracts/<id>/02-quote.json   # el mandante importa la cotización ya contrafirmada

hbp --dir .m addresses
hbp --dir .c status
```

`verify-funding` comprueba una transacción de fondeo en crudo contra los montos cotizados (rechaza un monto de partida malicioso). `unwind` arma la transacción de timeout por script path después de `T`.

Las claves están en **texto plano** en `.hbp/identity.json`. Solo para pruebas. No usar en mainnet.

## Crates

| crate | función |
|---|---|
| `hbp-core` | JSON del contrato, máquina de estados, registro de nonces |
| `hbp-bitcoin` | descriptores Taproot, key-path MuSig2, unwind CLTV, comprobación de fondeo |
| `hbp-cli` | protocolo por archivos |

## Licencia

[MIT](LICENSE)

## Fuera de este MVP

Tor, DHT, Android, árbitro tardío, boleta que rueda al siguiente 2-de-2 sin devolverse, watchtowers, mainnet.
