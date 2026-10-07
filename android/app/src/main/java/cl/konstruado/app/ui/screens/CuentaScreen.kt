package cl.konstruado.app.ui.screens

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import cl.konstruado.app.AppHolder
import cl.konstruado.app.Prefs
import cl.konstruado.app.abrirOrbot
import cl.konstruado.app.appEnVpn
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Seccion
import cl.konstruado.app.ui.Titulo
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.sondear

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
    var avanzado by remember { mutableStateOf(false) }
    val hint = remember { app.orbotHint() }

    Titulo("Tu cuenta")
    Pista("El nombre y el rol se pueden cambiar. Las obras no se borran.")
    OutlinedTextField(nombre, { nombre = it }, label = { Text("Nombre") }, modifier = Modifier.fillMaxWidth())
    Row {
        FilterChip(selected = rol == "mandante", onClick = { rol = "mandante" }, label = { Text("Pago la obra") })
        FilterChip(selected = rol == "contratista", onClick = { rol = "contratista" }, label = { Text("La construyo") })
    }
    Primario("Guardar") { acciones.correr("Guardado.") { app.guardarCuenta(nombre, rol) } }

    Seccion("Red")
    val red = sondear { app.red() }?.getOrNull()
    red?.let { RedTarjeta(it) }
    red?.let { r ->
        if (r.destinos.isNotEmpty()) Pista("Marcando: ${r.destinos.joinToString(", ")}")
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Switch(checked = usarOrbot, onCheckedChange = { usarOrbot = it })
        Text("  Usar Orbot (SOCKS) y marcar la sala")
    }
    OutlinedTextField(host, { host = it }, label = { Text("SOCKS host") }, modifier = Modifier.fillMaxWidth())
    OutlinedTextField(port, { port = it }, label = { Text("SOCKS puerto") },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
    Secundario("Aplicar Orbot") {
        val p = port.toIntOrNull() ?: 9050
        Prefs.guardarOrbot(ctx, usarOrbot, host, p)
        acciones.correr(if (usarOrbot) "Orbot configurado. Marcando la sala…" else "Orbot apagado para la sala.") {
            if (usarOrbot) app.configurarSocks(host, p.toUShort()) else app.quitarDestino("sala")
        }
    }
    Secundario("Abrir Orbot") {
        if (!abrirOrbot(ctx)) banner.error.value = "Orbot no está instalado. Bajalo de F-Droid o Google Play."
    }
    hint.pasos.forEachIndexed { i, s -> Pista("${i + 1}. $s") }
    Pista("Sala: ${hint.onionSala}")

    Seccion("Nodo Monero (stagenet)")
    val daemonDefecto = remember { app.daemonPorDefecto() }
    var daemonUrl by remember {
        mutableStateOf(if (app.daemonEsDefecto()) "" else app.daemonActivo())
    }
    var probando by remember { mutableStateOf(false) }
    var pruebaLocal by remember { mutableStateOf(app.ultimaPruebaDaemon()) }
    val daemonTick = sondear {
        Triple(app.daemonActivo(), app.daemonEsDefecto(), app.ultimaPruebaDaemon())
    }?.getOrNull()
    val daemonActivo = daemonTick?.first ?: app.daemonActivo()
    val esDefecto = daemonTick?.second ?: app.daemonEsDefecto()
    val pruebaRemota = daemonTick?.third
    val prueba = pruebaLocal ?: pruebaRemota
    Pista(
        if (esDefecto) "Activo (público): $daemonActivo"
        else "Activo (propio): $daemonActivo"
    )
    Pista("Guardar fija el nodo para scan, saldo, fondeo y pago. «Usar por defecto» vuelve a $daemonDefecto.")
    Pista("Ejemplo LAN/Tailscale: http://192.168.1.83:38081 o http://100.x.y.z:38081 (RPC stagenet; el público usa :38089).")
    Pista("Un nodo de la red local o Tailscale va siempre directo, nunca por Tor ni por el SOCKS de Orbot.")
    val vpnAhora = appEnVpn(ctx)
    app.avisoVpnDaemon(vpnAhora == true)?.let { ErrorTexto(it) }
    OutlinedTextField(
        daemonUrl, { daemonUrl = it },
        label = { Text("URL del nodo (http://host:puerto)") },
        placeholder = { Text(daemonDefecto) },
        modifier = Modifier.fillMaxWidth(),
        singleLine = true,
    )
    Primario("Guardar nodo") {
        acciones.pedir({ app.fijarDaemon(daemonUrl.trim()) }) { u ->
            daemonUrl = u
            banner.ok.value = "Nodo guardado: $u. Todavía conviene probar el RPC abajo."
        }
    }
    Secundario("Usar por defecto") {
        acciones.pedir({ app.usarDaemonPorDefecto() }) { u ->
            daemonUrl = ""
            pruebaLocal = null
            banner.ok.value = "Volví al nodo público: $u"
        }
    }
    Pista("«Probar RPC del nodo» solo pide la punta (altura de bloque) por HTTP al nodo activo. No gasta monedas ni prueba la sala.")
    Secundario(if (probando) "Probando RPC…" else "Probar RPC del nodo", enabled = !probando) {
        probando = true
        banner.ok.value = null
        banner.error.value = null
        acciones.pedir(
            { app.probarDaemon(appEnVpn(ctx)) },
            alFinal = { probando = false },
        ) { r ->
            pruebaLocal = r
        }
    }
    if (probando) Pista("Consultando la punta del nodo…")
    prueba?.let { r ->
        Card(modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp)) {
            Column(Modifier.padding(12.dp)) {
                Text(
                    if (r.ok) "Última prueba RPC: OK" else "Última prueba RPC: falló",
                    style = MaterialTheme.typography.titleSmall,
                    color = if (r.ok) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
                )
                Text(r.mensaje, style = MaterialTheme.typography.bodyMedium)
                Pista("URL: ${r.url}")
                Pista("Ruta: ${r.ruta}")
                r.tip?.let { Pista("Punta (bloque): $it") }
                Pista("Tiempo: ${r.ms} ms")
            }
        }
    }

    TextButton(onClick = { avanzado = !avanzado }) {
        Text(if (avanzado) "Ocultar avanzado" else "Avanzado (pruebas TCP)")
    }
    if (avanzado) {
        Seccion("Destinos TCP")
        Pista("Solo para lab: emulador 10.0.2.2:17432, adb reverse → 127.0.0.1:17432, o LAN IP:17432.")
        destinos.toList().forEach { d ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(d, Modifier.weight(1f))
                TextButton(onClick = {
                    destinos.remove(d); Prefs.guardarDestinos(ctx, destinos.toList())
                    acciones.correr { app.quitarDestino(d) }
                }) { Text("Quitar") }
            }
        }
        OutlinedTextField(nuevo, { nuevo = it }, label = { Text("host:puerto") }, modifier = Modifier.fillMaxWidth())
        Secundario("Agregar destino") {
            val d = nuevo.trim()
            acciones.correr("Destino agregado.", alTerminar = {
                if (d !in destinos) destinos.add(d)
                Prefs.guardarDestinos(ctx, destinos.toList()); nuevo = ""
            }) { app.agregarDestino(d) }
        }
    }

    Seccion("Acerca de")
    Pista(remember { runCatching { app.version() }.getOrElse { it.humano() } })
    Pista("Solo stagenet. Datos y share FROST en el almacenamiento privado de la app (sin copia de seguridad de Android).")
}
