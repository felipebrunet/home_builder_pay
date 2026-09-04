# home_builder_pay — documento de proyecto

Cliente Bitcoin P2P para acordar **partidas de obra** y una **boleta de garantía**, sin servidor, sin notario y sin banco. El caso de uso que lo justifica es construcción (o cualquier trabajo por hitos) en contextos de poca institucionalidad o de ticket que no paga abogados. El mecanismo no es exclusivo de casas.

Este archivo es la fuente de verdad de producto, protocolo, arquitectura, roadmap y plan de trabajo. El `README.md` de la raíz es el atajo de build/CLI.

**Si retomas en una sesión nueva: lee la sección 0 y el checklist de “listo / no listo”. El código en `crates/` es la implementación de MVP-0.**

Última actualización: 2026-09-04 (un clic: download/spawn Tor + Hidden Service, encontrable).

---

## 0. Dónde estamos (para retomar)

Checkpoint de sesión. Actualizar **esta sección** cada vez que se cierre un hito, no solo el roadmap de abajo.

**Decisiones de producto locked (2026-09-04) — pisan el texto más abajo si choca:**

1. **Disputa / no-acuerdo:** la quema (fee-burn) es el **único** camino sin acuerdo. El cierre cooperativo MuSig2 sigue cuando ambos firman.
2. **Mecánica:** en el `t1` que elige el usuario se consume el **50%** del pago de la partida viva **y** el 50% de la boleta como **fee de minero** (no NUMS). En `t2`, el 50% restante de ambos, igual. `t1` deja un output de continuación con la mitad. Formas exactas: [FEE_BURN.md](FEE_BURN.md).
3. **Boleta:** 10% del principal total (`bond_bps = 1000`). Principal y boleta siguen la misma regla de quema.
4. **Partidas en la GUI:** el total se parte de modo que **cada partida = la boleta** (ej. 100 → boleta 10 → 10 partidas de 10).
5. **Árbitro:** apagado (`ARBITER_ENABLED = false`). Sin nominación ni UI. El CLI legacy del catálogo todavía acepta `--dispute arbiter`.
6. **Red Windows v1:** Tor (punto a punto, SOCKS5 / onion) + **DHT Kademlia TCP** (descubrimiento real entre peers). Archivos = fallback/dev. No hay bootstrap público: se comparte un onion. [WINDOWS.md](WINDOWS.md), [NETWORK.md](NETWORK.md).
7. Redeem = MuSig2 vía `hbp`. Fondeo = wallet externa / PSBT.
8. **Red de producto: Signet.** La GUI no ofrece mainnet ni un selector que salga de Signet. Regtest queda para CLI / tests / catálogo.

`hbp-ui` (localhost:3847) **no** es el producto. El producto es `home_builder_pay` (egui). Android APK = fase posterior; no bloquea este hito.

| Campo | Valor |
|---|---|
| Fecha | 2026-09-04 |
| Rama | `cursor/fee-burn-windows-gui-6be3` (PR sobre `master`) |
| Hito | **Producto: fee-burn t1/t2 + GUI Signet-only + Tor + DHT Kademlia TCP** (catálogo unwind/MAD/árbitro legacy) |
| Verificar al abrir | `cargo test --workspace`. GUI: `cargo run -p hbp-app`. UI de prueba (no producto): `cargo run -p hbp-ui` → http://127.0.0.1:3847 |
| Nodo Bitcoin local | `/home/felipe/projects/btc_clients` — Core 31.1 regtest, RPC `:18443`. Los scripts usan `bitcoin-cli`; `hbp` no embebe el cliente. Signet no usa este nodo. |
| Origin | `git@github.com-hbp:felipebrunet/home_builder_pay.git` (MIT, público) |
| Sesión Signet | Carpetas locales `.ms` / `.cs` (gitignore). Relato: [SIGNET_HAPPY_PATH.md](SIGNET_HAPPY_PATH.md). |

### Historial reciente (changelog)

