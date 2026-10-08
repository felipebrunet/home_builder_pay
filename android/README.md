# Konstruado Android

App nativa en **Jetpack Compose (español)** que corre el flujo completo de obra
igual que el escritorio. La lógica es la misma en Rust, expuesta por UniFFI:

- `crates/konstruado-ffi` usa la lib compartida `crates/konstruado-motor`
  (`caja.rs`: motor de Monero, DKG FROST, fondeo CLSAG cooperativo, gasto FROST,
  scan, envío, respaldos; `respaldo.rs`, `persist.rs`, `i18n.rs`), la misma que
  usa el escritorio, más `konstruado-core` y `konstruado-net`.
- Tor = **Orbot externo** (SOCKS 127.0.0.1:9050). No hay tor/Arti dentro del APK.
- Solo **Monero stagenet**. Por defecto `https://stagenet.xmr.kernal.eu:38089` (público). En **Cuenta → Nodo Monero** podés poner el tuyo (p. ej. `http://100.x.y.z:38081` por Tailscale) y queda en `filesDir/konstruado/daemon.url`; vacío / «Usar por defecto» vuelve al público.

> Parte del monorepo. Los `.so` nativos y los APKs no van en git; se construyen o se bajan del Release.

## Qué hace la app

| Pantalla | Qué hace |
|---|---|
| Bienvenida | **Crear cuenta nueva** (nombre + rol) o **Restaurar desde respaldo** (archivo `.kbak` + contraseña) |
| Tablero | estado de red, **Te toca**, mis obras, ofertas propias/ajenas, Buscar ofertas, Publicar |
| Oferta | aceptar condiciones o **contra** (otra garantía, ajustar partidas) |
| Obra | tarjetas **Ahora** (estado + contra / cierre), **Partidas** (chips de estado), extra, **Caja y pagos** (dirección, view key, historia, mirar 200 bloques atrás), chip «Se libera en ~N bloques», recordatorio de respaldo, **Importar share suelto** (plegable, solo 0.2.7), **Avanzado** (abandonar / archivar, plegable) |
| Partida | **Ahora** (chips, estado de alto fijo, solo las acciones válidas: Encerrar → Confirmar y fondear / reintentar / no encerrar, Avisar que terminé, Aceptar X% y pagar / contra %; con el fondeo sin 10 bloques el botón queda apagado con la cuenta regresiva), **Caja y pagos** (txids monoespaciados con copiar, recibo), Hilo, **Importar share suelto**, **Avanzado** (texto, salir en este equipo) |
| Billetera | **Saldo** (fila de estado fija + saldo que entra), **Entradas**, **Recibir**, **Enviar**; plegables **Respaldos y recuperación** (respaldo completo; **Ver las 25 palabras** con advertencia, altura, copiar sensible + borrado a 60 s y `FLAG_SECURE`; dirección + view key; importación vieja en Avanzado) y **Nodo / Avanzado**; cajas de obras |
| Cuenta | nombre/rol, **Red** (estado real de Orbot/sala, Aplicar, Probar Orbot), **nodo Monero** (prueba RPC en fila fija), «Cómo funciona» plegados; destinos TCP solo bajo **Avanzado** |

### Estado de Orbot y la sala

La app ya no dice «Encendé Orbot» por tener cero pares. El motor mide el SOCKS en
cada intento (`DiagSocks` en `konstruado-net`): TCP al SOCKS y después el CONNECT
al onion, separando el error del SOCKS del error del destino. **Probar Orbot** hace
solo el saludo SOCKS5 (`05 01 00` → `05 00`) y la app mira si el paquete
`org.torproject.android` está instalado.

| Estado | Qué se ve |
|---|---|
| sin SOCKS en la app | «Orbot apagado en Konstruado» |
| Orbot no instalado y SOCKS cerrado | «Orbot no está instalado» |
| SOCKS cerrado | «Orbot no responde en 127.0.0.1:9050 · Abrí Orbot y tocá Iniciar…» |
| SOCKS OK, buscando | «Orbot responde · llamando a la sala…» |
| SOCKS OK, onion sin respuesta | «La sala no responde · Orbot funciona. ¿Está abierto Konstruado en el PC?» |
| sesión viva | «Conectado a la sala» |

Orbot en modo VPN por app con Konstruado **sin** marcar sirve: el SOCKS sigue en
127.0.0.1:9050 y es lo que usa la sala.

