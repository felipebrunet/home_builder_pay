# Política de disputa (opcional)

La **política** la propone el oferente en `hbp new`. El contratista la acepta o no con el resto del contrato. No se puede cambiar después del fondeo: el árbol Taproot (y por tanto la address) depende de ella.

Quién es el árbitro **no** va en esa oferta. Eso lo nombran **los dos** más tarde, cuando ya se aceptaron y van a generar los UTXO.

Default: **`unwind`** — lo que ya está minado en los E2E.

## Tres políticas

| `policy` | On-chain | Quién decide si no hay acuerdo |
|---|---|---|
| `unwind` | Tras T, M recupera la partida; C recupera la boleta | El reloj |
| `mad` | Igual, más un UTXO chico simétrico. Si nadie coopera tras T, ese UTXO queda **improductivo** (hoja NUMS) | El reloj + quema del stake |
| `arbiter` | Tras T, `árbitro+M` o `árbitro+C` pueden gastar. Tras T+ventana, unwind de último recurso | Un humano que **ambos** nombran **antes de fondear** |

No se mezclan MAD y árbitro en el MVP. No hay tercer firmante en el **key path**: el 2-de-2 sigue siendo MuSig2(M,C). El árbitro es una **hoja** del árbol, ciega hasta que se usa.

## CLI

```bash
hbp --dir .m new --unit USD --bond-bps 1000 --t-project <unix> --dispute unwind

hbp --dir .m new ... --dispute mad --mad-bps 100
# 1% de los sats de la partida 1, cada uno. Output on-chain = 2 × eso.

# Solo el *slot*. Nadie elige todavía a la persona.
hbp --dir .m new ... --dispute arbiter --arbiter-window 15
# window en segundos (default 7 días). T2 = plazo + window.

# Después de accept/commit, cualquiera propone; el otro contrafirma el mismo pubkey.
hbp --dir .m propose-arbiter --pubkey 02abc...
hbp --dir .c accept-arbiter .m/contracts/<id>/03-arbiter.json
hbp --dir .m accept-arbiter .c/contracts/<id>/03-arbiter.json   # el primero importa
```

`accept` no puede cambiar `dispute`. Si el JSON no coincide con el offer, `commit` rechaza (mismo `terms()`).

`hbp addresses` no imprime boleta/partida hasta que A está nombrado por ambos. Sin eso no hay UTXO que fondear.

## MAD

- `mad_bps` ∈ 1..=500 (0,01 %–5 % de la partida 1, **por parte**).
- Quote calcula `mad_sats`. `hbp addresses` imprime `mad bcrt1p…`.
- Fondeo inicial: tercera salida con `2 * mad_sats`.
- Key path: ambos firman y se lo reparte (`coop-close --kind mad --pay-sats ...`).
- Si no cooperan después de `t_project`: la única hoja es `pk(NUMS) && after(T)` y NUMS no tiene clave → **quema**.

No va a ninguna wallet de autor.

## Árbitro

Difícil en la práctica, no imposible. Un muro en un pueblo remoto no se lo va a juzgar un administrador de una comunidad Bitcoin en California: aunque sepa firmar un PSBT, no vio la obra. El protocolo no inventa un juez; solo deja el hueco si **las dos partes** conocen a alguien:

- imparcial respecto de esa obra (idealmente que sepa de construcción, o al menos que ambas acepten su criterio),
- y capaz de firmar un PSBT Taproot.

Por eso el mandante **no** pone un amigo en el aviso. En la publicación solo dice “esta obra admite árbitro”. El *quién* se acuerda al mismo tiempo que los UTXO, con dos firmas Schnorr tagged `hbp-arbiter` sobre `{contract_id, pubkey}`. Si no se ponen de acuerdo en la persona, no fondean (o no debieron haber aceptado la política).

Árbol de la partida (la boleta es simétrico):

```
key path: MuSig2(M,C)
hojas:
  after(T):     A && M
  after(T):     A && C
  after(T+win): M solo   (último recurso; en la boleta, C solo)
```

Hasta T, A no puede nada. A solo no puede vaciar. Si A no aparece, al cabo de la ventana vuelve el unwind.

`hbp unwind` con política arbiter usa la hoja de **último recurso** (T2), no la de A+M.

A no es M ni C. Cambiar A después de fondear es imposible: cambia la address. El `contract_id` **no** incluye a A (la nomination vive en `03-arbiter.json` / `state.json`).

Catálogo de escenarios (incl. A que desaparece, MAD improductivo, boleta con A): [SCENARIOS.md](SCENARIOS.md) §§8–12.

## E2E on-chain de MAD/árbitro

Corridos en `scripts/regtest_catalog.sh`: fondeo MAD 3 salidas + split / MAD improductivo; `hbp arbiter-close --with am|ac` (P1 a C, P1 a M, split 80/20, boleta a M); A desaparece → unwind T2. Índice con ticks: [SCENARIOS.md](SCENARIOS.md).