| Cuándo | Qué | Dónde |
|---|---|---|
| 2026-08-30 | MVP-0: Taproot 2-de-2, CLI por archivos | `crates/` |
| 2026-08-30 | Camino feliz minado (60k USD, 2×30k, boleta 20k) | [REGTEST_HAPPY_PATH.md](REGTEST_HAPPY_PATH.md) |
| 2026-08-30 | Unhappy paths: abandono, no-firma, cancel coop, stops-after-P1, split 80/20, sin boleta, cancel acordado tras P1 | [REGTEST_SCENARIOS.md](REGTEST_SCENARIOS.md) #1–8 |
| 2026-08-30 | Políticas opcionales `mad` (hoja NUMS) y `arbiter` (hojas A+M / A+C). **No** a wallet del autor. | [DISPUTE.md](DISPUTE.md), `d8f35b3` |
| 2026-08-30 | El **quién** del árbitro no va en el offer: ambos lo firman antes de fondear (`propose-arbiter` / `accept-arbiter`) | este checkpoint |
| 2026-08-30 | Catálogo de 142 escenarios (obra de 2 partidas, incl. A que desaparece) | [SCENARIOS.md](SCENARIOS.md) |
| 2026-08-30 | Quote con tag `hbp-quote` y verificación al accept | `9be02b1` |
| 2026-08-30 | MAD 3 salidas + `hbp arbiter-close` (A+M / A+C) minados | `scripts/regtest_catalog.sh`, `6b04665` |
| 2026-08-30 | Remainders: address mala, RBF, reorg, keys perdidas, carreras T2 | `scripts/regtest_remainders.sh`, `a81c0f9` |
| 2026-08-30 | `hbp fund` PSBT montos exactos; `coop-propose` / `coop-sign` / `coop-finish` (sin `--peer-dir`) | `crates/hbp-bitcoin/src/fund.rs`, CLI |
| 2026-08-30 | Identidad: se comparte pubkey comprimido (no xpub). Restore `init --secret`. `hbp identity [--backup]` | CLI |
| 2026-08-30 | **Opción C:** se mantiene el descriptor actual (`tr(musig(M,C), hojas)`). Los 142 no se recortan. No se baja a `wsh` ni a `multi_a` en lugar de MuSig2. | este checkpoint |
| 2026-08-30 | `identity.json` se puede cifrar con passphrase (Argon2id + XChaCha20). Sin política de fortaleza. Tests siguen en claro. | CLI `--passphrase` / `HBP_PASSPHRASE` |
| 2026-08-30 | Guía Signet **global** en dos PCs (Sparrow fondea; `hbp` redima). Faucet; montos chicos (no el demo 60k). | [SIGNET_TWO_PCS.md](SIGNET_TWO_PCS.md) |
| 2026-08-30 | Confirmación TTY (YES) en coop/unwind; `--yes` para scripts/UI. `nonces.json` se cifra con la misma passphrase. | CLI |
| 2026-08-30 | UI de pruebas `hbp-ui`: un rol, pasos 1–5. Fondeo = **un** hex de **una** tx con boleta+P1. | `crates/hbp-ui`, `d428b93` |
| 2026-08-30 | Feliz vía `POST /api/hbp` + wallets Core `hbp_mandante`/`hbp_contratista`: P1 paid + boleta minada. | `/tmp/hbp-ui-e2e` (no en git) |
| 2026-08-31 | Fondeo Blue **camino 2**: xpub watch-only local (nunca al peer); Esplora lista UTXOs; `05-coin.json` = un prevout; `fund --mine/--peer`; Blue/Electrum firma el PSBT; `fund-combine`. Atomicidad 2-in intacta. | `hbp-bitcoin/watch.rs`, CLI, [BLUE_FUNDING.md](BLUE_FUNDING.md) |
| 2026-09-01 | UI de prueba: red primero (Signet → `.ms`/`.cs`); log sticky; Esplora Blockstream + fallback mempool; paso 5 carga UTXO de `status`. | `hbp-ui` |
| 2026-09-01 | **Signet feliz minado**: fondeo 2-in, recepción P1 (4 800 al contratista), boleta 1 000 sigue locked. Unwind boleta prefirmado, locktime 2026-09-22. | [SIGNET_HAPPY_PATH.md](SIGNET_HAPPY_PATH.md) |
| 2026-09-04 | **Fee-burn t1/t2** + GUI nativa + Tor SOCKS + DHT **local** (primer PR). | [FEE_BURN.md](FEE_BURN.md) |
| 2026-09-04 | GUI **Signet-only**. DHT **Kademlia TCP** (2/3 nodos localhost). Tor spawn/`ADD_ONION` + torrc Windows. Sin bootstrap público; e2e Tor vivo no corrido en esta VM. | [NETWORK.md](NETWORK.md), [WINDOWS.md](WINDOWS.md) |
| 2026-09-04 | UX: plazos en fecha/hora local (no unix). Un botón **Conectar red**. SOCKS 9050 **y** 9150 (Tor Browser). Tema claro/oscuro. Contraste de campos. Flujo obra vs técnico. | `hbp-app`, [WINDOWS.md](WINDOWS.md) |
| 2026-09-04 | Un clic **encontrable**: descarga Expert Bundle oficial si falta, spawn HS, onion en el panel. 9150 = solo salida. | [NETWORK.md](NETWORK.md), [WINDOWS.md](WINDOWS.md) |

Default de **producto** = **`fee_burn`**. JSON viejo sin campo `dispute` sigue siendo `unwind` (catálogo). MAD y árbitro minados como legacy. Catálogo: **136 PASS**, **6 NO TEST** (humano). **0 FAIL**.

### Listo

**Producto**

- Disputa default de producto = **fee-burn t1/t2** (50% + 50% a miners). Coop MuSig2 si hay acuerdo. Unwind/MAD/árbitro = legacy del catálogo.
- Boleta **global** (`bond_bps = 1000` = 10%). Una partida viva a la vez. Bond **antes** de cualquier partida. GUI: cada partida = boleta.
- Montos en fiat/UF; sats al quote/fondeo.
- Red Windows v1: **Tor** p2p + **DHT Kademlia TCP** (`hbp-net`). Archivos = fallback. Sin servidor. GUI **Signet only**.
- Árbitro: **off** (`ARBITER_ENABLED = false`). Sin UI.
- GUI nativa: crate `hbp-app`, binario `home_builder_pay`. `hbp-ui` localhost no es el producto.

**Código**

