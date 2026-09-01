# Signet — camino feliz (minado)

Ensayo en la Signet **global** (no un signet privado). Dos puntas en la misma laptop: Chrome mandante (`.ms`) y Firefox contratista (`.cs`). Hot wallets: Electrum Signet Native SegWit (un `vpub` cada una). `hbp` no habla con Electrum; el xpub es watch-only **local**.

Montos de la UI de prueba: 5 USD + 5 USD, boleta 10 %, precio 100 000 USD/BTC → **5 000 sats** por partida, boleta **1 000**. Fee de fondeo default 2 000 (a medias). Recepción: fee 200.

No uses el demo de 60 k USD. Carpetas `.m` / `.c` son **regtest**; Signet = `.ms` / `.cs`.

Guías de procedimiento: [BLUE_FUNDING.md](BLUE_FUNDING.md) (watch-only + PSBT; Blue o Electrum), [SIGNET_TWO_PCS.md](SIGNET_TWO_PCS.md) (Sparrow). Checkpoint: [PROJECT.md](PROJECT.md) §0.

## Qué se minó

Contrato `6522e891222b61b0d1bd805781e58bb5f7de7626f4470272891e1f7fad53912c`.

**Fondeo** (una tx, dos inputs, boleta + P1):

- txid [`53ac882091ea59e6478a3d0b74f9211543fb3860de786b855bd0174d5ac7b833`](https://mempool.space/signet/tx/53ac882091ea59e6478a3d0b74f9211543fb3860de786b855bd0174d5ac7b833)
- vout 0: boleta 1 000 sats (`tb1p…`)
- vout 1: partida 1 5 000 sats
- vout 2–3: vueltos a cada Electrum

**Recepción P1** (MuSig2 key-path, solo `hbp`): 5 000 − 200 fee = **4 800** al Receive del contratista. Payout `c027db3fd0593091e1e1ef66927e10ac78d3f86b979592a88e70ba8fa6956447`.

**Boleta:** sigue unspent en `:0`. Pagar una partida **no** la suelta. Unwind del contratista (script path, `nLockTime` = `t_project` = 1790045246 ≈ **2026-09-22 02:47 UTC**) se firmó con CLI y **no** se publica acá (es un spend vivo). Alternativa antes de T: `coop-propose --kind bond`.

Partida 2 quedó `amount_agreed`, sin fondear.

## Lecciones

- El selector “Red” de `hbp-ui` solo aplica en `init`. Una identidad regtest pinta `bcrt1…` aunque el `vpub` sea de Signet. Carpetas nuevas: `.ms` / `.cs`.
- Esplora: `mempool.space/signet/api` a veces no contesta (timeout). Default: **blockstream.info**, fallback mempool. El scan recorre gap 20 receive + 20 change; un GET fallido aborta todo.
- Electrum sirve igual que Blue para xpub + firmar el PSBT de fondeo. Redeem/unwind = `hbp`.
- La UI de prueba no leía el UTXO de `status` al proponer el pago; el outpoint ya está en `state.json` tras `verify-funding`. La address de cobro sí es a mano (Receive nueva).

## No repetir con estas carpetas

`.ms` / `.cs` son la demo de 21 días. Ensayos nuevos (unhappy, plazos de 2 h) = **otras** `--dir`.
