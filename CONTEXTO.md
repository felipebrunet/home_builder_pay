# Konstruado — contexto para una sesión nueva

Producto de escritorio (Dioxus) para un **trato de obra entre dos personas**, sin servidor. No es un chat. El mandante publica un aviso; el contratista lo ve y acepta o contraoferta. Monero vive en `xmr-joint` y la ventana lo usa en stagenet contra `https://stagenet.xmr.kernal.eu:38089`. `monero_fn` sigue en 4: el dominio no finge el fondeo. Encerrada y Pagada se marcan cuando un scan local ve la transacción en un bloque. No hay Bitcoin. No se probó un broadcast.

Hablamos en español. UI rioplatense/chilena (“Poné”, “te toca”) por defecto; el usuario puede pasar a **English** (ES/EN en la barra y en Cuenta). El trato no cambia.

## Crates

| Crate | Rol |
|---|---|
| `konstruado-core` | Dominio: persona, oferta, obra, partidas, contra, extra, recibo, fusión. Sin UI ni Tor. |
| `konstruado-net` | Encuentro: TCP local `17432`, gossip DHT, Tor propio + onion horneado. |
| `konstruado` | Ventana: pantallas, persistir, exportar, temas, idioma. |
| `xmr-joint` | Semilla de 25 palabras, DKG 2-de-2, fondeo atómico y gasto con dos destinos. La ventana lo llama desde `caja.rs`. |

El split está bien. No hace falta un refactor grande. `main.rs` es largo; partir pantallas solo si duele.

## Roles y flujo

- **Mandante** paga. Publica nombre, trabajo, garantía sugerida, textos de partidas. Abre la sala Tor.
- **Contratista** construye. Solo busca sala. Ve avisos, acepta o propone otra garantía.
- Garantía de la obra **divide exacto** el trabajo (10 000 / 2 000 → 5 partidas). Cada partida original encierra esa garantía por lado.
- **Contra:** el contratista propone otra garantía. El mandante confirma o **No aceptar esta garantía** (el aviso vuelve al tablero con lo original).
- **Partida:** el primero propone el encierre (sesión viva). El segundo, con **Confirmar y fondear**, arma el fondeo; Encerrada llega cuando el scan ve esa tx. El contratista avisa término con % y nota (máx. 50) → el otro acepta o contraoferta %. **Aceptar y pagar** arma el gasto; Pagada y el recibo llegan cuando el scan ve esa tx. El click no llama al dominio.
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

## Monero (stagenet)

`third_party/monero-oxide` es el snapshot `731657ae` con tres parches (CLSAG con máscara, `sum_output_masks` público, `input_sum` / `external_payments`). No van secretos ahí.

Daemon fijo, sin campo en la ventana: `https://stagenet.xmr.kernal.eu:38089` (HTTPS, RPC restringido, `webpki-roots`). El binario `stagenet` usa el mismo por defecto. `check` llegó a la punta por ese URL. `get_block`, `get_outs` y `send_raw_transaction` no se probaron: un fondeo en vivo puede rechazarse.

1 unidad del trato = 20 000 000 piconero = 0,00002 XMR (`PICONERO_POR_UNIDAD`). La garantía de 2000 son 0,04 XMR por lado.

Una caja 2-de-2 por obra, al acordar. Mandante = índice FROST 1 y paga el fee del fondeo. Contratista = índice 2. El porcentaje del gasto se reparte off-chain sobre `2 × capital`.

Cuenta crea la semilla y puede guardar las 25 palabras. **Billetera** (barra de arriba) muestra el saldo, la dirección para recibir y un envío de la billetera personal. El scan de esa pantalla arranca 40 bloques atrás y se puede pedir de a 200 más; no ve monedas más viejas hasta entonces. El mempool no cuenta. El candado estándar de ~10 bloques es aparte: enviar o gastar puede fallar hasta que las salidas destraben, y se muestra el error. Un envío en vivo puede rechazarse si el nodo no entrega anillos o no acepta la publicación.

Archivos bajo `KONSTRUADO_DATOS/xmr`, modo 0600, fuera de `estado.json`: `semilla.txt` y `{obra}.share`. La semilla no viaja. Los mensajes de la caja van en `Caja` al otro, no al DHT. La view compartida va una vez en el anuncio y queda en el share. La semilla personal no reconstruye el share.

```bash
cargo run -p xmr-joint --bin stagenet -- init --dir DIR
cargo run -p xmr-joint --bin stagenet -- dkg --dir DIR --obra ID
cargo run -p xmr-joint --bin stagenet -- check
```

`fund` y `spend` piden `--capital` en piconero y no publican sin `--broadcast`. No se hizo un broadcast. Hacen falta monedas de faucet en las dos billeteras personales, y el lookback de 40 no ve salidas viejas. Secretos nuevos cada `init`.

## Qué no hacer sin que lo pidan

- Marcar Encerrada o Pagada sin ver la tx en un bloque, o poner semilla, share o view en el gossip o en `estado.json`.
- Un campo para cambiar el daemon. La ventana usa solo `STAGENET_DAEMON`.
- Bitcoin.
- Reutilizar secretos de laboratorio o commitear semillas / shares.
- Android / APK. Otro proyecto (Orbot, mandante en PC).
- Reescribir iced/Tauri.
- Meter un servidor o un keyword que el usuario tipeé.
- Refactor cosmético de `main.rs`.