- Workspace `hbp-core` / `hbp-bitcoin` / `hbp-cli` (binario `hbp`) / `hbp-net` / `hbp-app` (binario `home_builder_pay`). `hbp-ui` = wizard de prueba.
- Offer / accept / commit / import; JSON canónico; firmas BIP340 tagged `hbp-contract`.
- Quote de sats (ambos firman).
- Descriptores (**opción C, locked**): key-path MuSig2. Fee-burn (producto): `tr(musig(M,C))` sin hoja de unwind; la quema es una cadena prefirmada. Legacy unwind: boleta `tr(musig(M,C), pk(C)&&after(T_proyecto))`, partida `tr(musig(M,C), pk(M)&&after(T_partida))`. KeyAgg `[mandante, contratista]`.
- MuSig2 key-path (64 B) y unwind script-path (CLTV unix, witness 3 ítems). Sparrow/Blue firman el **fondeo** (singlesig → escrow). **No** firman la recepción/unwind: eso es BIP-327 + árbol, que esas wallets no implementan.
- `NonceJournal`: reutilizar seed aborta.
- CLI: `init [--secret]`, `identity [--backup\|--encrypt]`, `--passphrase` / `HBP_PASSPHRASE` (cifra identity.json **y** nonces.json; 4 letras vale; sin passphrase = claro). `--yes` salta el “type YES” en TTY. `new` default `--dispute fee-burn --t1 --t2`. `stage-plan`, `fee-burn-plan`.
- UI de pruebas: `cargo build -p hbp-cli -p hbp-ui && cargo run -p hbp-ui` → http://127.0.0.1:3847 (localhost; llama a `hbp --yes`). Selector de red primero: Signet → `.ms`/`.cs`; regtest → `.m`/`.c`. Log a la derecha (`sticky`). No es el producto final.
- Resto CLI: `new --dispute unwind\|mad\|arbiter`, `propose-arbiter`, `accept-arbiter`, `add-partida`, `offer`, `accept`, `commit`, `import`, `quote`, `accept-quote`, `addresses`, `status`, `verify-funding`, `watch-import`, `coins`, `offer-coin`, `fund` (`--mine`/`--peer` o flags viejos), `fund-combine`, `coop-propose` / `coop-sign` / `coop-finish`, `unwind`, `coop-close --peer-dir` (atajo misma máquina), `arbiter-close --with am\|ac --arbiter-dir`.
- Identidad = **una** clave secp256k1 por punta. Lo que ve el otro es el **pubkey comprimido** (33 B hex) dentro de `00-offer.json`. No es un xpub HD. El secreto no se pega ni se manda.
- `hbp fund`: PSBT sin firmar; salidas de escrow **exactas** (boleta + partida [+ MAD]); fee y change desde otros inputs. Dust &lt;546 se rechaza, no se pliega. stdout = PSBT en base64. `--partida-only`: solo el mandante. Camino Blue: `--mine` / `--peer` (archivos `05-coin.json`). Flags `--m-outpoint` siguen para Core/scripts.
- Watch-only **local**: `watch-import` (xpub/zpub/vpub o descriptor `wpkh`/`tr`) escribe `watch.json` (0600; se cifra si hay passphrase). `coins` pregunta a Esplora (default **blockstream.info**, fallback mempool.space; `--esplora` / `HBP_ESPLORA`). Gap 20+20. `offer-coin` emite un prevout. El peer **no** recibe el xpub. `fund-combine` junta las firmas y imprime hex. Blue/Electrum no firman MuSig2 ni el unwind.
- MuSig2 por archivos: `04-coop.json` (pubnonces + parciales, no el seed). El seed queda en `NonceJournal.pending[sighash]`. `coop-close --peer-dir` sigue para el demo en una máquina.
- `DisputePolicy::Arbiter { window_secs }` en el body; pubkey en `ArbiterNomination` (`03-arbiter.json`). Sin las dos firmas `hbp-arbiter` no hay addresses.
- MAD: tercera salida `2 * mad_sats`; key path coop split; tras T solo `pk(NUMS)&&after(T)`.
- `validate_funding_tx` rechaza monto de partida distinto al quote. `--partida-only` exige boleta ya fondeada.

**E2E minado** — unwind 1–8 [REGTEST_SCENARIOS.md](REGTEST_SCENARIOS.md); MAD + A+M/A+C + T2 [scripts/regtest_catalog.sh](../scripts/regtest_catalog.sh); races/reorg/RBF/keys [scripts/regtest_remainders.sh](../scripts/regtest_remainders.sh). Todo: `scripts/run_catalog.sh`. Catálogo 142 con ticks: [SCENARIOS.md](SCENARIOS.md) (136 PASS / 6 humano).

1. Feliz. 2. Abandono / ambos enojados. 4. Cancel coop. 5. Para tras P1 (timeout boleta). 6. Split 80/20. 7. Sin boleta (rechaza fondeo P1). 8. Cancel acordado tras P1.

### No listo / en pausa

- Signet **unhappy** (timeout partida, timeout boleta con plazo corto, cancel coop). El feliz ya está. Plazos de demo: **2–3 h** de CLTV, carpetas **nuevas** (no `.ms`/`.cs`).
- GUI nativa: obras, identidad por obra, offer/accept, t1/t2, tablero stage=bond, **Signet locked**, Tor+DHT overlay, backup. Falta: fondeo PSBT en la GUI, MuSig2 de recepción, armado fee-burn firmado, unhappy Signet.
- Overlay DHT verificado en localhost (2 y 3 nodos). **No** se corrió Tor vivo entre dos PCs en esta VM. No hay lista pública de bootstrap.
- Android, mainnet (producto), boleta que rueda. BIP39 no hace falta para `hbp` (backup = hex 256 bits / 64 chars). Electrum/Blue sí usan BIP39 para la hot wallet.
- 6 ítems del catálogo humano: [SCENARIOS.md](SCENARIOS.md).
- P2, unwind de boleta, MAD/árbitro **desde la UI de prueba**.
- No se cedió atomicidad del fondeo P1: **no** hay camino de dos envíos sueltos a boleta+P1.

### Cómo seguir en la próxima sesión

