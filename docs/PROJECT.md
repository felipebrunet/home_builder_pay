# home_builder_pay — documento de proyecto

Cliente Bitcoin P2P para acordar **partidas de obra** y una **boleta de garantía**, sin servidor, sin notario y sin banco. El caso de uso que lo justifica es construcción (o cualquier trabajo por hitos) en contextos de poca institucionalidad o de ticket que no paga abogados. El mecanismo no es exclusivo de casas.

Este archivo es la fuente de verdad de producto, protocolo, arquitectura, roadmap y plan de trabajo. El `README.md` de la raíz es el atajo de build/CLI.

**Si retomas en una sesión nueva: lee la sección 0 y el checklist de “listo / no listo”. El código en `crates/` es la implementación de MVP-0.**

Última actualización: 2026-08-30 (cierre de la sesión que dejó MVP-0 commiteado).

---

## 0. Dónde estamos (para retomar)

Checkpoint de sesión. Actualizar **esta sección** cada vez que se cierre un hito, no solo el roadmap de abajo.

| Campo | Valor |
|---|---|
| Fecha | 2026-08-30 |
| Rama | `master` |
| Hito | **MVP-0 + camino feliz on-chain en regtest** |
| Siguiente ítem | Plan §11: CLI MuSig2 **por archivos** (sin `--peer-dir`); PSBT de fondeo dentro de `hbp` |
| Verificar al abrir | `cargo test --workspace` (debe quedar verde) |
| Nodo Bitcoin local | `/home/felipe/projects/btc_clients` — Core 31.1 regtest, RPC `:18443`. El demo habla RPC con `bitcoin-cli`; `hbp` no embebe el cliente. |

### Listo (hecho en esta sesión)

- Decisiones de producto (disputa unwind, boleta global, fiat→sats al quote, red = archivos, Rust 100%).
- Workspace: `hbp-core`, `hbp-bitcoin`, `hbp-cli` (binario `hbp`).
- Contrato off-chain: offer / accept / commit / import, JSON canónico, firmas BIP340 tagged `hbp-contract`.
- Quote de sats (ambos firman; `accept-quote` también importa el quote ya contrafirmado).
- Descriptores Taproot: boleta `tr(musig(M,C), pk(C)&&after(T_proyecto))`, partida `tr(musig(M,C), pk(M)&&after(T_partida))`.
- MuSig2 key-path y unwind script-path (librería + tests; output key coincide con rust-bitcoin).
- Máquina de estados proyecto/partida (una partida viva; no fondear N+1 si N no está terminal).
- `NonceJournal`: reutilizar seed aborta.
- CLI: `init`, `new`, `add-partida`, `offer`, `accept`, `commit`, `import`, `quote`, `accept-quote`, `addresses`, `status`, `verify-funding`, `unwind`, `coop-close --peer-dir`.
- `validate_funding_tx` rechaza monto de partida distinto al quote.
- Tests: 13 unitarios verdes (`hbp-core` 8, `hbp-bitcoin` 5).
- **Camino feliz minado en regtest** (60k USD, 2×30k, boleta 20k, 5 s/partida): ver [REGTEST_HAPPY_PATH.md](REGTEST_HAPPY_PATH.md). `hbp coop-close --peer-dir` (misma máquina) + PSBT 2 wallets.
- **Contratista abandona antes de la partida 2**: unwind de la partida al mandante; boleta no es ejecutable por el mandante; [REGTEST_CONTRACTOR_ABANDONS.md](REGTEST_CONTRACTOR_ABANDONS.md).
- Matriz E2E (feliz, abandono, split 80 %, sin boleta, cancelar tras P1, MAD no implementado): [REGTEST_SCENARIOS.md](REGTEST_SCENARIOS.md).

### No listo (no está en el código)

- CLI de recepción cooperativa **por archivos** (sin `--peer-dir`). Hoy el cierre feliz en una sola máquina usa `coop-close --peer-dir`.
- PSBT de fondeo **dentro de** `hbp` (el script de demo lo arma con `bitcoin-cli`).
- Broadcast / mine / spend real contra `btc_clients`.
- `hbp listen` / `connect`, Tor, DHT.
- Seed BIP39, árbitro, boleta que rueda, GUI, Android, mainnet.

