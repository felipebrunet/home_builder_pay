# home_builder_pay

[Español](README_ES.md)

Peer-to-peer Bitcoin escrow for **work packages** plus a **performance bond**. Two parties — the principal (`mandante`) and the contractor (`contratista`) — lock funds in a Taproot 2-of-2 (MuSig2). There is no server.

This is an MVP: desktop CLI + **native product GUI** (Windows-first, egui). Regtest/signet. The throwaway localhost `hbp-ui` is **not** the product.

Full protocol, architecture, roadmap, and **where the last session left off**: [docs/PROJECT.md](docs/PROJECT.md) (start at section 0). Fee-burn shapes: [docs/FEE_BURN.md](docs/FEE_BURN.md). Windows exe + Tor: [docs/WINDOWS.md](docs/WINDOWS.md). Tor/DHT: [docs/NETWORK.md](docs/NETWORK.md). Mined Signet happy path: [docs/SIGNET_HAPPY_PATH.md](docs/SIGNET_HAPPY_PATH.md).

Current milestone: **fee-burn t1/t2 foundation + native GUI + Tor/DHT scaffolding**, on top of MVP-0 and the mined catalog (**136 PASS / 6 human-only**). Product dispute path is **fee-burn** (50% then 50% as miner fees). Cooperative MuSig2 stays. Arbiter UI is hard-off. Legacy unwind/MAD/arbiter remain for the catalog: [docs/DISPUTE.md](docs/DISPUTE.md).

## Protocol (short)

Two Taproot outputs, never mixed:

```
# product (fee-burn): key-path only; no-agreement = presigned t1/t2 fee-burn chain
bond     = tr(musig(M,C))
package  = tr(musig(M,C))

# legacy unwind (catalog):
bond     = tr(musig(M,C), pk(C) && after(T_project))
package  = tr(musig(M,C), pk(M) && after(T_package))
```

In the code and CLI those outputs are still named `boleta` (bond) and `partida` (work package).

- Cooperative close: both parties sign with MuSig2. On-chain it looks like a normal payment.
- No agreement: at user `t1`, 50% of the **live package and the bond** are consumed as miner fees (continuation output holds the rest); at `t2` the remaining 50% of both is likewise consumed. Exact shapes: [docs/FEE_BURN.md](docs/FEE_BURN.md).
- Bond is **10% of total principal** (`bond_bps = 1000`). The GUI splits the total so **each stage equals the bond**.

This is **not** a bank performance bond: Bitcoin cannot see whether the wall was built. The contractor’s defense is small packages and stopping work if there is no acceptance.

## Build

```bash
cargo test --workspace
cargo run -p hbp-cli -- --help
# product GUI (native): cargo run -p hbp-app
# throwaway test UI (localhost, not the product): cargo run -p hbp-ui
# then open http://127.0.0.1:3847
```

Binaries: `hbp` (CLI), `home_builder_pay` (native GUI). Windows: [docs/WINDOWS.md](docs/WINDOWS.md).

## CLI sketch

Two directories, one per party:

```bash
# principal
hbp --dir .m init --network regtest --role mandante
# optional: encrypt identity.json (any passphrase; toy, no strength check)
# hbp --dir .m --passphrase ab init --network regtest --role mandante
hbp --dir .m identity                 # public_key only — that is what the offer carries
# hbp --dir .m identity --backup      # YOUR secret; restore later with init --secret HEX
hbp --dir .m stage-plan --total 100 --bond-bps 1000   # 10 stages of 10, bond 10
hbp --dir .m new --unit USD --bond-bps 1000 --work-name Casa \
    --dispute fee-burn --t1 1700000000 --t2 1800000000
# legacy catalog: --dispute unwind --t-project 1800000000
hbp --dir .m add-partida --desc Foundation --amount 10 --plazo 1700000000
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

# Blue (both parties): local watch-only, then share one coin each — never the xpub
hbp --dir .m watch-import --xpub vpub...          # YOUR Blue xpub; stays on this machine
hbp --dir .m coins                                # Esplora (signet: blockstream, then mempool.space)
hbp --dir .m offer-coin --outpoint TXID:VOUT      # writes 05-coin.json (send that file)
hbp --dir .m fund --mine contracts/<id>/05-coin.json --peer 05-coin-from-peer.json
# each Blue signs the .psbt (do not broadcast) →
hbp --dir .m fund-combine mine-signed.psbt peer-signed.psbt   # hex; either side broadcasts

# Core/Sparrow still works with explicit outpoints:
#   bitcoin-cli -rpcwallet=hbp_mandante walletprocesspsbt <psbt>
hbp --dir .m fund --m-outpoint TXID:VOUT --m-sats N --m-prev ADDR --m-change ADDR \
  --c-outpoint TXID:VOUT --c-sats N --c-prev ADDR --c-change ADDR

# MuSig2 close across two laptops (files). Same-machine demo: coop-close --peer-dir
hbp --dir .m coop-propose --kind partida --partida 1 --outpoint TXID:VOUT --sats N --dest ADDR
hbp --dir .c coop-sign .m/04-coop.json
hbp --dir .m coop-finish .c/04-coop.json
```

`verify-funding` checks a raw funding transaction against the quoted amounts (rejects a malicious package amount). `unwind` builds the script-path timeout transaction after `T`.

Keys default to **plaintext** in `.hbp/identity.json`. Pass `--passphrase` (or `HBP_PASSPHRASE`) to encrypt; there is no minimum length. Toy only. Do not use on mainnet. The other party never sees that file: they get a compressed pubkey inside `00-offer.json`. Restore with `hbp init --secret HEX`.

## Crates

| crate | role |
|---|---|
| `hbp-core` | contract JSON, state machine, nonce journal, stage=bond helpers |
| `hbp-bitcoin` | Taproot descriptors, MuSig2 key-path, fee-burn t1/t2 txs, CLTV unwind, funding checks |
| `hbp-cli` | file protocol (`hbp`) |
| `hbp-net` | Tor SOCKS + DHT scaffolding (same JSON as files) |
| `hbp-app` | native product GUI (`home_builder_pay`) |
| `hbp-ui` | throwaway localhost test wizard |

## License

[MIT](LICENSE)

## Not in this MVP

Android APK, mainnet, arbiter UX, WAN DHT, in-process Tor hidden service, mined fee-burn E2E, Felipe’s local Signet unhappy tests. Tor + DHT are **scaffolded** (see [docs/NETWORK.md](docs/NETWORK.md)), not a finished overlay.