1. Leer esta sección 0, [FEE_BURN.md](FEE_BURN.md), [WINDOWS.md](WINDOWS.md).
2. Armar fee-burn: `hbp fee-burn-plan` produce txs **sin firmar**; falta el flujo MuSig2 de armado (`06-feeburn.json` firmado) y un E2E regtest que *mine* t1/t2.
3. Cruzar `home_builder_pay.exe` en una máquina Windows con Tor Expert Bundle (docs ya escritos; no corrido aquí).
4. Probar dos PCs Windows + Tor Expert Bundle + Signet (bootstrap = onion del otro). Esta VM no lo corrió.
5. Signet unhappy **no** es de este PR. No reusar `.ms`/`.cs` para plazos cortos. No cambiar opción C (Taproot + MuSig2 key-path). No mandar xpub al peer.

On-chain = **opción C** + fee-burn presigned (no covenant). Redeem = `hbp`. Fondeo = PSBT 2-in. Identidad **por obra** en la GUI.

---

## 1. Problema

Un mandante quiere pagar por hitos. Un contratista quiere no trabajar a merced de un “ya te pago después”. En un contrato tradicional (p.ej. administración de contrato en Copec) eso se resuelve con:

- un contrato escrito
- partidas con monto y plazo
- pagos contra recepción
- una boleta de garantía (~10%) que el mandante puede ejecutar si el contratista incumple

Bitcoin no ve si el radier está echado. Solo puede saber **quién firmó, cuánto se bloqueó y si ya pasó un plazo**. Toda la “justicia” está en el *unhappy path*.

Copiar Bisq 2-de-2 con quema simétrica no sirve: los montos son asimétricos (20k vs 2k) y el trabajo queda **hundido en el predio del mandante**. Copiar la boleta bancaria (“al timeout todo es del mandante”) tampoco: el mandante nunca firmaría la recepción y se quedaría con plata y boleta.

---

## 2. Decisiones cerradas

| # | Tema | Decisión |
|---|---|---|
| 1 | Disputa | Default de producto **fee-burn t1/t2** (50%+50% a miners). Coop MuSig2 si hay acuerdo. Unwind / MAD / árbitro = legacy. Árbitro **off** en la UI. Ver [DISPUTE.md](DISPUTE.md), [FEE_BURN.md](FEE_BURN.md). |
| 2 | Boleta | **Una, global**, `bond_bps = 1000` (10% del total). Se fondea una vez. **Una partida viva a la vez.** GUI: cada partida = boleta. |
| 3 | Moneda | Contrato en USD / UF / CLP / EUR / GBP / ARS / MXN / BRL / PEN / COP / UYU / BTC / SATS. Fiat: los sats se fijan **al quotear/fondear**. BTC/SATS: el monto ya está en esa unidad. |
| 4 | Red | **Sin servidor propio, siempre.** Windows v1: **Tor p2p** + **DHT Kademlia TCP**. Producto: **Signet only**. Archivos = fallback. |
| 5 | Lenguaje | **100% Rust** |
| 6 | On-chain (opción C) | Se **conserva** Taproot + MuSig2 key-path + hojas (`after(T)`, MAD NUMS, árbitro A&&M/A&&C). Catálogo 142 no transable. No se recorta a 2-de-2 `wsh`/Electrum para ganar Sparrow/Blue en el redeem. Fondeo: hot wallet externa (Sparrow/Blue/Core). Redeem: firmante que hable MuSig2 (hoy `hbp`; después Core/Ledger/Nunchuk si calzan). |

Cómo se “conectan”: Bitcoin no necesita un socket. Dos laptops que se pasan `00-offer.json` ya cierran un 2-de-2. Tor oculta IP cuando el contacto ya existe; la DHT es para *encontrar* extraños. No se finge un marketplace antes de minar una recepción en regtest.

Consecuencias:

- Con 20 partidas de 1 000 y boleta 10% de 20 000, el contratista tiene **2 000 locked** y el mandante **1 000** en la partida activa. Es esperado: la boleta no se pone 20 veces.
- El % se avisa en CLI si es &lt;5% del total, &gt;30% del total, o mayor que la partida actual.

---

## 3. Modelo funcional

### 3.1 Roles

- **Mandante**: pone el pago de la partida activa.
- **Contratista**: pone la boleta una vez y ejecuta la obra.
- **Árbitro** (opcional, difícil en la práctica): un tercero que ambas partes conocen, imparcial respecto de la obra y capaz de firmar un PSBT. La oferta solo abre el *slot*; el pubkey se nombra después (`propose-arbiter` / `accept-arbiter`) y entra en el árbol **antes** de fondear. No se puede agregar a un UTXO ya fondeado. No es un administrador remoto de Bitcoin que no vio el muro.

### 3.2 Unidades de trabajo

Un **proyecto** tiene N **partidas**. Cada partida tiene descripción, monto en la moneda del contrato y un plazo absoluto (unix time, CLTV).

La **boleta** es un porcentaje del *total* del proyecto, no de cada partida.

### 3.3 Lo que el protocolo garantiza (y lo que no)

| Situación | Pago de la partida | Boleta |
|---|---|---|
| Ambos firman recepción | → contratista | no se toca (se libera al cierre) |
| Ambos firman cancelar | → mandante | → contratista |
| Vence `T_partida`, nadie firma | mandante solo | intacta |
| Vence `T_proyecto` | (si quedó algo, mandante) | contratista solo |
| Mandante espera el timeout | recupera el pago; **no** la boleta | — |
| Contratista espera el timeout | **no** se lleva el pago | recupera la boleta al final |

Hueco que Bitcoin no tapa: si el contratista ya construyó en el terreno y el mandante no firma, al plazo el mandante recupera sats y se queda la obra. Defensa del contratista: **partidas chicas** y **parar** si no hay recepción. Eso debe decirse en la UI, no esconderse.

