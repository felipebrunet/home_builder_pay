# Protocol: boxes, payments, backups

Back to the [README](../README.md). See also [ARCHITECTURE.md](ARCHITECTURE.md).

## How Monero pays a stage

One personal wallet per machine, then one shared box per job, then one transaction in and one transaction out. The box is an ordinary stagenet address. Its spend key exists only as two FROST shares.

**1. Personal wallet.** Cuenta or Billetera runs **Crear billetera de stagenet**. The address to fund is on Billetera. A fresh wallet scans from 40 blocks back; a restored seed scans from the block height stored in its backup; **Mirar 200 bloques más atrás** walks further. Spent outputs are pruned by key image, so the balance matches after a restore. The mempool does not count. Outputs stay locked for about 10 blocks. The other PC creates its own seed. Do not copy `semilla.txt`.

**2. One 2-of-2 box when the two match.** The client is FROST index 1 and pays the funding fee. The contractor is index 2. The DKG context includes the job id. Neither side holds the full spend scalar. The shared view is sent once and stored in the share file.

**3. Funding one stage.** **Confirmar y fondear** builds one transaction whose output is `2 × guarantee` to the box. Both personal wallets contribute. Encerrada is set when the scan sees that transaction in a block. If the node rejects the tx, **Empezar el fondeo de nuevo** clears the stuck session and picks fresh outputs.

**4. Paying the stage.** The pot is `2 × guarantee`. The contractor receives the agreed percent of the payment plus their own guarantee. The client receives the rest. The fee comes from the client's remainder first. At 100% the contractor receives the pot minus the fee. The client's output is still there (Monero needs two outputs), but with 0 XMR: up to 0.2.7 it carried 1 piconero of dust. The same applies to personal sends: an exact send or **Usar el máximo** (free balance minus the fee) leaves a 0-amount change output.

**5. Unlock gate.** Monero only spends an output 10 blocks after the block that holds it. The box learns the funding block from its scan; `caja::traba_partida` (funding block + 10 vs the node tip) decides whether **Avisar que terminé** and **Aceptar X% y pagar** are enabled. While locked both apps show “Podés marcarla terminada en ~N bloques (~M min, bloque X)”, or “esperando que el fondeo entre en un bloque” if the funding is not in a block yet. The tip is refreshed every minute, so the gate opens without a restart.

Peers on 0.2.7 still build the old 1-piconero split and cannot co-sign a 0.2.8 payment proposal (they reject the 0-amount split); 0.2.8 still co-signs the old split. Upgrade both sides.

Both shares have to sign. One share is not a transaction.

## Prices in USD (since 0.2.10)

The client posts the job amount and the guarantee per stage in **USD** (`Moneda::Usd`, cents). The XMR of each stage is fixed when it is locked:

- **Price source.** `konstruado-motor::cotizacion`: Kraken `XMRUSD`, Bitfinex `tXMRUSD`, CoinGecko `simple/price` and CoinPaprika `xmr-monero` queried in parallel (8 s per source, first valid answer wins), over HTTPS (rustls + webpki roots). With Tor on it goes through the SOCKS proxy (Orbot on Android, the bundled tor on desktop); remote DNS. If every source fails over Tor, clearnet is used only when the user asks for it (one-off button, or an opt-in "always" setting stored in `precio-ajustes.json`). A manually typed price (`fuente: "manual"`) is the last resort and is fixed like any other. The last quote is cached with its timestamp; the UI shows each source's failure reason, and **Proponer encerrar** refuses without a quote from the last 30 minutes.
- **Stagenet.** Stagenet XMR has no value; the mainnet price is used as a reference and the UI says so.
- **Fixed per stage.** The proposer stores a `PrecioFijado` in the stage (USD cents, USD/XMR rate, source, timestamp and the resulting piconero amount). The other side sees it before **Confirmar y fondear** and accepts by confirming; a warning shows the drift from the current price. Both funding sides read the piconero amount from the shared job state (`Obra::piconero_partida`), never reconvert, so different local prices cannot make `check_dest` reject the funding. Simultaneous proposals converge by the existing tie-break; a copy from an older peer without the price never erases it.
- **Display.** Before funding: “USD X ≈ Y XMR al precio actual”. After: “Y XMR (USD X al dd/mm hh:mm, precio Z)”. Percent payments split the fixed XMR.
- **Old jobs** keep the legacy unit (1 unit = 0.00002 XMR, a guarantee of 2000 is 0.04 XMR per side). The new fields are `serde(default)`. Peers on 0.2.9 do not understand USD jobs: upgrade both sides.

