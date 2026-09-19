# Konstruado — contexto para una sesión nueva

Producto de escritorio (Dioxus) para un **trato de obra entre dos personas**, sin servidor. No es un chat. El mandante publica un aviso; el contratista lo ve y acepta o contraoferta. Monero sigue en stub (`2+2` en `xmr-joint`). No hay Bitcoin.

Hablamos en español. UI rioplatense/chilena (“Poné”, “te toca”) por defecto; el usuario puede pasar a **English** (ES/EN en la barra y en Cuenta). El trato no cambia.

## Crates

| Crate | Rol |
|---|---|
| `konstruado-core` | Dominio: persona, oferta, obra, partidas, contra, extra, recibo, fusión. Sin UI ni Tor. |
| `konstruado-net` | Encuentro: TCP local `17432`, gossip DHT, Tor propio + onion horneado. |
| `konstruado` | Ventana: pantallas, persistir, exportar, temas, idioma. |
| `xmr-joint` | Stub de encierro. No implementar cripto a menos que se pida. |

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
- Tablero central = solo **activas** (avisos sin tomar + obras en curso). Sidebar **Mis obras** = historial en las que participás, más reciente arriba (`actualizado`).
- **Te toca** = interacción de un trato abierto (contra, %, extra), no “alguien publicó”.
- Exportar constancia: `.txt` y `.pdf` desde el detalle de la obra.

## Cómo probar dos máquinas

```bash
git pull
cargo run
```

Clone del notebook: `git clone git@github.com-hbp:felipebrunet/home_builder_pay.git` (alias SSH `hbp_deploy`). En el notebook hay que tener GTK/WebKit/`libxdo-dev`/`tor`.

Primero mandante (esperar `sala abierta`). Después contratista. La primera vez Tor puede tardar en bootstrap.

## Qué no hacer sin que lo pidan

- Implementar Monero o Bitcoin.
- Reescribir iced/Tauri.
- Meter un servidor o un keyword que el usuario tipeé.
- Refactor cosmético de `main.rs`.
