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

Isolated Tor hidden services are **not** a DHT by themselves. Pasting the other `.onion` into Avanzado still works (and is the fallback). The product path is **name search** after both press **Conectarme**:

1. Each app listens on `127.0.0.1:3848` (or the next free port). Tor maps `HiddenServicePort 80 → 127.0.0.1:<that port>` → `xxx.onion:80`.
2. **Publicar / Buscar** uses the normalized work name (`casa norte` = `Casa Norte`).
3. The overlay `STORE`s / `FIND_VALUE`s on the TCP Kademlia graph (`hbp-work:{normalized}` SHA-256) **and** publishes the same `{name, onion, role}` JSON to a public HTTPS topic board through Tor SOCKS (**ntfy.sh**, then **ntfy.envs.net**). Topic id is a hash of the name — not a secret.
4. On a hit, the contratista **auto-bootstraps** that onion (Kademlia `PING`) and can receive `Offer` / send `Accept` without copying files.
5. **Avanzado → código de respaldo** is onion paste if the board or Tor is blocked. `HBP_DHT_BOOTSTRAP` and `net-peers.json` (last seen onions) are also tried after connect.

Onion connect timeout is **40s** (8s was too tight for a new HS circuit). Lookup / announce / send run on a background thread so the window stays usable (“Pensando…”).

Work-name topics are public. Do not put keys there. The xpub stays local (`watch.json`); only protocol messages (`Offer` / `Accept` / `Commit`) go to the peer.

## Tor on Windows

Documented in [WINDOWS.md](WINDOWS.md). Short version:

- Expert Bundle `tor.exe` next to `home_builder_pay.exe`, or `TOR_BINARY`, or PATH.
- GUI **Conectar red (Tor + DHT)** finds or **downloads** the official Expert Bundle, then **spawns our own Tor + Hidden Service**. Status when the onion exists: *Conectado. Ya puedes ser encontrado.*
- First connect may take a minute (download ~20 MB + bootstrap). Cache: `%LOCALAPPDATA%\home_builder_pay\tor\` (Windows) or `~/.local/share/home_builder_pay/tor/` (Linux).
- Spawn uses dedicated SOCKS around `19050` and `{works}/tor/torrc` (`HiddenServicePort 80 → 127.0.0.1:<DHT port>`).
- Tor Browser SOCKS **9150** is outbound-only fallback if spawn fails. It does not make you findable.
- Env (advanced): `HBP_SKIP_TOR_DOWNLOAD=1`, `TOR_BINARY`, `HBP_TOR_SOCKS`.
- Without Tor the DHT still speaks on localhost (how the unit tests run). That is **not** the product WAN path.

## Messages

`NetMessage` wraps `hbp-core` (`Offer`, `SignedContract`, `Quote`) plus `Artifact` JSON for bitcoin files. Same bytes as USB / Signal.

## Verified vs not

| Piece | This environment |
|---|---|
| Kademlia 2- and 3-node overlays on `127.0.0.1` | **tested** (`cargo test -p hbp-net`) |
| Name key normalization + announce/lookup | **tested** |
| Rendezvous topic + ntfy JSON parse | **unit-tested** (no live ntfy/Tor in this VM) |
| `DELIVER` inbox | **tested** |
| Offer → accept → commit signatures | **tested** (`hbp-app` protocol) |
| SOCKS5 client + torrc writer + `tor.exe` search | **unit-tested** (no live Tor here) |
| Two Windows PCs, name search over live Tor | **not re-run in this Linux VM** — Felipe’s previous run needed onion paste; that path is now the fallback |
| PSBT / broadcast wizard | **not in this slice** |

## Env

| Variable | Meaning |
|---|---|
| `HBP_TOR_SOCKS` | `host:port` SOCKS5 (default `127.0.0.1:9050`) |
| `HBP_TOR_CONTROL` | control port (default `9051`) |
| `HBP_TOR_COOKIE` | path to `control_auth_cookie` |
| `TOR_BINARY` | `tor.exe` / `tor` |
| `HBP_DHT_BOOTSTRAP` | comma-separated `host:port` |
| `HBP_WORKS` | works directory (Tor datadir lives under it) |
