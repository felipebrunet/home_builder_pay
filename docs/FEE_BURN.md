# Fee-burn t1 / t2 (product no-agreement path)

Burn is the **only** no-agreement path. Cooperative MuSig2 key-path remains when both parties agree. This is not NUMS and not “anyone-can-spend”.

## Why presigned key-path (and not a script leaf)

Bitcoin Script cannot constrain transaction *outputs* without a covenant (CTV / APO are not active on Bitcoin).

| Tempting leaf | What actually happens |
|---|---|
| `pk(NUMS) && after(T)` (legacy MAD) | Coins are unspendable forever. Miners never receive them. Product forbids this. |
| `after(T)` anyone-can-spend | A third party can sweep to themselves. That is a bounty, not a miner-fee burn. |

So the enforceable shape is a **MuSig2 key-path transaction both parties sign after the funding outpoint is known**, with `SIGHASH_DEFAULT` binding the outputs. Either party (or anyone holding the signed hex) can broadcast after `nLockTime`.

Until those signatures exist, the UTXO can only move by a later cooperative close. **Do not start work until the burn chain is armed.**

## Funding address (fee-burn policy)

```
tr(musig(M, C))     # key-path only — no unilateral unwind leaf
```

Same descriptor for the bond UTXO and each partida UTXO. Legacy `unwind` still uses `pk(M)&&after(T)` / `pk(C)&&after(T_project)`.

## Exact transactions

Let `N` be the locked value (bond sats **or** active partida sats). Require `N ≥ 2 × 546`.

`half = floor(N / 2)`  
`t1_fee = N − half`   (odd sat goes to the t1 miner fee)

### t1

```
nLockTime = t1
nSequence = ENABLE_LOCKTIME_NO_RBF
vin[0]    = funding outpoint (N)
vout[0]   = continuation, same tr(musig(M,C)), `half` sats
fee       = t1_fee                          # 50% consumed as miner fees
```

The continuation is required: one transaction cannot both give 50% to miners and leave 50% locked until t2 without an output. SegWit txid excludes the witness, so t2 can be built against t1’s txid before signatures exist.

### t2

```
nLockTime = t2          # must be > t1; both unix (≥ 500_000_000)
nSequence = ENABLE_LOCKTIME_NO_RBF
vin[0]    = t1 vout[0] (`half`)
vout[0]   = 0-value OP_RETURN "hbp-feeburn"   # consensus needs ≥1 output
fee       = half                              # remaining 50% → miners
```

Bond and the live partida each have their **own** t1/t2 chain. Same user-defined deadlines.

## Cooperative close

A different key-path spend of the **same** funding UTXO (reception or cancel). If it confirms first, the burn txs are invalid. After t1 has confirmed, coop can still spend the continuation before t2 (recover the remaining half by agreement).

**Agreed % (GUI).** When both parties agree, they set one **contratista %** (mandante = 100 − that). The same ratio is applied to **partida 1** and **boleta** as two sequential MuSig2 spends (`build_split_key_spend_tx`: one input, two outputs). 100% to the contratista stays a single-output spend. Broadcast is in-app Esplora, same as today. Fee-burn remains the **only** path if they do not agree.

## CLI / files

```
hbp --dir .m new --unit USD --bond-bps 1000 --work-name Casa \
    --dispute fee-burn --t1 <unix> --t2 <unix>
hbp --dir .m fee-burn-plan --kind bond --outpoint TXID:VOUT --sats N
hbp --dir .m fee-burn-plan --kind partida --partida 1 --outpoint TXID:VOUT --sats N
```

Writes `06-feeburn.json` (**unsigned** hex). Signing reuses the existing MuSig2 coop rounds; a dedicated arming command is not in this PR.

## Honest status

**Ready:** split math, tx builders, shape asserts, key-path-only descriptors, unit tests, CLI plan, GUI t1/t2 fields.

**Not ready:** exchanged signatures on `06-feeburn.json`, mined t1/t2 on regtest/signet, watchtower / auto-broadcast.
