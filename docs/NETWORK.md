# Tor + DHT (Windows v1 scaffolding)

No application server. The contract is the same JSON the file protocol already speaks (`00-offer.json`, accept, quote, coop, fee-burn plan). Transport is substitutable.

Crate: `hbp-net`.

## Messages

`NetMessage` wraps `hbp-core` types (`Offer`, `SignedContract`, `Quote`) plus an `Artifact` blob for bitcoin-specific files (`04-coop.json`, `06-feeburn.json`, `05-coin.json`) so `hbp-net` does not depend on `hbp-bitcoin`.

Fallback constant: `file` — USB / Signal / mail still work.

## Tor (point-to-point)

Once the peer onion is known, connect through SOCKS5.

- `TorConfig` / `HBP_TOR_SOCKS` (default `127.0.0.1:9050`)
- `socks5_connect` — real SOCKS5 CONNECT (no auth), host can be `.onion`
- `tor_status` — TCP probe of the SOCKS port
- Windows search path for `tor.exe`: see [WINDOWS.md](WINDOWS.md)

**Scaffolding vs ready:** handshake code is real. There is no control-port parser, no in-process hidden-service publisher, and no `hbp listen` daemon in this PR.

## DHT (discovery)

Mainline BitTorrent DHT is UDP. Tor here is a TCP SOCKS proxy. A production overlay must be **TCP Kademlia** (or an onion service directory), not raw mainline.

`DhtNode` is that API:

- `dht_key("hbp-work:{name}")` = SHA-256
- `announce_work` / `lookup_work`
- `announce_offer_blob`
- `merge` — how two laptops would sync a store over Tor later

**Ready:** types, put/get, unit tests, GUI “announce locally”.

**Not ready:** WAN replication, peer ping, NAT traversal, a process that speaks the wire format on the internet.

## Honest status

| Piece | Status |
|---|---|
| Shared JSON messages | done |
| SOCKS5 client | done (untested against a live Tor in this environment) |
| Tor Expert Bundle layout | documented |
| Local DHT store | done |
| Onion listen + DHT WAN | not started |
| File fallback | still the reliable path |
