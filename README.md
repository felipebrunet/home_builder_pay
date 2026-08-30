# home_builder_pay

P2P Bitcoin escrow for **partidas + boleta de garantía**. Two parties (mandante and contratista) lock funds in Taproot 2-of-2 (MuSig2). There is no server.

This is an MVP: desktop CLI, **regtest/signet**, files passed by hand. No Tor, no DHT, no arbiter yet.

Protocol, architecture, roadmap and **where the last session left off**: **[docs/PROJECT.md](docs/PROJECT.md)** (start at §0).

Current hito: **MVP-0** (CLI + Taproot/MuSig2 + tests, no bitcoind). Next: cooperative-reception CLI (`coop-nonce` / `coop-sign` / `coop-finish`).

## Protocol (short)

Two Taproot outputs, never mixed:

```
boleta   = tr(musig(M,C), pk(C) && after(T_proyecto))
partida  = tr(musig(M,C), pk(M) && after(T_partida))
```

- Cooperative close (recepción conforme): both sign MuSig2. Looks like a normal payment.
- Timeout of a partida: mandante recovers **only** that payment.
- Timeout of the project: contratista recovers **only** the boleta.

The boleta is **global** (default 10% of the whole project, configurable in bps) and stays locked until the last partida is done. One partida is funded at a time. Contract amounts are fiat/UF; sats are quoted when funding.

Unwind is **not** a bank boleta: Bitcoin cannot see whether the wall was built. The contractor’s defence is small partidas and stopping work if there is no recepción.

## Build

```bash
cargo test --workspace
cargo run -p hbp-cli -- --help
```

Binary name: `hbp`.

## CLI sketch

Two directories, one per party:

```bash
# mandante
hbp --dir .m init --network regtest --role mandante
hbp --dir .m new --unit USD --bond-bps 1000 --t-project 1800000000
hbp --dir .m add-partida --desc Radier --amount 1500 --plazo 1700000000
hbp --dir .m add-partida --desc Muros --amount 500 --plazo 1710000000
hbp --dir .m offer                         # writes .m/00-offer.json

# contratista
hbp --dir .c init --network regtest --role contratista
hbp --dir .c accept .m/00-offer.json      # writes .c/01-accepted.pending.json

# mandante countersigns
hbp --dir .m commit .c/01-accepted.pending.json

# contratista imports the signed contract
hbp --dir .c import .m/contracts/<id>/01-accepted.json

# both sign a sats quote (BTC price in the contract unit)
hbp --dir .m quote --btc-price 80000 --fx-note "manual"
hbp --dir .c accept-quote .m/contracts/<id>/02-quote.json
hbp --dir .m accept-quote .c/contracts/<id>/02-quote.json   # mandante imports the fully signed quote

hbp --dir .m addresses
hbp --dir .c status
```

`verify-funding` checks a raw funding transaction against the quoted amounts (rejects a malicious partida amount). `unwind` builds the script-path timeout tx after `T`.

Keys are **plaintext** in `.hbp/identity.json`. Toy only.

## Crates

| crate | role |
|---|---|
| `hbp-core` | contract JSON, state machine, nonce journal |
| `hbp-bitcoin` | Taproot descriptors, MuSig2 key-path, CLTV unwind, funding checks |
| `hbp-cli` | file protocol |

## Not in this MVP

Tor, DHT, Android, late arbiter, rolling the boleta into the next 2-of-2 without returning it, watchtowers, mainnet.