### Cómo seguir en la próxima sesión

1. Leer esta sección 0 y las decisiones §2.
2. `cargo test --workspace`.
3. Implementar ítem 1 del plan: comandos de recepción MuSig2 que escriben/leen JSON en `--dir`, sin nodo.
4. No abrir Tor/DHT ni GUI.

El usuario acordó explícitamente: canal = archivos ahora; Tor p2p después; DHT solo si hay marketplace.

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
| 1 | Disputa | **Unwind** on-chain. El árbitro tardío es un campo opcional del contrato (cambia el descriptor). El MVP no lo implementa. |
| 2 | Boleta | **Una, global**, `bond_bps` configurable (default 1000 = 10% del total). Se fondea una vez. **Una partida viva a la vez.** |
| 3 | Moneda | Contrato en USD / UF / CLP. Los sats se fijan **al quotear/fondear**. |
| 4 | Red | **Sin servidor propio, siempre.** El canal no es parte del contrato. MVP: **archivos** (USB, Signal, mail, carpeta). Después: socket LAN opcional, luego **Tor punto a punto** (onion ya conocido, va en el contrato). **DHT / offer book al final**, solo si el producto es marketplace. |
| 5 | Lenguaje | **100% Rust** |

Cómo se “conectan”: Bitcoin no necesita un socket. Dos laptops que se pasan `00-offer.json` ya cierran un 2-de-2. Tor oculta IP cuando el contacto ya existe; la DHT es para *encontrar* extraños. No se finge un marketplace antes de minar una recepción en regtest.

Consecuencias:

- Con 20 partidas de 1 000 y boleta 10% de 20 000, el contratista tiene **2 000 locked** y el mandante **1 000** en la partida activa. Es esperado: la boleta no se pone 20 veces.
- El % se avisa en CLI si es &lt;5% del total, &gt;30% del total, o mayor que la partida actual.

---

## 3. Modelo funcional

### 3.1 Roles

- **Mandante**: pone el pago de la partida activa.
- **Contratista**: pone la boleta una vez y ejecuta la obra.
- **Árbitro** (no en MVP): solo existiría *después* de un plazo, y solo si se declaró al crear el contrato. No se puede agregar a un UTXO ya fondeado.

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

### 4.6 Árbitro (plantilla, no código)

Si `arbiter_pubkey` está presente al crear el contrato, el árbol sería distinto (p.ej. `A+M` / `A+C` después de T, y unwind unilateral en T2 ≫ T). Cambiar el árbol **después** de fondear es imposible: cambia la address. El MVP deja el campo en el JSON y construye solo el árbol unwind-only.

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
  identity.json          # toy: secret en claro
  nonces.json            # seeds MuSig2 consumidos
  draft.json
  00-offer.json
  CURRENT                # id del contrato activo
  contracts/<id>/
    01-accepted.json
    02-quote.json
    state.json
