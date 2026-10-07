# Konstruado Android

App nativa en **Jetpack Compose (español)** que corre el flujo completo de obra
igual que el escritorio. La lógica es la misma en Rust, expuesta por UniFFI:

- `crates/konstruado-ffi` incluye **tal cual** `crates/konstruado/src/caja.rs`
  (motor de Monero: DKG FROST, fondeo CLSAG cooperativo, gasto FROST, scan,
  envío, respaldos), `persist.rs` e `i18n.rs` (con `#[path]`), más
  `konstruado-core` y `konstruado-net`.
- Tor = **Orbot externo** (SOCKS 127.0.0.1:9050). No hay tor/Arti dentro del APK.
- Solo **Monero stagenet**. Por defecto `https://stagenet.xmr.kernal.eu:38089` (público). En **Cuenta → Nodo Monero** podés poner el tuyo (p. ej. `http://100.x.y.z:38081` por Tailscale) y queda en `filesDir/konstruado/daemon.url`; vacío / «Usar por defecto» vuelve al público.

> Parte del monorepo. Los `.so` nativos y los APKs no van en git; se construyen o se bajan del Release.

## Qué hace la app

| Pantalla | Qué hace |
|---|---|
| Bienvenida | nombre + rol (mandante / contratista) |
| Tablero | estado de red, **Te toca**, mis obras, ofertas propias/ajenas, Buscar ofertas, Publicar |
| Oferta | aceptar condiciones o **contra** (otra garantía, ajustar partidas) |
| Obra | contra (confirmar / no aceptar), cierre / abandono, extra, **caja 2-de-2** (dirección, view key, guardar/recuperar share, mirar 200 bloques atrás), partidas |
| Partida | editar texto, **Encerrar** → **Confirmar y fondear** (CLSAG cooperativo) / reintentar / no encerrar, **Avisar que terminé** (%, nota), **Aceptar X% y pagar** (gasto FROST) / contra %, hilo de notas, recibo, txids |
| Billetera | crear semilla, saldo total/libre/trabado, entradas, recibir, **enviar**, actualizar, mirar atrás, guardar/recuperar 25 palabras, recuperar share, lista de cajas con view key |
| Cuenta | nombre/rol, Orbot, **nodo Monero**, pasos Orbot; destinos TCP solo bajo **Avanzado** |

- Encerrada / Pagada se marcan **solo cuando el motor ve la transacción en el scan** (`Hecho` con `visto`), igual que el escritorio.
- Errores del motor (sin saldo, trabadas, sin semilla, el otro no está en línea, sincronizando…) se muestran tal cual, en español.
- Respaldos (semilla y share) con el selector de Android (SAF): crear documento / abrir documento.
- Datos (`estado.json`, `xmr/` con semilla, shares FROST y libro) en el almacenamiento **privado** de la app (`filesDir/konstruado`), `allowBackup=false`.
- Un **servicio en primer plano** (notificación "Konstruado sincronizando") mantiene vivo el proceso: el gossip, los mensajes Caja y el scan siguen con la app en segundo plano.

## Red: cómo entra un teléfono

El teléfono **no hospeda onion** (no hay control de Orbot). Arranca en *modo celular*:

1. Abre una **sesión viva saliente** hacia la sala: el onion horneado
   (`RENDEZVOUS_ONION`, puerto 17432) a través del SOCKS de Orbot, y/o destinos TCP extra.
2. Por esa misma conexión recibe todo: ofertas, obras, presentes (gossip cada 2 s),
   la lista de pares y los mensajes **Caja** (DKG, fondeo, gasto).
3. Si el destinatario no está, la PC lo guarda en un **buzón** y lo entrega cuando vuelve.
4. **Dos teléfonos** se hablan a través de la PC que hospeda la sala (relay, máx. 3 saltos).

Alguien tiene que **hospedar la sala** (un solo anfitrión por vez, la clave está horneada):

- el **mandante de escritorio** (como siempre), o
- una PC contratista con `KONSTRUADO_HOSPEDAR_SALA=1`, o
- el relay sin ventana: `cargo run -p konstruado-net --bin konstruado-sala`
  (necesita `tor` en el PATH; con `--tcp 17432` solo escucha por TCP, para pruebas).

**El escritorio tiene que compilarse desde este árbol local**: el protocolo
cambió (sesiones vivas, `PeerAddr::Buzon`, campo `saltos` en Caja). Un escritorio
viejo no entrega mensajes a un teléfono.

### Monero por Orbot

`monero-simple-request-rpc` no habla SOCKS. Para que el tráfico del daemon pase
por Tor, usá **Orbot en modo VPN** e incluí Konstruado en las apps enrutadas. En
modo solo-proxy, el gossip va por Tor pero el daemon va directo por HTTPS.

### Nodo Monero propio (LAN / Tailscale)

1. En tu PC: `monerod --stagenet --rpc-bind-ip 0.0.0.0 --confirm-external-bind`
   (RPC stagenet suele ser **38081**; el público usa 38089).
2. Anotá la IP Tailscale (o LAN) de esa PC.
3. En el teléfono: **Cuenta → Nodo Monero** →
   `http://IP-TAILSCALE:38081` → Guardar nodo → Probar conexión.
