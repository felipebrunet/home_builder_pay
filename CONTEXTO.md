# Konstruado — contexto para una sesión nueva

Producto de escritorio (Dioxus) para un **trato de obra entre dos personas**, sin servidor. No es un chat. El mandante publica un aviso; el contratista lo ve y acepta o contraoferta. El trato y la sala Tor funcionan. Monero está a medias: ver **Estado**. No hay Bitcoin.

Hablamos en español. UI rioplatense/chilena (“Poné”, “te toca”) por defecto; el usuario puede pasar a **English** (ES/EN en la barra y en Cuenta). El trato no cambia.

## Crates

| Crate | Rol |
|---|---|
| `konstruado-core` | Dominio: persona, oferta, obra, partidas, contra, extra, recibo, fusión. Sin UI ni Tor. |
| `konstruado-net` | Encuentro: TCP local `17432`, gossip DHT, Tor propio + onion horneado. |
| `konstruado` | Ventana: pantallas, persistir, exportar, temas, idioma. |
| `xmr-joint` | Hot wallet, DKG 2-de-2, plan de encierre y reparto. No emite una tx. |

El split está bien. No hace falta un refactor grande. `main.rs` es largo; partir pantallas solo si duele.

## Roles y flujo

- **Mandante** paga. Publica nombre, trabajo, garantía sugerida, textos de partidas. Abre la sala Tor.
- **Contratista** construye. Solo busca sala. Ve avisos, acepta o propone otra garantía.
- Garantía de la obra **divide exacto** el trabajo (10 000 / 2 000 → 5 partidas). Cada partida original encierra esa garantía por lado.
- **Contra:** el contratista propone otra garantía. El mandante confirma o **No aceptar esta garantía** (el aviso vuelve al tablero con lo original).
- **Partida:** los dos confirman el encierre (sesión viva) → el contratista avisa término con % y nota (máx. 50) → el otro acepta o contraoferta % → al pagar, **recibo congelado**.
- **Abandono:** si no hay encierre, unilateral. Si hay Encerrando/Encerrada/En trato/Pagada, es *propuesta de cierre* y el otro acepta.
- **Sesión viva + catch-up:** pagar, contra, extra, encerrar y cierre cooperativo piden par reciente (~25 s) **y** un dump de estado reciente (~30 s). Si Tor aún arranca o no bajó lo último: “Sincronizando el trato…”. Publicar, exportar, tema y editar texto pendiente no.
- **Extra:** cualquiera propone texto **y monto propio**. El otro acepta o rechaza. El rechazo no se puede pisar con gossip (`extra_seq`). Extra congelada si la obra está abandonada/cerrada/rechazada.
- **Abandonar** corta el trato. Cerradas/abandonadas no viven en el tablero central.

No son un chat de usuarios: se ven **obras y avisos**, no una lista tipo WhatsApp.

## Red

- Swarm horneado: `konstruado-red-1` + onion en `crates/konstruado-net/src/rendezvous.rs`. Nadie tipeá un código.
- Cada `cargo run` lanza **su** `tor` (`-f` propio, no el `torrc` del sistema).
- Mandante **hospeda** la sala; contratista **marca**. Publicar el descriptor tarda ~30 s; el dial del contratista espera hasta 45 s.
- Misma PC: se encuentran por `127.0.0.1:17432` sin esperar Tor.
- Dos PCs: hace falta el paquete `tor`. El contratista puede **Buscar ofertas**. Sigue marcando la sala para bajar obras nuevas.
- `fusionar` en obras **no retrocede** estados (Pendiente → Encerrada → En trato → Pagada).

## Persistencia y UI

- `~/.konstruado/estado.json` (override `KONSTRUADO_DATOS`). Dos ventanas en un PC: **dos dirs**.
- Campos nuevos en JSON llevan `#[serde(default)]` (`tema` → vivo, `idioma` → es). Si el parse falla, se copia a `estado.json.bak` y no se debe entrar como usuario nuevo hasta revisar.
- Click en el nombre → cuenta: cambiar nombre/rol (el **id** no cambia), tema **Vivo** (default) / **Calma**, idioma **Español** (default) / **English**. ES/EN también está en la barra de arriba (incluso en la bienvenida).
- Menú nativo **Help** (siempre en inglés): **About Konstruado** y **README**. El README de la app es `crates/konstruado/HELP.md` (cómo se usa). El `README.md` de GitHub es clonar, dependencias y binario.
- Tablero central = solo **activas** (avisos sin tomar + obras en curso). Sidebar **Mis obras** = historial en las que participás, más reciente arriba (`actualizado`).
- **Te toca** = interacción de un trato abierto (contra, %, extra), no “alguien publicó”.
- Exportar constancia: `.txt` y `.pdf` desde el detalle de la obra.

