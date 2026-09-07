# P2WSH 2-de-2 — protocolo (hold / burn)

Fuente de verdad del on-chain **nuevo**. Taproot + MuSig2 queda congelado en la rama `musig-mode`. El código en `crates/` todavía implementa MuSig2 hasta que se porte esto.

`hbp` es coordinador: xpubs in, PSBT out. **Nunca** mnemonic, xprv ni secreto. Las firmas viven en Blue, Electrum, Sparrow, Ledger o Trezor.

## Decisiones

| # | Tema | Decisión |
|---|---|---|
| 1 | Script | Un solo descriptor, los dos modos: `wsh(sortedmulti(2, A/*, B/*))`. Nada de miniscript `after(T)`, nada de OP_TRUE, nada de MuSig2. |
| 2 | Modo | Lo propone el **mandante** en el offer. El contratista acepta el paquete o no entra. No se cambia después de fondear. |
| 3 | Hold | 1 firma cada uno (`m/84'`). Si no hay acuerdo el UTXO queda **indefinido** (hace falta 2-de-2 para moverlo). |
| 4 | Burn | 2 firmas cada uno: primero quema `m/48'`, después funding `m/84'`. Pasado T se publica OP_RETURN 0 + 100 % fee. |
| 5 | Montos | Partida = boleta en cada etapa. Un UTXO. Obra 100 / boleta 10 → 10 etapas de 10+10. MAD simétrico. |
| 6 | xpubs | `m/48'` se comparte (cosigner). `m/84'` es watch-only **local**; al peer solo va el coin (outpoint, sats, script, change). |
| 7 | Árbitro | Fuera. |
| 8 | Wallets | Fondeo y coop: Blue, Electrum, Sparrow, Ledger, Trezor. Quema: las mismas; Blue a veces rechaza fee 100 % → esa firma en Sparrow (mismo seed/device). |

No hay `before(T)` en Bitcoin: el 2-de-2 nunca se apaga solo. Hold = deadlock. Burn = tx prefirmada que **compite** con un coop posterior; gana la que se mine primero.

## Descriptor

Signet / testnet:

```text
wsh(sortedmulti(2,
  [mandante]m/48'/1'/0'/2'/0/*,
  [contratista]m/48'/1'/0'/2'/0/*
))
```

Mainnet: `m/48'/0'/0'/2'`. Electrum con seed nativo (no BIP39) exporta la xpub de su wallet 2-de-2; Sparrow la usa tal cual. Mismo seed en Electrum + Ledger → BIP39 y el path de arriba.

Singlesig de la plata (no se comparte): `m/84'/1'/0'` (o el path que ya tenga la hot wallet).

## Offer

```text
modo: hold | burn
si burn: T = unix del deadline de esa etapa
partida_sats == boleta_sats
```

El contratista ve modo, montos y T y acepta o rechaza.

## Fondeo (los dos modos)

Una tx, dos inputs, **un** output:

```text
in  mandante     10   (singlesig)
in  contratista  10   (singlesig)
out P2WSH        20
    change M, change C
```

SegWit: el txid del funding se conoce **antes** de firmar (el witness no entra).

### Hold — 1 ronda

```text
1. Datos   m/48' al peer; coin m/84' (xpub m/84' no)
2. PSBT    funding unsigned (ambos la derivan)
3. Firma   m/84' cada uno, en paralelo
4. Combine → broadcast
```

### Burn — 2 rondas, orden obligatorio

```text
funding:  in M+C → out P2WSH 20

quema:    nLockTime = T
          in  P2WSH 20
          out OP_RETURN "hbp-burn"  0
          fee 20
```

```text
1. Datos     igual que hold; con eso se arman funding y quema
2. Ronda A   firman la quema (m/48'), paralelo; se verifica 2-de-2 completa
3. Ronda B   recién ahí firman el funding (m/84')
4. Broadcast funding. Guardar la quema firmada (cualquiera la publica a T)
```

Sin quema 2-de-2 completa, `hbp` no combina el funding.

Poner valor en OP_RETURN no: 0 sats de marcador; el 100 % es fee (relé y miner).

## Después del fondeo

| | Hold | Burn |
|---|---|---|
| Acuerdo | PSBT 2-de-2, 1 firma `m/48'` cada uno; outputs a gusto (pago / 10-10) | igual |
| Silencio | UTXO indefinido | pasado T, 0 firmas: se publica la quema |
| ¿Ladrón? | no (hace falta 2-de-2) | no (outputs ya fijos) |

## Qué tiene cada `hbp` local

| Dato | ¿Al peer? |
|---|---|
| xpub `m/48'` | sí |
| xpub `m/84'` | no |
| coin + change de esta tx | sí |
| modo + T | sí (van en el offer) |
| PSBT quema firmada | sí, si modo burn (ambos la necesitan para publicarla) |

## Wallets

| | Fondeo `m/84'` | Coop `m/48'` | Quema |
|---|---|---|---|
| Sparrow | singlesig | 2-de-2 BIP48 | sí |
| Ledger / Trezor | vía Sparrow | vía Sparrow, policy 2-de-2 | Approve fee alto |
| Blue | wallet standard | Vault 2-de-2 | a veces no; fallback Sparrow |
| Electrum | sí (PSBT) | multisig nativo | sí, con aviso de fee |

## Por qué no las alternativas

- **OP_TRUE / miniscript `after(T)`:** 1 firma, pero Blue no gasta ese script; el script no fija outputs; miners Core no reescriben un barrido a 100 % fee; el witness no se ve hasta el primer spend.
- **Taproot `multi_a`:** no suma firmas de funding vs este P2WSH; Electrum/Blue peor para el cierre.
- **Prefirmar dos quemas 50/50:** más rondas, mismo palo. Una quema 100 % basta.
- **Árbitro:** 99 % no hay tercero que vio el muro y firme PSBT.