```

Secuencia:

1. `init` — identidad (secp256k1, pubkey comprimida 33 bytes hex).
2. Mandante `new` + `add-partida` + `offer`. Firma BIP340 tagged `hbp-contract` sobre el JSON canónico del body **sin** clave del contratista.
3. Contratista `accept` — agrega su pubkey, firma el body completo. Sale `01-accepted.pending.json`.
4. Mandante `commit` — comprueba que los *terms* coinciden con la oferta, contrafirma el body completo.
5. Contratista `import` el `01-accepted.json`.
6. `quote` / `accept-quote` — ambos firman `bond_sats` y `partida_sats`. El archivo contrafirmado hay que devolvérselo a la otra punta (`accept-quote` importa si ya está firmado por mí).
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
    hbp-bitcoin/    # Taproot, MuSig2, PSBT/tx, verify-everything
    hbp-cli/        # binario `hbp`
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

HBP aún no habla RPC. El siguiente trabajo on-chain es: fondear las addresses `bcrt1p…` que imprime `hbp addresses` con esos wallets y pasar el hex a `hbp verify-funding`.

**No** usar el LND mainnet Neutrino de ese folder para este proyecto.

---

## 8. CLI (`hbp`)

```
hbp --dir <carpeta> <comando>
```

| Comando | Quién | Efecto |
|---|---|---|
| `init --network --role` | ambos | identidad |
| `new --unit --bond-bps --t-project` | mandante | draft |
| `add-partida --desc --amount --plazo` | mandante | hito |
| `offer` | mandante | `00-offer.json` |
| `accept FILE` | contratista | pending |
| `commit FILE` | mandante | contrato firmado |
| `import FILE` | contratista | carga el contrafirmado |
| `quote --btc-price/--bond-sats` | cualquiera | sats |
| `accept-quote FILE` | la otra punta / reimport | quote locked |
| `addresses` | ambos | boleta + partidas `bcrt1p`/`tb1p` |
| `status` | ambos | JSON de estado |
| `verify-funding --tx-hex --partida` | ambos | valida y marca funded |
| `unwind --kind --outpoint --sats --dest --fee` | dueño del unwind | tx hex |

Claves en claro. Toy. No mainnet.

---

## 9. Tests actuales

`cargo test --workspace`

**hbp-core**

- parseo de montos y conversión fiat→sats
- boleta = % del total
- JSON canónico estable
- nonce reuse aborta
- no se fondea partida 2 si la 1 está abierta
- happy path dos partidas + release de boleta
- rechazo de bond con monto distinto al quote

**hbp-bitcoin**

- output key MuSig2+tweak = rust-bitcoin Taproot
- unwind script-path: schnorr del mandante verifica contra la hoja
- cierre cooperativo MuSig2: `verify_single` + schnorr contra la output key
- funding con partida a 1 sat se rechaza
- firmas de contrato round-trip

No hay todavía un test que hable con `bitcoind` (broadcast + mine + spend real). Eso es el siguiente hito de verificación.

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

### Fase 1 — ciclo on-chain en regtest (siguiente)

Usar `/home/felipe/projects/btc_clients`.

1. CLI de recepción MuSig2 por archivos (`coop-nonce` / `coop-sign` / `coop-finish`).
2. Construir el PSBT de fondeo (dos inputs, dos escrows, change) en vez de “tráeme un hex”.
3. Test de integración: `bitcoin-cli` fondea, mina, `verify-funding`, unwind real minado, cooperative close minado.
4. `hbp watch` / recordatorio de T (el timeout no se broadcastea solo).
5. Importar quote/estado sin pisar un proyecto ya avanzado.

### Fase 2 — signet usable por dos humanos

- Identidades con backup de seed (BIP39 o descriptor), no hex suelto.
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

- Árbitro tardío opcional (descriptor con tercera clave).
- Boleta que **rueda** al 2-de-2 de la siguiente partida (splice) en vez de quedarse en un UTXO estático.
- MAD sobre un *dispute stake* chico y simétrico (no quemar 20k).
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
| 3 | Integración regtest con `btc_clients` | tests + scripts | script que mina unwind y coop close de punta a punta |
| 4 | Seed/backup y red signet | `hbp-cli` | dos máquinas, una partida de prueba |
| 5 | `hbp listen` / `connect`, luego Tor p2p | crate network nuevo | mismos JSON, primero TCP, después onion conocido |
| 6 | Árbitro opcional | `hbp-bitcoin` taproot tree | descriptor distinto si hay pubkey |
| 7 | Desktop GUI / Android | TBD | no empezar hasta 4 |

Criterio para no saltarse pasos: no hay DHT si el cooperative close no se minó en regtest. No hay GUI si el CLI de dos humanos en signet no cerró una partida.

Estimación grosera (una persona, no calendario de promesa):

- Ítem 1–2: días
- Ítem 3: días (el nodo ya está)
- Ítem 4: 1–2 semanas (UX de keys y fees)
- Ítem 5: semanas
- Ítem 6: días de script + más de producto (“quién es el árbitro”)
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