Binario Linux de prueba: ver `README.md`. No commitear `dist/`. Versión `0.1.0-dev`.

## Cómo probar dos máquinas

```bash
git pull
cargo run
```

Clone del notebook: `git clone git@github.com-hbp:felipebrunet/home_builder_pay.git` (alias SSH `hbp_deploy`). En el notebook hay que tener GTK/WebKit/`libxdo-dev`/`tor`.

Primero mandante (esperar `sala abierta`). Después contratista. La primera vez Tor puede tardar en bootstrap.

## Estado (2026-09-26)

Lo que la ventana ya hace:

- Trato, tablero, Tor, dos dirs de datos, constancia.
- Notas y texto del extra van cifrados entre las dos personas (X25519 + ChaCha20-Poly1305, una clave por persona). El contexto `{obra}:nota` / `{obra}:extra` ata la caja a esa obra. Un tercero ve el bloque y no el texto. La obra en curso no aparece en su tablero. El nodo igual reenvía el JSON para que los dos se alcancen.
- Al entrar se crea una hot wallet de **stagenet**. La dirección está en la persona y se ve en Cuenta. La spend (`spend_sec`, hex) queda en `estado.json` y no se publica. La view sale de `keccak256(spend)`.

Lo que está en `xmr-joint` y tiene tests, y **no** está enganchado al botón de aceptar ni al de pagar:

- DKG PedPoP 2-de-2 (`dkg-pedpop` vendido en `crates/xmr-joint/vendor/dkg-pedpop`, parche multiexp 0.5). Tres mensajes: compromiso, share cifrado, view que inventa el mandante. Cada nodo guarda solo su `ThresholdKeys`. Los dos llegan a la misma dirección. `cargo test -p xmr-joint`.
- Encierre de una partida: una salida de `2 × garantía` a esa dirección. La sesión no cierra si falta un lado. Todavía no hay CLSAG.
- Pago: 100% manda el pot al contratista; 80% le deja `1.8 × garantía` y devuelve `0.2 × garantía` al mandante. Hacen falta los dos shares. Todavía no es `SignableTransaction::multisig`.

Por qué no hay tx: hace falta un daemon de stagenet para los anillos, y `monero-wallet` 0.2 firma todos los inputs con una sola spend. El funding en dos máquinas necesita `Clsag::sign_input_with_mask` y `sum_output_masks` público. Esos parches no están en crates.io.

No commitear shares, views ni `.raw` de laboratorio, aunque sean stagenet. `.gitignore` tapa `artifacts/`, `threshold_keys.bin`, `*view_private*`, `*shared_view*` y `*.raw`.

Regtest local (escenario 1, pago al 100%) está en `crates/xmr-joint/tests/regtest_pago.rs` y la lista de los que faltan en `crates/xmr-joint/ESCENARIOS.md`. No corre con `cargo test` pelado.

```bash
MONEROD=/ruta/monerod ./scripts/sanidad.sh
```

Ese script corre los tests del proyecto (sin los tests propios del `dkg-pedpop` vendido) y después el pago al 100%. Si `monerod` está en el `PATH`, `MONEROD` no hace falta.

Ese test fondea con dos transferencias (una por hot wallet) y el pago sí es FROST 2-de-2. El encierre de una sola tx sigue bloqueado por la firma de un solo spend key.

Siguiente paso, si lo piden: los escenarios 2–10 de esa lista, o pasar las rondas del DKG por el gossip.

## Qué no hacer sin que lo pidan

- Dar por emitida una transacción Monero, o meter Bitcoin.
- Android / APK. Otro proyecto (Orbot, mandante en PC).
- Reescribir iced/Tauri.
- Meter un servidor o un keyword que el usuario tipeé.
- Refactor cosmético de `main.rs`.
