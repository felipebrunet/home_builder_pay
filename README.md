# home_builder_pay

[Español](README_ES.md)

Peer-to-peer Bitcoin escrow for **work packages** plus a **performance bond**. Two parties — the principal (`mandante`) and the contractor (`contratista`) — lock funds in a Taproot 2-of-2 (MuSig2). There is no server.

This is an MVP: desktop CLI, **regtest/signet**, files passed by hand. No Tor, no DHT. An optional arbiter exists as a Taproot leaf, jointly named by both parties before funding — not picked by the offeror in the listing.

Full protocol, architecture, roadmap, and **where the last session left off**: [docs/PROJECT.md](docs/PROJECT.md) (start at section 0). That document is currently in Spanish.

Current milestone: **MVP-0** plus mined catalog (**136 PASS / 6 human-only**). Dispute **policy** (unwind default; optional MAD / arbiter slot) is set by the offeror; the arbiter *person* is named later by both: [docs/DISPUTE.md](docs/DISPUTE.md). Catalog: [docs/SCENARIOS.md](docs/SCENARIOS.md). Run everything: `scripts/run_catalog.sh`. Unwind index: [docs/REGTEST_SCENARIOS.md](docs/REGTEST_SCENARIOS.md).

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
hbp --dir .m identity                 # public_key only — that is what the offer carries
# hbp --dir .m identity --backup      # YOUR secret; restore later with init --secret HEX
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

# optional, only if --dispute arbiter: both name the same pubkey before addresses exist
# hbp --dir .m propose-arbiter --pubkey 02...
# hbp --dir .c accept-arbiter .m/contracts/<id>/03-arbiter.json
# hbp --dir .m accept-arbiter .c/contracts/<id>/03-arbiter.json

hbp --dir .m addresses
hbp --dir .c status

# unsigned funding PSBT (escrow amounts exact; fee from change). Sign with Core:
#   bitcoin-cli -rpcwallet=hbp_mandante walletprocesspsbt <psbt>
#   bitcoin-cli -rpcwallet=hbp_contratista walletprocesspsbt <psbt>
hbp --dir .m fund --m-outpoint TXID:VOUT --m-sats N --m-prev ADDR --m-change ADDR \
  --c-outpoint TXID:VOUT --c-sats N --c-prev ADDR --c-change ADDR

# MuSig2 close across two laptops (files). Same-machine demo: coop-close --peer-dir
hbp --dir .m coop-propose --kind partida --partida 1 --outpoint TXID:VOUT --sats N --dest ADDR
hbp --dir .c coop-sign .m/04-coop.json
hbp --dir .m coop-finish .c/04-coop.json
```

`verify-funding` checks a raw funding transaction against the quoted amounts (rejects a malicious package amount). `unwind` builds the script-path timeout transaction after `T`.

Keys are **plaintext** in `.hbp/identity.json`. Toy only. Do not use on mainnet. The other party never sees that file: they get a compressed pubkey inside `00-offer.json` (the equivalent of an xpub for a **single** key, not HD). Restore with `hbp init --secret HEX`.

## Crates

| crate | role |
|---|---|
| `hbp-core` | contract JSON, state machine, nonce journal |
| `hbp-bitcoin` | Taproot descriptors, MuSig2 key-path, CLTV unwind, funding checks |
| `hbp-cli` | file protocol |

## License

[MIT](LICENSE)

## Not in this MVP

Tor, DHT, Android, rolling the bond into the next 2-of-2 without returning it, watchtowers, mainnet. Next: two laptops on signet with the file protocol.