Esto **no** es una boleta bancaria. Tampoco es la quema de Bisq. Es el máximo sin un tercero.

### 3.4 Volatilidad

Los montos del contrato son fiat/UF. Los sats de la boleta se congelan al primer fondeo; los de cada partida, al fondear esa partida. Si BTC se mueve 40%, la boleta en USD ya no es el 10%. Hay que mostrarlo al quotear.

---

## 4. Protocolo on-chain

### 4.1 Dos UTXO Taproot, nunca mezclados

```
boleta   = tr(musig(M, C), pk(C) && after(T_proyecto))
partida  = tr(musig(M, C), pk(M) && after(T_partida))
```

- **Key path** (feliz): MuSig2 de mandante + contratista. On-chain parece un pago normal (Schnorr 64 bytes).
- **Script path** (unwind):
  - partida: después de `T_partida`, el mandante gasta solo.
  - boleta: después de `T_proyecto`, el contratista gasta solo.

Orden de claves MuSig2: siempre `[mandante, contratista]` (índice 0 y 1). No se ordenan lexicográficamente.

Cada partida tiene **otra address**: el plazo entra en el tweak.

### 4.2 Por qué no un solo UTXO

Si pago y boleta van juntos, un unwind unilateral o le da todo al mandante (trampa) o todo al contratista (robo), o exige transacciones pre-firmadas con fee de acá a seis meses (se pudren). Separados, el unwind se arma **al broadcast** con fee de mercado. Para obra de meses eso es mejor que el delayed-payout de Bisq.

### 4.3 Fondeo inicial (atómico)

Una transacción:

| | Qué |
|---|---|
| Input C | ≥ `bond_sats` |
| Input M | ≥ `partida_1_sats` + fees |
| Output 0 | script boleta, `bond_sats` exactos |
| Output 1 | script partida 1, `partida_sats` exactos |
| Change | solo a descriptores conocidos de cada uno |

Antes de firmar, **cada cliente verifica montos y scripts**. Un output extra que no sea change → rechazo. Un monto distinto al quote → rechazo. Eso cubre la clase de bugs de “el peer miente el fee/monto” (incidente Bisq).

Partidas 2…N: solo el mandante envía a la address Taproot de esa partida. El contratista verifica y recién ahí trabaja. Sin atomicidad de input del contratista: la boleta ya está locked.

### 4.4 Cierre cooperativo (MuSig2, BIP-327)

Dos rondas: nonces, luego firmas parciales, luego agregación. El sighash es Taproot key-path (`SIGHASH_DEFAULT`) sobre el UTXO de la partida (pago al contratista menos fee).

**Nonces:** un seed se registra en `NonceJournal` *antes* de usarlo. Reutilizar un `SecNonce` filtra la clave. El journal aborta con `NonceReused`.

La boleta **no se mueve** al pagar una partida intermedia. Se libera con otro MuSig2 al cerrar el proyecto, o el contratista la unwind después de `T_proyecto`.

### 4.5 Unwind

La tx lleva `nLockTime = T` y `nSequence` que habilita locktime (`ENABLE_LOCKTIME_NO_RBF`). Witness: `[schnorr_sig, script, control_block]`.

No hay tx pre-firmada. Quien unwind arma y paga el fee al momento.

### 4.6 Árbitro (hoja Taproot; persona fuera del offer)

`DisputePolicy::Arbiter { window_secs }` va en el body (lo acepta el contratista con el resto). El pubkey **no**. Después de `commit`, cualquiera propone `03-arbiter.json`; el otro contrafirma el mismo comprimido. Hasta que hay dos firmas tagged `hbp-arbiter`, `hbp addresses` no imprime boleta/partida.

Árbol (partida; boleta simétrico): key path MuSig2(M,C); hojas `A&&M` y `A&&C` después de T; unwind unilateral en T2 = T+ventana. Cambiar A **después** de fondear es imposible: cambia la address. Si no hay persona de confianza que sepa de la obra *y* de firmar un PSBT, no usen esta política.

### 4.7 Timelocks

- `after(n)` en miniscript = `OP_CLTV` (absoluto).
- Exigimos unix time (`n >= 500_000_000`) para hablar el idioma de un plazo de ejecución (“15 de marzo”), no bloques relativos.
- `T_proyecto` ≥ último plazo de partida (en el MVP se pide explícitamente; recomendación de producto: último plazo + 30 días).
- Extender un plazo de un UTXO ya fondeado: cooperar antes de T, o mover el UTXO a una address nueva (renew). No está en el CLI todavía.

CSV máximo (~15 meses) no se usa en el MVP.

---

## 5. Protocolo off-chain (archivos)

Sin red en el MVP. Cada parte tiene un directorio (`--dir`, default `.hbp`). Se pasan JSON por USB, Signal, carpeta compartida. El transporte es sustituible: el mismo mensaje viajará después por TCP o Tor sin cambiar el JSON.

```
<dir>/
  identity.json          # secret; opcionalmente cifrado (passphrase)
  nonces.json            # seeds MuSig2 consumidos
  draft.json
  00-offer.json
  CURRENT                # id del contrato activo
  contracts/<id>/
    01-accepted.json
    02-quote.json
    03-arbiter.json      # nomination conjunta (solo policy=arbiter)
    04-coop.json         # MuSig2 por archivos (pubnonces + parciales)
    state.json
```

Secuencia:

