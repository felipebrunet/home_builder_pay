# home_builder_pay

[Español](README_ES.md)

Peer-to-peer Bitcoin escrow for **work packages** plus a **performance bond**. Two parties — the principal (`mandante`) and the contractor (`contratista`) — lock funds in a P2WSH 2-of-2. There is no server. `hbp` never holds seeds; wallets sign PSBTs (Blue, Electrum, Sparrow, Ledger, Trezor).

This is an MVP: desktop CLI, **regtest/signet**, files and PSBTs passed by hand. No Tor, no DHT. The principal picks **hold** (UTXO sits forever) or **burn** (pre-signed 100% fee after T). New protocol: [docs/P2WSH.md](docs/P2WSH.md). MuSig2/Taproot is frozen on branch `musig-mode`.

Full protocol, architecture, roadmap, and **where the last session left off**: [docs/PROJECT.md](docs/PROJECT.md) (start at section 0). That document is currently in Spanish. Mined Signet happy path: [docs/SIGNET_HAPPY_PATH.md](docs/SIGNET_HAPPY_PATH.md). Two-PC Signet (Sparrow): [docs/SIGNET_TWO_PCS.md](docs/SIGNET_TWO_PCS.md). Watch-only + atomic PSBT (Blue/Electrum): [docs/BLUE_FUNDING.md](docs/BLUE_FUNDING.md).

Current milestone: P2WSH **hold / burn** coordinator. Test UI: `cargo run -p hbp-ui` → http://127.0.0.1:3847.

## Protocol (short)

One P2WSH UTXO; installment equals the bond:

```
wsh(sortedmulti(2, A/*, B/*))
```

- **Hold:** one signature each at funding (`m/84'`). No agreement → UTXO sits forever.
- **Burn:** sign the burn first (`m/48'`, nLockTime=T, OP_RETURN + 100% fee), then funding.
- **Coop:** both `m/48'` keys pay the agreed address.

## Build

```bash
cargo test --workspace
cargo run -p hbp-cli -- --help
# throwaway test UI (localhost): cargo run -p hbp-ui
# then open http://127.0.0.1:3847
```

Binary name: `hbp`.

## CLI sketch

`hbp` holds no seeds. Two directories, one per party:

```bash
hbp --dir .mh init --network signet --role mandante
hbp --dir .mh cosigner Vpub...             # m/48' — share this
hbp --dir .mh watch-import --xpub vpub...  # m/84' — local only
hbp --dir .mh new --mode hold --sats 5000 --fee 500
hbp --dir .mh offer

hbp --dir .ch init --network signet --role contratista
hbp --dir .ch cosigner Vpub...
hbp --dir .ch watch-import --xpub vpub...
hbp --dir .ch accept .mh/00-offer.json

hbp --dir .mh import .ch/01-accepted.json
hbp --dir .mh fund --mine .mh/05-coin.json --peer .ch/05-coin.json
hbp --dir .mh combine-fund mine.signed.psbt peer.signed.psbt
hbp --dir .mh coop --dest tb1q... --fee 200
hbp --dir .mh combine-coop mine.signed.psbt peer.signed.psbt
```

Test UI: `cargo run -p hbp-ui` → http://127.0.0.1:3847

## Crates

| crate | role |
|---|---|
| `hbp-core` | hold/burn terms and state |
| `hbp-bitcoin` | `wsh(sortedmulti(2))`, funding/burn/coop PSBTs |
| `hbp-cli` | coordinator (xpubs in, PSBTs out) |

## License

[MIT](LICENSE)

## Not in this MVP

Tor, DHT, Android, rolling the bond into the next 2-of-2 without returning it, watchtowers, mainnet. Next: two laptops on signet with the file protocol.