### Diseño

- Componentes compartidos en `ui/Componentes.kt`: `Tarjeta`, `Plegable`,
  `ComoFunciona` (ayuda larga plegada), `Ayuda` (chica y apagada), `EstadoFila` /
  `EstadoTarjeta` (alto fijo: un punto o un spinner chico + texto), `Chip` por tono
  (ok / espera / error / apagado, los mismos que `caja::Tono` del escritorio),
  `Copiable` (monoespaciada + copiar), `SaldoGrande` (entero + 4 decimales grandes,
  el resto chico; precisión completa a la vista), botones `Primario` / `Secundario` /
  `Peligro` / `TextoBoton`.
- La línea de estado de la billetera sale de `caja::estado_billetera` (la misma que
  la barra del escritorio): «Mirando la cadena… quedan N bloques», «Al día · bloque N»…
- Paleta completa en claro y oscuro (`ui/theme/Theme.kt`); todos los pares
  texto/fondo de tonos y botones dan ≥ 4,5:1 (WCAG AA).
- La app Android está solo en español (como antes); los textos compartidos del
  motor tienen su versión en inglés para el escritorio.

- Los botones de una partida salen de `caja::acciones_partida` (la misma regla del escritorio). Con el pago ya firmándose o esperando bloque no aparecen **Aceptar X% y pagar** ni la contra; se ve un chip con el estado del pago.
- **Quitar oferta** retira la oferta para todos (lápida firmada en el DHT): no vuelve cuando el contratista se reconecta. Solo el mandante que la publicó, y solo si nadie la tomó.
- Encerrada / Pagada se marcan **solo cuando el motor ve la transacción en el scan** (`Hecho` con `visto`), igual que el escritorio.
- Errores del motor (sin saldo, trabadas, sin semilla, el otro no está en línea, sincronizando…) se muestran tal cual, en español.
- **Ver las 25 palabras** (billetera personal): advertencia, grilla, altura de restauración, copiar con `ClipDescription.EXTRA_IS_SENSITIVE` (API 33+) y borrado del portapapeles a los 60 s; `FLAG_SECURE` mientras se ven. Dirección y view key privadas aparte (solo lectura).
- **Respaldo completo** con el selector de Android (SAF): `CreateDocument` para exportar el `.kbak`, `OpenDocument` para restaurar. Mismo formato y mismas verificaciones que el escritorio (`konstruado-motor/src/respaldo.rs`, el mismo código). Restaurar deja todo en `restaurar.listo/` y reinicia el proceso (`ReinicioActivity` en `:reinicio`); al arrancar, `KonstruadoApp.nuevo` aplica el cambio y deja lo anterior en `previo-<fecha>/`.
- Los respaldos sueltos de 0.2.7 (25 palabras, share, obras) se importan en **Billetera → Respaldos y recuperación → Avanzado**; ya no se exportan por separado.
- Traba de desbloqueo: `caja::traba_partida` (bloque del fondeo + 10 contra la punta del nodo, que se refresca cada minuto). Mientras falta, «Avisar que terminé» / «Aceptar X% y pagar» se ven apagados con «Podés marcarla terminada en ~N bloques (~M min, bloque X)».
- «Usar el máximo» manda todo el saldo libre menos el fee; los pagos de la caja y los envíos exactos dejan la salida extra en 0 XMR (antes 1 piconero).
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

### Monero y Orbot

`monero-simple-request-rpc` no habla SOCKS: el RPC del daemon siempre abre TCP
directo, aunque «Usar Orbot» esté encendido (ese SOCKS es solo para la sala).

- **Nodo local / Tailscale** (192.168.x, 10.x, 172.16–31.x, 100.64/10, `.local`):
  va directo por la red local y nunca por Tor. Tor no llega a IPs privadas.
- **Nodo público**: va directo por internet (el nodo ve tu IP), salvo que la VPN
  de Orbot capture a Konstruado; ahí va por Tor.