1. `init` — identidad (secp256k1, pubkey comprimida 33 bytes hex). `--passphrase` opcional cifra `identity.json`.
2. Mandante `new` + `add-partida` + `offer`. Firma BIP340 tagged `hbp-contract` sobre el JSON canónico del body **sin** clave del contratista.
3. Contratista `accept` — agrega su pubkey, firma el body completo. Sale `01-accepted.pending.json`.
4. Mandante `commit` — comprueba que los *terms* coinciden con la oferta, contrafirma el body completo.
5. Contratista `import` el `01-accepted.json`.
6. `quote` / `accept-quote` — ambos firman `bond_sats` y `partida_sats`. El archivo contrafirmado hay que devolvérselo a la otra punta (`accept-quote` importa si ya está firmado por mí).
6b. Si `dispute=arbiter`: `propose-arbiter` / `accept-arbiter` (cualquiera propone; el otro firma el mismo pubkey). Sin eso no hay addresses.
7. Fondeo on-chain (hoy: el usuario arma la tx; `verify-funding` valida hex crudo y actualiza estado).
8. Obra off-chain.
9. Recepción: MuSig2 (librería lista; CLI de rondas aún no).
10. Unwind: `hbp unwind --kind partida|bond` después de T.

`contract_id` = SHA256 del body canónico **con ambas pubkeys**.

JSON canónico: objetos con claves ordenadas, para que ambas puntas hasheen lo mismo.

---

## 6. Máquina de estados

**Proyecto:** `Offered → Accepted → Active → Closed | Cancelled`

**Partida** (como máximo una no-terminal a la vez):

```
Scheduled
  → AmountAgreed
    → Funding
      → Locked
        → ReceptionProposed → Paid
                            → Locked          (rechazo)
        → Unwound
```

Reglas:

- No se fondea N+1 si N no está `Paid` o `Unwound`.
- Quote con ambas firmas pasa las partidas `Scheduled` a `AmountAgreed`.
- Bond `Funded` + alguna partida `Funding/Locked` → proyecto `Active`.
- Liberar boleta exige todas las partidas terminales.
- Unwind de boleta → proyecto `Cancelled`.

---

## 7. Arquitectura técnica

Workspace Cargo, resolver 2, edition 2021.

```
home_builder_pay/
  Cargo.toml
  crates/
    hbp-core/       # cero Bitcoin: tipos, estado, nonces, montos
    hbp-bitcoin/    # Taproot, MuSig2, fee-burn, PSBT/tx, verify-everything
    hbp-cli/        # binario `hbp`
    hbp-net/        # Tor SOCKS + TCP Kademlia DHT
    hbp-app/        # GUI nativa `home_builder_pay` (egui)
    hbp-ui/         # wizard localhost (no es el producto)
  docs/PROJECT.md
```

### 7.1 Dependencias clave

| Crate | Versión | Para |
|---|---|---|
| `bitcoin` | 0.32.x | tx, taproot, addresses |
| `miniscript` | 12.3.x | hoja `and_v(v:pk(X),after(T))` |
| `musig2` | 0.4.1 | BIP-327 (trae `secp256k1` 0.31) |
| `clap` | 4 | CLI |

`musig2` y `bitcoin` 0.32 no comparten la misma `secp256k1`. El puente es serializar/parsear bytes en `hbp-bitcoin/src/convert.rs`.

No se usa BDK en el MVP: las txs se construyen a mano. Wallet de usuario = bitcoind/electrum externo por ahora.

### 7.2 Módulos `hbp-bitcoin`

- `taproot` — `Escrow`, descriptores, `tweaked_key_agg`, chequeo de que el tweak MuSig2 coincide con la output key de rust-bitcoin.
- `spend` — key-path tx, unwind tx, firmas, control block.
- `musig` — `FirstRound`/`SecondRound`, journal de seeds, helper in-process `finish_coop_signature` (tests).
- `validate` — funding tx vs quote.
- `sign_contract` — BIP340 tagged `hbp-contract`.
- `identity` — generación de claves toy.

### 7.3 Red Bitcoin local

No hay bitcoind embebido. En esta máquina ya existe un stack en:

`/home/felipe/projects/btc_clients`

- Bitcoin Core **31.1**, regtest, RPC `127.0.0.1:18443`, P2P `18444`
- conf: `btc_clients/bitcoin.conf` (`regtest=1`, `txindex=1`)
- datadir: `btc_clients/data/bitcoind`
- wallets: `miner`, `cormorant`
- arranque: `./start-regtest.sh` (también levanta LND; para HBP basta `./start-bitcoind.sh`)
- CLI: `source ./env.sh` luego `bitcoin-cli getblockchaininfo`

`hbp` no habla RPC. Los scripts `scripts/regtest_*.sh` usan `bitcoin-cli` de este stack: fondean las `bcrt1p…` de `hbp addresses` y pasan el hex a `hbp verify-funding`. Wallets de demo: `miner`, `hbp_mandante`, `hbp_contratista` (cormorant es watch-only).

**No** usar el LND mainnet Neutrino de ese folder para este proyecto.

---

## 8. CLI (`hbp`)

```
hbp --dir <carpeta> <comando>
```