Atomic spending and the multisig/FROST box path stay separate in the code; do not mix them.

## Personal seed (view words)

**Billetera → Respaldos y recuperación → Ver las 25 palabras** shows the personal single-sig mnemonic after a confirmation. The words are the standard Monero English seed (same derivation as Feather / monero-wallet-cli: spend from the 25 words, view = keccak(spend)). Job boxes are **not** recovered from this seed — they need the FROST shares in the full `.kbak`. The restore height is shown next to the words. Copy is allowed; the clipboard is cleared after 60 s if it still holds the seed (on Android the clip is marked `EXTRA_IS_SENSITIVE`). Android sets `FLAG_SECURE` while the words are on screen. The same block also shows the personal address and private view key for a view-only check without exposing the seed.

## Full backup

**Billetera → Respaldos y recuperación → Exportar respaldo completo** writes one file, `konstruado-respaldo-<YYYY-MM-DD>.kbak`:

| Bytes | Field |
|---|---|
| 8 | magic `KSTRBAK\0` |
| 1 | format version (1) |
| 1 | KDF id (1 = Argon2id v1.3) |
| 4 + 4 + 1 | memory KiB, passes, lanes (default 64 MiB, 3, 1) |
| 16 | salt |
| 24 | XChaCha20-Poly1305 nonce |
| rest | ciphertext of a JSON document; the 59-byte header is the AAD |

The JSON holds the full `estado.json` profile, the seed backup text and its height, each job's share (`xmr::ShareBackup`) with the box's scan start, and the node URL. Password: at least 8 characters, nothing stored. Code: `xmr_joint::sobre` (envelope) and `crates/konstruado-motor/src/respaldo.rs` (contents, restore), shared by desktop and Android.

Restore (welcome screen **Restaurar desde respaldo**, or the same section in Billetera): decrypt, check the seed, check every share against its job and role (same checks as a single share import), show a summary. If the device already has an account, seed or shares, an explicit **Reemplazar lo de este equipo** is required. Then everything is written to `restaurar.tmp/`, renamed to `restaurar.listo/`, and the app restarts; at startup the old files move to `previo-<date>/` and the new ones take their place (resumable if cut). The wallet and each box scan from the stored height. Newer deal progress comes back from the other person through the room.

The app shows when the last full backup was made and reminds you to export again after a new job or a new box (`ultimo-respaldo.json`).

## Withdrawing an offer

Before 0.2.6, **Quitar oferta** only deleted the offer from the local DHT store. The board key merges by union, so the next gossip from the contractor or the room put it back.

Now each offer carries `retiro_hash = SHA-256(secret)`. The secret is derived from the client's X25519 secret and the offer id, so the same device can rebuild it after a restart. Withdrawing publishes a `RetiroOferta` that reveals the secret under the DHT key `clave_retiradas()`. Every peer checks the reveal against the hash and drops the offer from its board, from its store and from what it gossips. A forged withdrawal does not match the hash and is ignored.

Offers published by 0.2.5 have no hash. For those, a withdrawal signed by the same client id is accepted. Withdrawals are capped at 512 and kept newest first. `estado.json` gains a `retiradas` list with a serde default, so older files still load. Older peers store the unknown key unchanged and keep relaying it.