4. «Usar por defecto» borra `daemon.url` y vuelve a
   `https://stagenet.xmr.kernal.eu:38089`.
5. Scan, tip, fondeo, gasto y broadcast usan siempre la URL activa.
6. «Probar RPC del nodo» pide solo la punta (altura) por HTTP y deja el resultado
   en pantalla (bloque + ms). No es un toast: si falla, explica timeout / puerto / LAN.

## Probar

### Dos teléfonos con Orbot

1. En una PC: compilá el escritorio de este árbol y abrilo como **mandante**
   (hospeda la sala), o corré `konstruado-sala` (con `tor` instalado).
2. En cada teléfono: instalar el APK y Orbot; encender Orbot (VPN recomendado).
3. En la app: Bienvenida → nombre y rol (uno mandante, otro contratista).
   Cuenta → Red: «Usar Orbot» encendido (127.0.0.1:9050) → Aplicar.
4. Esperar «Conectado a la sala» en el Tablero (publicar el onion puede tardar ~30 s).
5. Mandante: Publicar obra. Contratista: la ve, Aceptar. En ~5 s aparece la **misma
   caja 2-de-2** en los dos (Obra → Caja). Guardar el share (SAF).
6. Billetera → Crear billetera, cargar stagenet desde un faucet, esperar 10 bloques.
7. Partida 1: mandante «Encerrar» → contratista «Confirmar y fondear».
   Pasa a Encerrada cuando el scan ve la transacción. Contratista «Avisar que terminé» (%).
   Mandante «Aceptar X% y pagar» → Pagada cuando el scan ve el pago.

### Teléfono + PC

- Igual que arriba con un solo teléfono: la PC (mandante de escritorio) hospeda y
  es el otro lado del trato.
- **Sin Orbot, en la misma red** (solo pruebas): en la PC
  `KONSTRUADO_ESCUCHAR=0.0.0.0 cargo run -p konstruado` (o `konstruado-sala --tcp 17432`),
  y en el teléfono Cuenta → Destinos TCP: `IP-de-la-PC:17432`.
- **USB:** `adb reverse tcp:17432 tcp:17432` y destino `127.0.0.1:17432`.
- **Emulador:** destino `10.0.2.2:17432` (la PC del emulador).

## Build

Requisitos: JDK 17+, Android SDK 34, NDK 26, Rust 1.89+ (`rustup`) con
`aarch64-linux-android` y `x86_64-linux-android`, `cargo-ndk`.

```bash
android/build-apk.sh      # bindings + .so (arm64-v8a, x86_64) + APK
```

Lo mismo a mano:

```bash
cargo build -p konstruado-ffi --lib --bin uniffi-bindgen
./target/debug/uniffi-bindgen generate target/debug/libkonstruado_ffi.so \
  --library -l kotlin -o /tmp/kbind --no-format     # lib debug: la release va con strip
cp /tmp/kbind/uniffi/konstruado_ffi/konstruado_ffi.kt \
  android/app/src/main/java/uniffi/konstruado_ffi/
cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs \
  build -p konstruado-ffi --release --lib
cd android && ./gradlew :app:assembleDebug
```

## Tests

```bash
cargo test -p konstruado-net       # sesiones vivas, relay entre dos celulares, buzón
cargo test -p konstruado-ffi --lib # incluye los tests de caja.rs, persist.rs, i18n.rs
cargo test -p xmr-joint
```

Smoke de interop en una sola máquina (procesos separados, TCP local 17432):

```bash
cargo build -p konstruado-ffi --bin konstruado-par && cargo build -p konstruado-net --bin konstruado-sala
P=target/debug/konstruado-par
# A) par "escritorio" (escucha 17432) vs par celular por la fachada FFI
$P --datos /tmp/m --nombre "Don Dinero" --rol mandante --escritorio --segundos 90 &
$P --datos /tmp/c --nombre Chasquilla --rol contratista --destino 127.0.0.1:17432 --segundos 90
# B) dos celulares por relay
target/debug/konstruado-sala --tcp 17432 &
$P --datos /tmp/m2 --nombre M --rol mandante    --destino 127.0.0.1:17432 &
$P --datos /tmp/c2 --nombre C --rol contratista --destino 127.0.0.1:17432
```

Cada par imprime `PASO CAJA <dirección>`: tiene que ser la misma en los dos.

## Limitaciones conocidas

- El teléfono no hospeda onion: sin una PC con la sala, dos teléfonos no se encuentran.
- Daemon de Monero por Tor solo con Orbot en modo VPN.
- Fondeo/pago real necesitan monedas de faucet de stagenet en las dos billeteras;
  sin saldo, el motor lo dice y no arma nada.
- Un solo perfil por instalación (la carpeta de datos es global al proceso).
- El share vive en el almacenamiento privado de la app; no usa Android Keystore todavía.

## Saldo personal y fondeo

El total/libre/trabado suma las salidas del libro local. Al **fondear** o **enviar**, esas
salidas se marcan gastadas y no vuelven a contar (ni si mirás más atrás). Tras actualizar
saldo, si el daemon acepta `is_key_image_spent` (típico en monerod propio sin restricted-rpc),
se podan fantasmas de fondeos anteriores al arreglo. El nodo público a veces no ofrece ese RPC.
