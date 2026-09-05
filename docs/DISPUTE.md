# Política de disputa

La **política** la propone el oferente en `hbp new`. El contratista la acepta o no con el resto del contrato. No se puede cambiar después del fondeo: el árbol Taproot (y por tanto la address) depende de ella.

## Producto (2026-09-04)

**Fee-burn t1/t2 es el único camino sin acuerdo.** El cierre cooperativo MuSig2 sigue cuando ambas partes firman. Formas exactas: [FEE_BURN.md](FEE_BURN.md).

```bash
hbp --dir .m new --unit USD --bond-bps 1000 --work-name Casa \
    --dispute fee-burn --t1 <unix> --t2 <unix>
```

- `t1`: 50% de la partida viva **y** 50% de la boleta se consumen como fee de minero; la otra mitad queda en un output de continuación.
- `t2` (> t1): el 50% restante de ambos, igual.
- Boleta = 10% del principal (`bond_bps = 1000`). En la GUI cada partida = boleta.
- Árbitro: **apagado** (`ARBITER_ENABLED = false`). Sin nominación ni UI de producto.

No es NUMS. No es anyone-can-spend. Las txs de quema se prefirman (MuSig2 key-path) después de conocer el outpoint de fondeo.

## Políticas (enum)

| `policy` | On-chain | Quién decide si no hay acuerdo | Producto |
|---|---|---|---|
| `fee_burn` | t1/t2 miner-fee 50%+50% (continuación en t1) | El reloj + txs prefirmadas | **default** |
| `unwind` | Tras T, M recupera la partida; C recupera la boleta | El reloj | legacy / catálogo |
| `mad` | Igual, más un UTXO chico. Tras T, hoja NUMS | Reloj + quema NUMS del stake | legacy |
| `arbiter` | Tras T, `A+M` o `A+C`. Tras T+ventana, unwind | Un humano nombrado antes de fondear | **off** en la UI |

JSON viejo sin campo `dispute` deserializa como `unwind` (catálogo minado). Ofertas nuevas de producto emiten `fee_burn`.

## CLI legacy

```bash
hbp --dir .m new --unit USD --bond-bps 1000 --t-project <unix> --dispute unwind

hbp --dir .m new ... --dispute mad --mad-bps 100
# 1% de los sats de la partida 1, cada uno. Output on-chain = 2 × eso.

# Solo el *slot*. Nadie elige todavía a la persona. Producto: no usar.
hbp --dir .m new ... --dispute arbiter --arbiter-window 15
# window en segundos (default 7 días). T2 = plazo + window.

# Después de accept/commit, cualquiera propone; el otro contrafirma el mismo pubkey.
hbp --dir .m propose-arbiter --pubkey 02abc...
hbp --dir .c accept-arbiter .m/contracts/<id>/03-arbiter.json
hbp --dir .m accept-arbiter .c/contracts/<id>/03-arbiter.json   # el primero importa

# Tras T: A+C paga al contratista (A+M es --with am y --dir del mandante).
hbp --dir .c arbiter-close --kind partida --partida 1 --with ac \
  --arbiter-dir .a --outpoint txid:vout --sats N --dest bcrt1… --fee 200
```

`accept` no puede cambiar `dispute`. Si el JSON no coincide con el offer, `commit` rechaza (mismo `terms()`).

`hbp addresses` no imprime boleta/partida hasta que A está nombrado por ambos (solo policy=arbiter).

## MAD (legacy)

- `mad_bps` ∈ 1..=500 (0,01 %–5 % de la partida 1, **por parte**).
- Quote calcula `mad_sats`. `hbp addresses` imprime `mad bcrt1p…`.
- Fondeo inicial: tercera salida con `2 * mad_sats`.
- Key path: ambos firman y se lo reparte (`coop-close --kind mad --pay-sats ...`).
- Si no cooperan después de `t_project`: la única hoja es `pk(NUMS) && after(T)` y NUMS no tiene clave → **quema**.

No va a ninguna wallet de autor.

## Árbitro (legacy, UI off)

Difícil en la práctica, no imposible. El protocolo no inventa un juez; solo deja el hueco si **las dos partes** conocen a alguien imparcial y capaz de firmar un PSBT Taproot. El mandante **no** pone un amigo en el aviso.

Árbol de la partida (la boleta es simétrico):

```
key path: MuSig2(M,C)
hojas:
  after(T):     A && M
  after(T):     A && C
  after(T+win): M solo   (último recurso; en la boleta, C solo)
```

El `contract_id` **no** incluye a A (la nomination vive en `03-arbiter.json` / `state.json`).

Catálogo de escenarios: [SCENARIOS.md](SCENARIOS.md) §§8–12.

## E2E on-chain de MAD/árbitro (legacy)

Corridos en `scripts/regtest_catalog.sh` y `scripts/regtest_remainders.sh` (todo: `scripts/run_catalog.sh`). Índice: [SCENARIOS.md](SCENARIOS.md) (136 PASS / 6 humano). Fee-burn **aún no** tiene E2E minado.
