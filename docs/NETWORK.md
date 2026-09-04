# Tor + DHT (Windows v1)

No application server. The contract is the same JSON the file protocol already speaks. Transport is substitutable; **product networking is Tor + a TCP Kademlia DHT**. File-passing remains fallback/dev.

Crate: `hbp-net`. Product network: **Signet only** (see [WINDOWS.md](WINDOWS.md)).

## Why not mainline DHT

BitTorrent mainline DHT is **UDP**. Tor (Expert Bundle / this app) is a **TCP SOCKS5** proxy. UDP does not go through that proxy. A Tor-suitable overlay has to be TCP (or an onion-service directory). HBP speaks its own Kademlia on TCP:

| RPC | Purpose |
|---|---|
| `PING` | learn node id + listen address |
| `FIND_NODE` | k closest peers to a 256-bit id |
| `FIND_VALUE` | value or closest peers |
| `STORE` | put a record (work announce, offer blob) |
| `DELIVER` | point-to-point `NetMessage` (offer / accept / …) |

Framing: `u32` big-endian length + JSON. `.onion` destinations always use SOCKS5. `127.0.0.1` is always direct (tests + the local hidden-service target).

## How two machines find each other

There is **no public HBP bootstrap cloud** yet. That is the honest limitation.

1. Each app listens on `127.0.0.1:3848` (or the next free port).
2. Tor maps `HiddenServicePort 80 → 127.0.0.1:<that port>` so the peer address is `xxx.onion:80`.
3. Share **one** bootstrap address with the other laptop (the onion, or `127.0.0.1:port` on a LAN/dev box):
   - GUI field “bootstrap”, or
   - `HBP_DHT_BOOTSTRAP=abcd.onion:80`
4. **Bootstrap** → `PING` + iterative `FIND_NODE`.
5. **Anunciar esta obra** → local `STORE` + `STORE` to the k closest peers.
6. The other side **Buscar** by work name (`hbp-work:{name}` SHA-256). Iterative `FIND_VALUE` walks the overlay.

After a hit you have the publisher onion and can `DELIVER` the offer over Tor.

Three or more nodes work the same way: you only need a path of bootstraps (A knows B, C knows B → C can find a value stored on A).

## Tor on Windows

Documented in [WINDOWS.md](WINDOWS.md). Short version:

- Expert Bundle `tor.exe` next to `home_builder_pay.exe`, or `TOR_BINARY`, or PATH.
- GUI **Tor + onion** attaches first if SOCKS is already up (`ADD_ONION`, or reuse `{works}/tor/hidden_service/hostname`). It only spawns `tor.exe` when nothing is listening.
- Spawn path writes `{works}/tor/torrc` with `HiddenServiceDir` + `HiddenServicePort 80 → 127.0.0.1:<DHT port>`.
- Env: SOCKS `HBP_TOR_SOCKS` (default `127.0.0.1:9050`), control `HBP_TOR_CONTROL` / `HBP_TOR_COOKIE`.
- Without Tor the DHT still speaks on localhost (how the unit tests run). That is **not** the product WAN path.

## Messages

`NetMessage` wraps `hbp-core` (`Offer`, `SignedContract`, `Quote`) plus `Artifact` JSON for bitcoin files. Same bytes as USB / Signal.

## Verified vs not

| Piece | This environment |
|---|---|
| Kademlia 2- and 3-node overlays on `127.0.0.1` | **tested** (`cargo test -p hbp-net`) |
| `DELIVER` inbox | **tested** |
| SOCKS5 client + torrc writer + `tor.exe` search | **unit-tested** (no live Tor here) |
| Two Windows PCs over real Tor / Signet | **not run in this VM** — use the GUI steps above |
| Public bootstrap nodes | **none** — you share an onion |

## Env

| Variable | Meaning |
|---|---|
| `HBP_TOR_SOCKS` | `host:port` SOCKS5 (default `127.0.0.1:9050`) |
| `HBP_TOR_CONTROL` | control port (default `9051`) |
| `HBP_TOR_COOKIE` | path to `control_auth_cookie` |
| `TOR_BINARY` | `tor.exe` / `tor` |
| `HBP_DHT_BOOTSTRAP` | comma-separated `host:port` |
| `HBP_WORKS` | works directory (Tor datadir lives under it) |
