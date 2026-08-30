# home_builder_pay

[Español](README_ES.md)

Peer-to-peer Bitcoin escrow for **work packages** plus a **performance bond**. Two parties — the principal (`mandante`) and the contractor (`contratista`) — lock funds in a Taproot 2-of-2 (MuSig2). There is no server.

This is an MVP: desktop CLI, **regtest/signet**, files passed by hand. No Tor, no DHT, no arbiter yet.

Full protocol, architecture, roadmap, and **where the last session left off**: [docs/PROJECT.md](docs/PROJECT.md) (start at section 0). That document is currently in Spanish.

Current milestone: **MVP-0** plus mined regtest demos (happy path and contractor-abandons). Walkthroughs (Spanish): [docs/REGTEST_HAPPY_PATH.md](docs/REGTEST_HAPPY_PATH.md), [docs/REGTEST_CONTRACTOR_ABANDONS.md](docs/REGTEST_CONTRACTOR_ABANDONS.md).

## Protocol (short)

Two Taproot outputs, never mixed:

```
bond     = tr(musig(M,C), pk(C) && after(T_project))
package  = tr(musig(M,C), pk(M) && after(T_package))
```

In the code and CLI those outputs are still named `boleta` (bond) and `partida` (work package).

- Cooperative close (acceptance of the work): both parties sign with MuSig2. On-chain it looks like a normal payment.
- Timeout of a work package: the principal recovers **only** that payment.
- Timeout of the project: the contractor recovers **only** the bond.

The bond is **global** (default 10% of the whole project, configurable in basis points) and stays locked until the last package is done. One package is funded at a time. Contract amounts are fiat/UF; sats are quoted at funding time.

Unwind is **not** a bank performance bond: Bitcoin cannot see whether the wall was built. The contractor’s defense is small packages and stopping work if there is no acceptance.

## Build

```bash
cargo test --workspace
cargo run -p hbp-cli -- --help
```

Binary name: `hbp`.

## CLI sketch

Two directories, one per party:

```bash
# principal
hbp --dir .m init --network regtest --role mandante
hbp --dir .m new --unit USD --bond-bps 1000 --t-project 1800000000
hbp --dir .m add-partida --desc Foundation --amount 1500 --plazo 1700000000
hbp --dir .m add-partida --desc Walls --amount 500 --plazo 1710000000
hbp --dir .m offer                         # writes .m/00-offer.json

# contractor
hbp --dir .c init --network regtest --role contratista
hbp --dir .c accept .m/00-offer.json      # writes .c/01-accepted.pending.json

# principal countersigns
hbp --dir .m commit .c/01-accepted.pending.json

# contractor imports the signed contract
hbp --dir .c import .m/contracts/<id>/01-accepted.json

# both sign a sats quote (BTC price in the contract unit)
hbp --dir .m quote --btc-price 80000 --fx-note "manual"
hbp --dir .c accept-quote .m/contracts/<id>/02-quote.json
hbp --dir .m accept-quote .c/contracts/<id>/02-quote.json   # principal imports the fully signed quote

hbp --dir .m addresses
hbp --dir .c status
```

`verify-funding` checks a raw funding transaction against the quoted amounts (rejects a malicious package amount). `unwind` builds the script-path timeout transaction after `T`.

Keys are **plaintext** in `.hbp/identity.json`. Toy only. Do not use on mainnet.

## Crates

| crate | role |
|---|---|
| `hbp-core` | contract JSON, state machine, nonce journal |
| `hbp-bitcoin` | Taproot descriptors, MuSig2 key-path, CLTV unwind, funding checks |
| `hbp-cli` | file protocol |

## License

[MIT](LICENSE)

## Not in this MVP

Tor, DHT, Android, late arbiter, rolling the bond into the next 2-of-2 without returning it, watchtowers, mainnet.
