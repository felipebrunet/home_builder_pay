package cl.konstruado.app.ui

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import cl.konstruado.app.AppHolder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.konstruado_ffi.PrecioVista

/**
 * Precio USD/XMR (Publicar obra, encerrar partida, Cuenta).
 *
 * El motor lo mantiene al día en segundo plano (por Orbot si está). Acá se ve el
 * estado (qué fuente falló y por qué), se puede actualizar ya, reintentar sin Tor
 * solo si el usuario lo pide (avisando que la API ve la IP) o escribirlo a mano.
 * Con [ajustes], también el permiso de pedirlo siempre sin Tor si Tor falla.
 */
@Composable
fun PanelPrecio(banner: Banner, ajustes: Boolean = false) {
    val app = AppHolder.a
    val acciones = rememberAcciones(banner)
    var v by remember { mutableStateOf<PrecioVista?>(null) }
    var pidiendo by remember { mutableStateOf(false) }
    var manualAbierto by remember { mutableStateOf(false) }
    var manual by remember { mutableStateOf("") }
    var siempre by remember { mutableStateOf(app.precioSinTorSiempre()) }
    LaunchedEffect(Idioma.en) {
        while (true) {
            if (!pidiendo) v = withContext(Dispatchers.IO) { app.precioVista() }
            delay(3_000)
        }
    }
    val p = v ?: run { Pista(tr("Leyendo el precio…", "Reading the price…")); return }
    when {
        p.error != null && !p.listo -> EstadoTarjeta(Tono.Error, tr("Sin precio de XMR", "No XMR price"), p.texto, maxLineas = 10)
        !p.listo -> EstadoTarjeta(Tono.Espera, tr("Precio de XMR", "XMR price"), p.texto, enCurso = true, maxLineas = 6)
        else -> Ayuda(p.texto)
    }
    Secundario(if (pidiendo) tr("Pidiendo el precio…", "Fetching the price…") else tr("Actualizar precio", "Update price"), enabled = !pidiendo) {
        pidiendo = true
        acciones.pedir({ app.actualizarPrecio() }, alFinal = { pidiendo = false }) { v = it }
    }
    if (p.ofrecerSinTor && !pidiendo) {
        EstadoTarjeta(Tono.Espera, tr("¿Reintentar sin Tor?", "Retry without Tor?"), p.avisoSinTor, maxLineas = 4)
        Secundario(tr("Probar sin Tor esta vez", "Try without Tor this time")) {
            pidiendo = true
            acciones.pedir({ app.actualizarPrecioSinTor() }, alFinal = { pidiendo = false }) { v = it }
        }
    }
    if (!p.listo || p.error != null) {
        TextoBoton(if (manualAbierto) tr("Cerrar el precio a mano", "Close manual price") else tr("Escribir el precio a mano", "Type the price by hand")) {
            manualAbierto = !manualAbierto
        }
    }
    if (manualAbierto && (!p.listo || p.error != null)) {
        OutlinedTextField(
            manual, { manual = it },
            label = { Text(tr("USD por 1 XMR", "USD per 1 XMR")) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Ayuda(tr(
            "Queda fijo al encerrar, igual que un precio leído. El otro ve que fue escrito a mano antes de confirmar.",
            "It is fixed when locking, like a fetched price. The other side sees it was typed by hand before confirming.",
        ))
        Primario(tr("Usar este precio", "Use this price")) {
            acciones.pedir({ app.fijarPrecioManual(manual) }) {
                v = it
                manualAbierto = false
                manual = ""
                banner.ok.value = tr("Precio a mano guardado.", "Manual price saved.")
            }
        }
    }
    if (ajustes) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(tr("Si por Tor no hay precio, pedirlo sin Tor sin preguntar", "If there is no price over Tor, fetch it without Tor without asking"), Modifier.weight(1f))
            Switch(checked = siempre, onCheckedChange = {
                siempre = it
                app.fijarPrecioSinTorSiempre(it)
            })
        }
        Ayuda(p.avisoSinTor)
    }
}