| Comando | Quién | Efecto |
|---|---|---|
| `--passphrase` / `HBP_PASSPHRASE` | ambos | global; cifra/desbloquea `identity.json` (sin largo mínimo) |
| `init --network --role [--secret HEX]` | ambos | identidad (o restore); con `--passphrase` queda cifrada |
| `identity [--backup] [--encrypt]` | ambos | pubkey; `--backup` = secret; `--encrypt` pasa de claro a cifrado |
| `new --unit --bond-bps [--work-name] --dispute fee-burn --t1 --t2` | mandante | draft producto; `--dispute unwind\|mad\|arbiter` = legacy |
| `add-partida --desc --amount --plazo` | mandante | hito |
| `offer` | mandante | `00-offer.json` |
| `accept FILE` | contratista | pending |
| `commit FILE` | mandante | contrato firmado |
| `import FILE` | contratista | carga el contrafirmado |
| `propose-arbiter --pubkey` | cualquiera | `03-arbiter.json` (una firma) |
| `accept-arbiter FILE` | la otra punta / reimport | A locked; sin esto no hay addresses |
| `quote --btc-price/--bond-sats` | cualquiera | sats |
| `accept-quote FILE` | la otra punta / reimport | quote locked |
| `addresses` | ambos | boleta + partidas `bcrt1p`/`tb1p` |
| `status` | ambos | JSON de estado |
| `watch-import --xpub\|--descriptor` | cada laptop | watch-only **local** (Blue xpub). Nunca al peer |
| `coins [--esplora]` | cada laptop | UTXOs vía Esplora (no habla con Blue) |
| `offer-coin --outpoint` | cada laptop | `05-coin.json` shareable (un UTXO) |
| `verify-funding --tx-hex --partida` | ambos | valida y marca funded |
| `fund --mine/--peer` o `--m-outpoint …` | ambos (PSBT 2 wallets) | stdout PSBT base64; escrow exacto |
| `fund-combine FILE FILE` | cualquiera | junta firmas Blue; stdout hex para broadcast |
| `coop-propose` / `coop-sign FILE` / `coop-finish FILE` | cada laptop | MuSig2 por `04-coop.json` |
| `coop-close --kind --peer-dir …` | demo misma máquina | MuSig2 key-path (atajo) |
| `arbiter-close --kind --with am\|ac --arbiter-dir` | A+M o A+C | script path tras T |
| `unwind --kind --outpoint --sats --dest --fee` | dueño del unwind | tx hex |

Claves en claro por defecto; `--passphrase` las cifra. Toy. No mainnet.

---

## 9. Tests actuales

`cargo test --workspace` — 56 unitarios (Rust 1.88, `rust-toolchain.toml`).

**hbp-core** (22)

- parseo de montos y conversión fiat→sats; boleta = % del total
- JSON canónico estable; `DisputePolicy::Arbiter` **sin** pubkey
- nomination fuera del `contract_id`; A ≠ M y A ≠ C
- A se nombra solo con dos firmas y **antes** de fondear
- nonce reuse aborta; seed pendiente de un `coop-propose` se guarda y se consume
- vault: round-trip de identity cifrada (passphrase corta)
- no se fondea partida sin boleta, ni la 2 si la 1 está abierta
- happy path dos partidas + release de boleta; boleta se suelta si P2 nunca se fondeó

- fee-burn deadlines, stage=bond (10 × 10% ), work_name in contract_id, t1→t2 state

**hbp-bitcoin** (28)

- output key MuSig2+tweak = rust-bitcoin Taproot
- unwind script-path; cierre cooperativo y split 80/20
- funding con partida a 1 sat se rechaza
- PSBT de fondeo: montos de escrow exactos; dust se rechaza; MAD 50/50; `--partida-only`
- watch-only: vpub→tpub, gap scan, `05-coin.json` sin xpub, combine de dos PSBT parciales P2WPKH
- identidad se restaura desde el secret hex
- MuSig2 por archivos = mismo sig que el helper in-process
- firmas de contrato, quote (`hbp-quote`) y nomination `hbp-arbiter` round-trip
- árbol con A cambia la address; sin A nombrado, error
- hoja MAD = NUMS
- witness A+M de 4 ítems (script path árbitro)
- fee-burn: split 50/50, t1 continuación + fee, t2 OP_RETURN, coop key-path en escrow sin hojas

**hbp-net** — mensajes core, Kademlia 2/3 nodos, SOCKS/torrc

**hbp-app** (2) — stage=bond draft, backup import/export

Los E2E que hablan con `bitcoind` son scripts en `scripts/` (`run_catalog.sh`). Ver [REGTEST_SCENARIOS.md](REGTEST_SCENARIOS.md) y [SCENARIOS.md](SCENARIOS.md).

---

## 10. Roadmap

Horizonte: de “el protocolo existe en tests” a “dos personas lo usan en signet”, después red y extra.

### Hecho (MVP-0) — detalle vivo en §0

- Decisiones de producto
- Crates core / bitcoin / cli
- Descriptores Taproot + MuSig2 + unwind
- Estado de proyecto/partidas
- CLI de contrato + quote + addresses + verify-funding + unwind
- Tests locales (sin nodo)

### Fase 1 — ciclo on-chain en regtest

Usar `/home/felipe/projects/btc_clients`.

1. ~~CLI de recepción MuSig2 por archivos (`coop-propose` / `coop-sign` / `coop-finish`).~~
2. ~~PSBT de fondeo (`hbp fund`: escrow exacto, fee desde change).~~
3. Test de integración: `bitcoin-cli` fondea, mina, `verify-funding`, unwind real minado, cooperative close minado. **Hecho** (catálogo + remainders; P1 usa `hbp fund`).
4. `hbp watch` / recordatorio de T (el timeout no se broadcastea solo).
5. Importar quote/estado sin pisar un proyecto ya avanzado.
6. UI de pruebas (`hbp-ui`): hecha, fea a propósito. Fondeo Blue = watch-only + PSBT atómico (paso 4, ambas puntas).

### Fase 2 — signet usable por dos humanos

