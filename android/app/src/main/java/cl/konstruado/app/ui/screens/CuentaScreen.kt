package cl.konstruado.app.ui.screens

import cl.konstruado.app.ui.tr
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.Prefs
import cl.konstruado.app.abrirOrbot
import cl.konstruado.app.appEnVpn
import cl.konstruado.app.orbotInstalado
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.ComoFunciona
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.DatoFila
import cl.konstruado.app.ui.Divisor
import cl.konstruado.app.ui.EnlaceGithub
import cl.konstruado.app.ui.Idioma
import cl.konstruado.app.ui.EstadoFila
import cl.konstruado.app.ui.Plegable
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Tarjeta
import cl.konstruado.app.ui.TextoBoton
import cl.konstruado.app.ui.Tono
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.sondear
import uniffi.konstruado_ffi.SalaEstado

@Composable
fun CuentaScreen(banner: Banner) {
    val ctx = LocalContext.current
    val acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val perfil0 = remember { app.perfil() }
    var nombre by remember { mutableStateOf(perfil0.nombre) }
    var rol by remember { mutableStateOf(perfil0.rol) }
    var usarOrbot by remember { mutableStateOf(Prefs.usarOrbot(ctx)) }
    var host by remember { mutableStateOf(Prefs.socksHost(ctx)) }
    var port by remember { mutableStateOf(Prefs.socksPort(ctx).toString()) }
    val destinos = remember { mutableStateListOf<String>().apply { addAll(Prefs.destinos(ctx)) } }
    var nuevo by remember { mutableStateOf("") }
    var probandoOrbot by remember { mutableStateOf(false) }
    val hint = remember(Idioma.en) { app.orbotHint() }

    Tarjeta(tr("Tu cuenta", "Your account")) {
        OutlinedTextField(nombre, { nombre = it }, label = { Text(tr("Nombre", "Name")) }, modifier = Modifier.fillMaxWidth(), singleLine = true)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(selected = rol == "mandante", onClick = { rol = "mandante" }, label = { Text(tr("Pago la obra", "I pay for the job")) })
            FilterChip(selected = rol == "contratista", onClick = { rol = "contratista" }, label = { Text(tr("La construyo", "I build it")) })
        }
        Primario(tr("Guardar", "Save")) { acciones.correr(tr("Guardado.", "Saved.")) { app.guardarCuenta(nombre, rol) } }
        Ayuda(tr("El nombre y el rol se pueden cambiar. Las obras no se borran.", "Name and role can be changed. Jobs are not deleted."))
    }

    // ------------------------------------------------------------ idioma
    Tarjeta(tr("Idioma", "Language")) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("es" to "Español", "en" to "English").forEach { (codigo, nombreIdioma) ->
                FilterChip(
                    selected = Idioma.codigo == codigo,
                    onClick = {
                        // Se aplica ya (UI y textos del motor) y queda en el perfil, como en el escritorio.
                        Idioma.en = codigo == "en"
                        acciones.correr { app.fijarIdioma(codigo) }
                    },
                    label = { Text(nombreIdioma) },
                )
            }
        }
        Ayuda(tr("El idioma de la app. El trato es el mismo para los dos.", "The app's language. The deal is the same for both sides."))
    }

    // ------------------------------------------------------------ red
    val tick = sondear { app.red() to app.estadoSala(orbotInstalado(ctx)) }?.getOrNull()
    Tarjeta(tr("Red", "Network")) {
        if (tick != null) {
            val (red, salaViva) = tick
            // «Probar Orbot» actualiza el diagnóstico en el motor; el sondeo lo trae al toque.
            val sala = if (!probandoOrbot) salaViva else SalaEstado(
                tipo = "probando", tono = "espera", titulo = tr("Probando Orbot…", "Testing Orbot…"),
                detalle = tr("Saludo SOCKS5 a $host:$port.", "SOCKS5 handshake to $host:$port."), socks = salaViva.socks,
            )
            RedTarjeta(red, sala)
            Ayuda(
                if (red.destinos.isEmpty()) tr("Marcando: —", "Dialing: —") else tr("Marcando: ", "Dialing: ") + red.destinos.joinToString(", "),
                maxLines = 1,
            )
        } else {
            EstadoFila(Tono.Espera, tr("Leyendo la red…", "Reading the network…"), enCurso = true)
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(tr("Usar Orbot (SOCKS) y marcar la sala", "Use Orbot (SOCKS) and dial the room"), Modifier.weight(1f))
            Switch(checked = usarOrbot, onCheckedChange = { usarOrbot = it })
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(host, { host = it }, label = { Text("SOCKS host") }, singleLine = true, modifier = Modifier.weight(2f))
            OutlinedTextField(port, { port = it }, label = { Text(tr("Puerto", "Port")) }, singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.weight(1f))
        }
        Primario(tr("Aplicar", "Apply")) {
            val p = port.toIntOrNull() ?: 9050
            Prefs.guardarOrbot(ctx, usarOrbot, host, p)
            val instalado = orbotInstalado(ctx)
            acciones.pedir({
                if (usarOrbot) {
                    app.configurarSocks(host, p.toUShort())
                    // Respuesta inmediata: prueba el SOCKS sin esperar al próximo intento a la sala.
                    app.probarOrbot(instalado)
                } else {
                    app.quitarDestino("sala")
                    null
                }
            }) { s ->
                banner.ok.value = if (usarOrbot) tr("Orbot configurado: ", "Orbot set: ") + (s?.titulo ?: tr("marcando la sala…", "dialing the room…")) else tr("Orbot apagado para la sala.", "Orbot is off for the room.")
            }
        }
        Secundario(if (probandoOrbot) tr("Probando…", "Testing…") else tr("Probar Orbot", "Test Orbot"), enabled = usarOrbot && !probandoOrbot) {
            probandoOrbot = true
            val instalado = orbotInstalado(ctx)
            acciones.pedir({ app.probarOrbot(instalado) }, alFinal = { probandoOrbot = false }) { }
        }
        TextoBoton(tr("Abrir Orbot", "Open Orbot")) {
            if (!abrirOrbot(ctx)) banner.error.value = tr("Orbot no está instalado. Bajalo de F-Droid o Google Play.", "Orbot is not installed. Get it from F-Droid or Google Play.")
        }
        ComoFunciona(tr("Cómo funciona Orbot y la sala", "How Orbot and the room work")) {
            hint.pasos.forEachIndexed { i, s -> Ayuda("${i + 1}. $s") }
            Ayuda(tr("«Probar Orbot» solo abre el SOCKS ($host:$port) y hace el saludo SOCKS5; no llama a la sala.", "“Test Orbot” only opens the SOCKS proxy ($host:$port) and does the SOCKS5 handshake; it does not call the room."))
            Copiable(tr("Sala (onion)", "Room (onion)"), hint.onionSala)
        }
    }

    // ------------------------------------------------------------ nodo
    val daemonDefecto = remember { app.daemonPorDefecto() }
    var daemonUrl by remember { mutableStateOf(if (app.daemonEsDefecto()) "" else app.daemonActivo()) }
    var probando by remember { mutableStateOf(false) }
    var pruebaLocal by remember { mutableStateOf(app.ultimaPruebaDaemon()) }
    val daemonTick = sondear { Triple(app.daemonActivo(), app.daemonEsDefecto(), app.ultimaPruebaDaemon()) }?.getOrNull()
    val daemonActivo = daemonTick?.first ?: app.daemonActivo()
    val esDefecto = daemonTick?.second ?: app.daemonEsDefecto()
    val prueba = pruebaLocal ?: daemonTick?.third
    Tarjeta(tr("Nodo Monero (stagenet)", "Monero node (stagenet)")) {
        DatoFila(if (esDefecto) tr("Activo (público)", "Active (public)") else tr("Activo (propio)", "Active (own)"), daemonActivo, mono = true)
        app.avisoVpnDaemon(appEnVpn(ctx) == true)?.let { EstadoFila(Tono.Error, it) }
        OutlinedTextField(
            daemonUrl, { daemonUrl = it },
            label = { Text(tr("URL del nodo (http://host:puerto)", "Node URL (http://host:port)")) },
            placeholder = { Text(daemonDefecto, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        Primario(tr("Guardar nodo", "Save node")) {
            acciones.pedir({ app.fijarDaemon(daemonUrl.trim()) }) { u ->
                daemonUrl = u
                pruebaLocal = null
                banner.ok.value = tr("Nodo guardado: $u. Conviene probar el RPC.", "Node saved: $u. Better test the RPC.")
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(Modifier.weight(1f)) {
                Secundario(if (probando) tr("Probando…", "Testing…") else tr("Probar RPC", "Test RPC"), enabled = !probando) {
                    probando = true
                    banner.ok.value = null
                    banner.error.value = null
                    acciones.pedir({ app.probarDaemon(appEnVpn(ctx)) }, alFinal = { probando = false }) { r -> pruebaLocal = r }
                }
            }
            Row(Modifier.weight(1f)) {
                Secundario(tr("Usar por defecto", "Use default"), enabled = !esDefecto) {
                    acciones.pedir({ app.usarDaemonPorDefecto() }) { u ->
                        daemonUrl = ""
                        pruebaLocal = null
                        banner.ok.value = tr("Volví al nodo público: $u", "Back to the public node: $u")
                    }
                }
            }
        }
        // Resultado de la prueba en una fila fija: probar no agrega ni quita renglones.
        PruebaRpcFila(prueba, probando, tr("RPC sin probar todavía", "RPC not tested yet"))
        Ayuda(prueba?.let { tr("Ruta: ${it.ruta}", "Route: ${it.ruta}") } ?: tr("Ruta: —", "Route: —"), maxLines = 1)
        ComoFunciona {
            Ayuda(tr("«Probar RPC» solo pide la punta (altura de bloque) por HTTP al nodo activo. No gasta monedas ni prueba la sala.", "“Test RPC” only asks the active node for its tip (block height) over HTTP. It spends nothing and does not test the room."))
            Ayuda(tr("Guardar fija el nodo para scan, saldo, fondeo y pago. «Usar por defecto» vuelve a $daemonDefecto.", "Save sets the node for scan, balance, funding and payment. “Use default” goes back to $daemonDefecto."))
            Ayuda(tr("Ejemplo LAN/Tailscale: http://192.168.1.83:38081 o http://100.x.y.z:38081 (RPC stagenet; el público usa :38089).", "LAN/Tailscale example: http://192.168.1.83:38081 or http://100.x.y.z:38081 (stagenet RPC; the public one uses :38089)."))
            Ayuda(tr("Un nodo de la red local o Tailscale va siempre directo, nunca por Tor ni por el SOCKS de Orbot.", "A LAN or Tailscale node is always reached directly, never over Tor or Orbot's SOCKS."))
        }
    }

    Plegable(tr("Avanzado", "Advanced"), tr("Destinos TCP para pruebas de laboratorio", "TCP targets for lab tests")) {
        Ayuda(tr("Solo para lab: emulador 10.0.2.2:17432, adb reverse → 127.0.0.1:17432, o LAN IP:17432.", "Lab only: emulator 10.0.2.2:17432, adb reverse → 127.0.0.1:17432, or LAN IP:17432."))
        destinos.toList().forEach { d ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(d, Modifier.weight(1f))
                TextoBoton(tr("Quitar", "Remove")) {
                    destinos.remove(d); Prefs.guardarDestinos(ctx, destinos.toList())
                    acciones.correr { app.quitarDestino(d) }
                }
            }
        }
        OutlinedTextField(nuevo, { nuevo = it }, label = { Text(tr("host:puerto", "host:port")) }, modifier = Modifier.fillMaxWidth(), singleLine = true)
        Secundario(tr("Agregar destino", "Add target")) {
            val d = nuevo.trim()
            acciones.correr(tr("Destino agregado.", "Target added."), alTerminar = {
                if (d !in destinos) destinos.add(d)
                Prefs.guardarDestinos(ctx, destinos.toList()); nuevo = ""
            }) { app.agregarDestino(d) }
        }
    }

    Tarjeta(tr("Acerca de", "About")) {
        Ayuda(remember { runCatching { app.version() }.getOrElse { it.humano() } })
        EnlaceGithub(remember { app.repositorio() })
        Divisor()
        Ayuda(tr("Solo stagenet. Datos y share FROST en el almacenamiento privado de la app (sin copia de seguridad de Android).", "Stagenet only. Data and FROST share live in the app's private storage (no Android backup)."))
    }
}
