# Política de disputa (opcional)

La define el **oferente** en `hbp new`. El contratista la **acepta o no** con el resto del contrato. No se puede cambiar después del fondeo: el árbol Taproot (y por tanto la address) depende de ella.

Default: **`unwind`** — lo que ya está minado en los E2E.

## Tres políticas

| `policy` | On-chain | Quién decide si no hay acuerdo |
|---|---|---|
| `unwind` | Tras T, M recupera la partida; C recupera la boleta | El reloj |
| `mad` | Igual, más un UTXO chico simétrico. Si nadie coopera tras T, ese UTXO queda **improductivo** (hoja NUMS) | El reloj + quema del stake |
| `arbiter` | Tras T, `árbitro+M` o `árbitro+C` pueden gastar. Tras T+ventana, unwind de último recurso | Un humano nombrado **en el offer** |

No se mezclan MAD y árbitro en el MVP. No hay tercer firmante en el **key path**: el 2-de-2 sigue siendo MuSig2(M,C). El árbitro es una **hoja** del árbol, ciega hasta que se usa.

## CLI (oferente)

```bash
hbp --dir .m new --unit USD --bond-bps 1000 --t-project <unix> --dispute unwind

hbp --dir .m new ... --dispute mad --mad-bps 100
# 1% de los sats de la partida 1, cada uno. Output on-chain = 2 × eso.

hbp --dir .m new ... --dispute arbiter --arbiter 02abc... --arbiter-window 15
# window en segundos (default 7 días). T2 = plazo + window.
```

`accept` no puede cambiar `dispute`. Si el JSON no coincide con el offer, `commit` rechaza (mismo `terms()`).

## MAD

- `mad_bps` ∈ 1..=500 (0,01 %–5 % de la partida 1, **por parte**).
- Quote calcula `mad_sats`. `hbp addresses` imprime `mad bcrt1p…`.
- Fondeo inicial: tercera salida con `2 * mad_sats`.
- Key path: ambos firman y se lo reparte (`coop-close --kind mad --pay-sats ...`).
- Si no cooperan después de `t_project`: la única hoja es `pk(NUMS) && after(T)` y NUMS no tiene clave → **quema**.

No va a ninguna wallet de autor.

## Árbitro

El pubkey va en el contrato. No es el mandante ni el contratista.

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

## Qué falta para E2E on-chain de MAD/árbitro

Los descriptores y el contrato ya existen (tests de address distinta y hoja NUMS verdes). Falta un script de fondeo con 3 outputs (MAD) y un flujo A+M firmando juntos. No bloquea el default `unwind`.