- Identidades: backup = secret hex (`identity --backup` / `init --secret`). No hay xpub/BIP39 del 2-de-2. El xpub de Blue es **otra** capa, local. Guías: [BLUE_FUNDING.md](BLUE_FUNDING.md), [SIGNET_TWO_PCS.md](SIGNET_TWO_PCS.md).
- Confirmaciones según monto (1 en signet, política en mainnet después).
- Fee bump del unwind (RBF ya está en sequence; falta UX).
- `extend-partida` / `renew` cooperativo si se va a pasar T.
- Mensajes de “esto no es boleta bancaria” y del trabajo hundido, en español.
- Empaquetado: un binario, instrucciones de dos laptops.

### Fase 3 — red (después de que el contrato ya funcione)

Orden fijo:

1. Mismos JSON por socket TCP (`hbp listen` / `hbp connect`) para no copiar archivos. Contacto ya conocido (IP o hostname).
2. **Tor punto a punto:** cada identidad publica un `.onion` en el contrato. La app habla solo por SOCKS. No hay directorio.
3. **DHT / offer book** solo si queremos que extraños se encuentren. Es otro producto encima del contrato, no un requisito del 2-de-2.

Android **después** de desktop estable (wallet + Tor + verificación de cadena es otro producto).

### Fase 4 — producto más rico

- Árbitro y MAD: E2E minados ([SCENARIOS.md](SCENARIOS.md)). `hbp fund` y MuSig2 por archivos: hechos.
- Boleta que **rueda** al 2-de-2 de la siguiente partida (splice) en vez de quedarse en un UTXO estático.
- Fotos/hashes de evidencia en `reception-proposed` (humanos, no el script).
- Watchtower casero o calendario para no perder el unwind.

### Fuera de alcance (consciente)

- Mainnet con plata real hasta que Fase 2 esté aburrida de tan estable.
- Oráculos de “la obra está hecha”.
- Custodia, KYC, matching marketplace.
- Lightning como rail de las partidas (posible después; no ahora).
- Replicar UF on-chain.

---

## 11. Plan de trabajo (cómo seguir)

Orden realista, cada ítem deja algo demoable.

| Orden | Ítem | Dónde | Hecho cuando |
|---|---|---|---|
| 0 | Este documento + MVP-0 compilando | `docs/PROJECT.md`, crates | `cargo test --workspace` verde |
| 1 | CLI MuSig2 de recepción en archivos | `hbp-cli`, `hbp-bitcoin` | dos dirs cierran una partida sin bitcoind (tx hex firmada) |
| 2 | PSBT de fondeo boleta+partida 1 | `hbp-bitcoin` | `hbp fund` imprime PSBT; `verify-funding` lo acepta |
| 3 | Integración regtest con `btc_clients` | tests + scripts | **hecho** (escenarios 1–8, default unwind) |
| 4 | Seed/backup y red signet | `hbp-cli` | **hecho** (feliz Signet minado, Electrum, [SIGNET_HAPPY_PATH.md](SIGNET_HAPPY_PATH.md)) |
| 5 | `hbp listen` / `connect`, luego Tor p2p | crate network nuevo | mismos JSON, primero TCP, después onion conocido |
| 6 | Árbitro / MAD opcionales | `hbp-bitcoin` taproot tree | **hecho** (E2E minado; catálogo 136 PASS) |
| 7 | Desktop GUI / Android | TBD | no empezar hasta 4 |

Criterio para no saltarse pasos: no hay DHT si el cooperative close no se minó en regtest. Feliz Signet **cerrado**. GUI nativo de producto: después de 2–3 unhappy en Signet (plazos de horas). No Electron / no web.

Estimación grosera (una persona, no calendario de promesa):

- Ítem 1–2: días
- Ítem 3: **hecho** (unwind)
- Ítem 4: **hecho** (Signet feliz; falta unhappy + GUI nativo)
- Ítem 5: semanas
- Ítem 6: **hecho** (minado)
- Ítem 7: proyecto aparte

---

## 12. Riesgos

- **MuSig2 nonce reuse** — mitigado con journal; hay que persistir *antes* de firmar en el CLI de recepción (aún no cableado).
- **Fee en unwind a 6 meses** — script path se firma al vencer, fee de mercado. OK.
- **Pérdida de seed** — sin timeout el dinero muere; con timeout, esperar T. Backup es parte de Fase 2.
- **Cliente malicioso** — `validate_funding_tx` es obligatorio en todo fondeo. Nunca confiar montos del peer.
- **Legal** — no es boleta de banco ni escrow regulado. Riesgo de producto, no consejo legal.
- **Asimetría boleta vs partida** — feature; hay que explicarla o el contratista cree un bug.

---

## 13. Glosario

| Término | Significado aquí |
|---|---|
| Partida | Hito con monto y plazo; un UTXO de pago |
| Boleta | Colateral global del contratista; otro UTXO |
| Unwind | Timeout: cada uno recupera lo suyo |
| Recepción | Cierre cooperativo MuSig2 de una partida |
| Quote | Acuerdo de sats al tipo de cambio del día |
| Mandante / Contratista | Principal / contractor |
| `bond_bps` | Boleta en basis points del total (1000 = 10%) |

---

## 14. Cómo correr lo que hay

```bash
cargo test --workspace
cargo run -p hbp-cli -- --help
```

Dos directorios (ver `README.md`). Nodo local cuando toque on-chain:

```bash
cd /home/felipe/projects/btc_clients
./start-bitcoind.sh
source ./env.sh
bitcoin-cli getblockchaininfo
```