- **Orbot en modo VPN + nodo local**: si la VPN captura a Konstruado, la conexión
  muere con «connection reset». Android no deja saltar la VPN de Orbot (Orbot no
  llama a `VpnService.Builder.allowBypass()`, netd rechaza `bindSocket`). Dejá
  Konstruado afuera: en Orbot → «Elegir aplicaciones» marcá **otra** app y no
  Konstruado (sin ninguna marcada Orbot vuelve a «VPN de dispositivo completo»),
  o usá «Modo de usuarie avanzado» (solo SOCKS, sin VPN). La app detecta la VPN y
  lo avisa en Cuenta y en «Probar RPC del nodo».

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
2. En cada teléfono: instalar el APK y Orbot; encender Orbot (no hace falta el modo VPN; la sala usa su SOCKS).
3. En la app: Bienvenida → nombre y rol (uno mandante, otro contratista).
   Cuenta → Red: «Usar Orbot» encendido (127.0.0.1:9050) → Aplicar.
4. Esperar «Conectado a la sala» en el Tablero (publicar el onion puede tardar ~30 s).
   Si dice «La sala no responde», Orbot anda bien: falta abrir Konstruado en la PC.
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
android/build-apk.sh          # debug: bindings + .so (arm64-v8a, x86_64) + APK debug
android/build-apk-release.sh  # release: .so arm64-v8a (release, strip, LTO) + R8 + firma release
```

**APK release.** `assembleRelease -Pabis=arm64-v8a` con R8 (`isMinifyEnabled`,
`isShrinkResources`); `app/proguard-rules.pro` mantiene `uniffi.**` y JNA, que se
usan por reflexión. La firma sale de variables de entorno o propiedades de Gradle
`KONSTRUADO_RELEASE_STORE_FILE`, `KONSTRUADO_RELEASE_STORE_PASSWORD`,
`KONSTRUADO_RELEASE_KEY_ALIAS`, `KONSTRUADO_RELEASE_KEY_PASSWORD`; el script las lee
de `KONSTRUADO_RELEASE_ENV` (por defecto `~/.config/konstruado-release.env`, fuera
del repo). Sin esos datos el release se firma con la clave debug, así cualquiera
puede compilarlo. El asset del Release se llama `konstruado-X.Y.Z-android-arm64.apk`
(hasta 0.2.9: `…-android-arm64-debug.apk`).

**Pasar de la firma debug a la release:** Android no actualiza una app firmada con
otra clave. Antes, **respaldo completo** (Billetera → Respaldos y recuperación →
Exportar respaldo completo, `.kbak`); después desinstalar la app debug, instalar el
APK release y elegir **Restaurar desde respaldo**. Desinstalar borra los datos
privados de la app (semilla y shares de las obras).

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

### Capturas de pantalla (sin emulador)

Robolectric + Roborazzi dibujan las pantallas Compose en la JVM, con el motor Rust
real por JNA (`target/debug/libkonstruado_ffi.so`):

```bash
cargo build -p konstruado-ffi --lib
cd android
# pantallas reales sobre un perfil de demo (carpeta de datos de la app)
./gradlew :app:testDebugUnitTest --tests 'cl.konstruado.app.capturas.*' \
  -Pcapturas=/tmp/capturas -Pdemo=/ruta/a/datos-demo   # -Poscuro=1 para tema oscuro
```

`GaleriaEstadosTest` dibuja además los estados de Orbot/sala, la fila de escaneo,
saldos largos, chips y botones (claro y oscuro) con datos falsos. Sin `-Pcapturas`
estos tests se saltan.

## Limitaciones conocidas

- El teléfono no hospeda onion: sin una PC con la sala, dos teléfonos no se encuentran.
- Daemon público por Tor solo con Orbot en modo VPN capturando a Konstruado, y en
  ese modo un nodo LAN no responde (ver «Monero y Orbot»).
- Fondeo/pago real necesitan monedas de faucet de stagenet en las dos billeteras;
  sin saldo, el motor lo dice y no arma nada.
- Un solo perfil por instalación (la carpeta de datos es global al proceso).
- El share vive en el almacenamiento privado de la app; no usa Android Keystore todavía.
- Restaurar un respaldo completo y el reinicio del proceso están probados en la JVM y en el escritorio, no en un teléfono real.

## Saldo personal y fondeo

El total/libre/trabado suma las salidas del libro local. Al **fondear** o **enviar**, esas
salidas se marcan gastadas y no vuelven a contar (ni si mirás más atrás). Tras actualizar
saldo, si el daemon acepta `is_key_image_spent` (típico en monerod propio sin restricted-rpc),
se podan fantasmas de fondeos anteriores al arreglo. El nodo público a veces no ofrece ese RPC.
