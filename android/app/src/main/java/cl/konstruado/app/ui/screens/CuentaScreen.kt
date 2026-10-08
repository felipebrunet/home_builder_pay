package cl.konstruado.app.ui.screens

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
    val hint = remember { app.orbotHint() }

    Tarjeta("Tu cuenta") {
        OutlinedTextField(nombre, { nombre = it }, label = { Text("Nombre") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(selected = rol == "mandante", onClick = { rol = "mandante" }, label = { Text("Pago la obra") })
            FilterChip(selected = rol == "contratista", onClick = { rol = "contratista" }, label = { Text("La construyo") })
        }
        Primario("Guardar") { acciones.correr("Guardado.") { app.guardarCuenta(nombre, rol) } }
        Ayuda("El nombre y el rol se pueden cambiar. Las obras no se borran.")
    }

    // ------------------------------------------------------------ red
    val tick = sondear { app.red() to app.estadoSala(orbotInstalado(ctx)) }?.getOrNull()
    Tarjeta("Red") {
        if (tick != null) {
            val (red, salaViva) = tick
            // «Probar Orbot» actualiza el diagnóstico en el motor; el sondeo lo trae al toque.
            val sala = if (!probandoOrbot) salaViva else SalaEstado(
                tipo = "probando", tono = "espera", titulo = "Probando Orbot…",
                detalle = "Saludo SOCKS5 a $host:$port.", socks = salaViva.socks,
            )
            RedTarjeta(red, sala)
            Ayuda(
                if (red.destinos.isEmpty()) "Marcando: —" else "Marcando: ${red.destinos.joinToString(", ")}",
                maxLines = 1,
            )
        } else {
            EstadoFila(Tono.Espera, "Leyendo la red…", enCurso = true)
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Usar Orbot (SOCKS) y marcar la sala", Modifier.weight(1f))
            Switch(checked = usarOrbot, onCheckedChange = { usarOrbot = it })
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(host, { host = it }, label = { Text("SOCKS host") }, singleLine = true, modifier = Modifier.weight(2f))
            OutlinedTextField(port, { port = it }, label = { Text("Puerto") }, singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.weight(1f))
        }
        Primario("Aplicar") {
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
                banner.ok.value = if (usarOrbot) "Orbot configurado: ${s?.titulo ?: "marcando la sala…"}" else "Orbot apagado para la sala."
            }
        }
        Secundario(if (probandoOrbot) "Probando…" else "Probar Orbot", enabled = usarOrbot && !probandoOrbot) {
            probandoOrbot = true
            val instalado = orbotInstalado(ctx)
            acciones.pedir({ app.probarOrbot(instalado) }, alFinal = { probandoOrbot = false }) { }
        }
        TextoBoton("Abrir Orbot") {
            if (!abrirOrbot(ctx)) banner.error.value = "Orbot no está instalado. Bajalo de F-Droid o Google Play."
        }
        ComoFunciona("Cómo funciona Orbot y la sala") {
            hint.pasos.forEachIndexed { i, s -> Ayuda("${i + 1}. $s") }
            Ayuda("«Probar Orbot» solo abre el SOCKS ($host:$port) y hace el saludo SOCKS5; no llama a la sala.")
            Copiable("Sala (onion)", hint.onionSala)
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
    Tarjeta("Nodo Monero (stagenet)") {
        DatoFila(if (esDefecto) "Activo (público)" else "Activo (propio)", daemonActivo, mono = true)
        app.avisoVpnDaemon(appEnVpn(ctx) == true)?.let { EstadoFila(Tono.Error, it) }
        OutlinedTextField(
            daemonUrl, { daemonUrl = it },
            label = { Text("URL del nodo (http://host:puerto)") },
            placeholder = { Text(daemonDefecto, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        Primario("Guardar nodo") {
            acciones.pedir({ app.fijarDaemon(daemonUrl.trim()) }) { u ->
                daemonUrl = u
                pruebaLocal = null
                banner.ok.value = "Nodo guardado: $u. Conviene probar el RPC."
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(Modifier.weight(1f)) {
                Secundario(if (probando) "Probando…" else "Probar RPC", enabled = !probando) {
                    probando = true
                    banner.ok.value = null
                    banner.error.value = null
                    acciones.pedir({ app.probarDaemon(appEnVpn(ctx)) }, alFinal = { probando = false }) { r -> pruebaLocal = r }
                }
            }
            Row(Modifier.weight(1f)) {
                Secundario("Usar por defecto", enabled = !esDefecto) {
                    acciones.pedir({ app.usarDaemonPorDefecto() }) { u ->
                        daemonUrl = ""
                        pruebaLocal = null
                        banner.ok.value = "Volví al nodo público: $u"
                    }
                }
            }
        }
        // Resultado de la prueba en una fila fija: probar no agrega ni quita renglones.
        PruebaRpcFila(prueba, probando, "RPC sin probar todavía")
        Ayuda(prueba?.let { "Ruta: ${it.ruta}" } ?: "Ruta: —", maxLines = 1)
        ComoFunciona {
            Ayuda("«Probar RPC» solo pide la punta (altura de bloque) por HTTP al nodo activo. No gasta monedas ni prueba la sala.")
            Ayuda("Guardar fija el nodo para scan, saldo, fondeo y pago. «Usar por defecto» vuelve a $daemonDefecto.")
            Ayuda("Ejemplo LAN/Tailscale: http://192.168.1.83:38081 o http://100.x.y.z:38081 (RPC stagenet; el público usa :38089).")
            Ayuda("Un nodo de la red local o Tailscale va siempre directo, nunca por Tor ni por el SOCKS de Orbot.")
        }
    }

    Plegable("Avanzado", "Destinos TCP para pruebas de laboratorio") {
        Ayuda("Solo para lab: emulador 10.0.2.2:17432, adb reverse → 127.0.0.1:17432, o LAN IP:17432.")
        destinos.toList().forEach { d ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(d, Modifier.weight(1f))
                TextoBoton("Quitar") {
                    destinos.remove(d); Prefs.guardarDestinos(ctx, destinos.toList())
                    acciones.correr { app.quitarDestino(d) }
                }
            }
        }
        OutlinedTextField(nuevo, { nuevo = it }, label = { Text("host:puerto") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
        Secundario("Agregar destino") {
            val d = nuevo.trim()
            acciones.correr("Destino agregado.", alTerminar = {
                if (d !in destinos) destinos.add(d)
                Prefs.guardarDestinos(ctx, destinos.toList()); nuevo = ""
            }) { app.agregarDestino(d) }
        }
    }

    Tarjeta("Acerca de") {
        Ayuda(remember { runCatching { app.version() }.getOrElse { it.humano() } })
        Divisor()
        Ayuda("Solo stagenet. Datos y share FROST en el almacenamiento privado de la app (sin copia de seguridad de Android).")
    }
}
