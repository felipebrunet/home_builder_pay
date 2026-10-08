package cl.konstruado.app.ui.screens

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Acciones
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.ComoFunciona
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.DatoFila
import cl.konstruado.app.ui.Divisor
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.EstadoFila
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Plegable
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.SaldoGrande
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Tarjeta
import cl.konstruado.app.ui.TextoBoton
import cl.konstruado.app.ui.Tono
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAbrirArchivo
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.rememberGuardarArchivo
import cl.konstruado.app.ui.sondear
import cl.konstruado.app.ui.tonoDe
import uniffi.konstruado_ffi.DaemonPrueba

@Composable
private fun Restaurar(acciones: Acciones, banner: Banner) {
    val app = AppHolder.a
    val abrirSemilla = rememberAbrirArchivo(acciones) { app.restaurarSemilla(it) }
    val abrirShare = rememberAbrirArchivo(acciones) { app.restaurarShare(it) }
    val guardarObras = rememberGuardarArchivo(
        acciones, banner, { app.exportarObras() },
        "Guardé el respaldo de obras. No incluye seed ni share; puede estar desfasado vs el otro.",
    )
    val abrirObras = rememberAbrirArchivo(acciones) { app.importarObras(it) }
    Secundario("Recuperar las 25 palabras") { abrirSemilla() }
    Secundario("Recuperar un share") { abrirShare() }
    Secundario("Guardar respaldo de obras") { guardarObras("konstruado-obras.json") }
    Secundario("Recuperar respaldo de obras") { abrirObras() }
    ComoFunciona("Qué trae cada respaldo") {
        Ayuda("Las 25 palabras traen tu dirección personal. No traen la caja de la obra ni tu nombre en el trato.")
        Ayuda("Un share trae la caja de una obra que ya está en este equipo. Tiene que ser el tuyo: el del otro lado no sirve.")
        Ayuda("El respaldo de obras guarda el perfil (obras/ofertas) para reinstalar. Puede estar desfasado respecto al otro; la cadena y el share mandan para el dinero. No incluye seed ni share.")
    }
}

/** Resultado de la última prueba RPC en una fila de alto fijo. */
@Composable
fun PruebaRpcFila(p: DaemonPrueba?, probando: Boolean, sinProbar: String) {
    when {
        probando -> EstadoFila(Tono.Espera, "Consultando la punta del nodo…", enCurso = true)
        p == null -> EstadoFila(Tono.Apagado, sinProbar)
        p.ok -> EstadoFila(Tono.Ok, "RPC OK · bloque ${p.tip ?: "?"} · ${p.ms} ms")
        else -> EstadoFila(Tono.Error, "RPC falló (${p.ms} ms): ${p.mensaje}")
    }
}

@Composable
fun BilleteraScreen(banner: Banner) {
    val acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val r = sondear { app.billetera() } ?: run { Pista("Cargando…"); return }
    val b = r.getOrElse { ErrorTexto(it.humano()); return }
    var destino by remember { mutableStateOf("") }
    var monto by remember { mutableStateOf("") }
    val guardarPalabras = rememberGuardarArchivo(
        acciones, banner, { app.exportarSemilla() }, "Guardé las 25 palabras en el archivo que elegiste.",
    )
    val esDefecto = app.daemonEsDefecto()
    val pruebaNodo = sondear { app.ultimaPruebaDaemon() }?.getOrNull()
    val ocupado = b.buscando || b.enviando || b.retro != null

    // Saldo + estado (la fila de estado tiene alto fijo: escanear no mueve nada).
    Tarjeta {
        EstadoFila(tonoDe(b.estadoTono), b.estadoLinea, enCurso = ocupado)
        val dir = b.direccion
        if (dir == null) {
            Text("Todavía no hay billetera", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Ayuda("Se crean 25 palabras nuevas y quedan en el almacenamiento privado de la app. Tu Monero personal de stagenet; la caja de una obra es otra dirección, de las dos personas.")
            Primario("Crear billetera de stagenet") { acciones.correr("Billetera creada. Guardá las 25 palabras.") { app.crearSemilla() } }
            Ayuda(b.escala)
        } else {
            Ayuda("Saldo")
            SaldoGrande(b.total)
            DatoFila("Libre", "${b.libre} XMR", mono = true)
            DatoFila("Trabado (10 bloques)", "${b.trabado} XMR", mono = true)
        }
    }
    val dir = b.direccion
    if (dir == null) {
        Plegable("Respaldos y recuperación", "Recuperar 25 palabras, share u obras", abierta = true) {
            Restaurar(acciones, banner)
        }
        NodoPlegable(b.daemon, esDefecto, b.tip, b.visto, pruebaNodo, acciones)
        return
    }

    Tarjeta("Entradas") {
        if (b.movs.isEmpty()) Ayuda("Todavía no vimos entradas en esta billetera.")
        b.movs.forEachIndexed { i, m ->
            if (i > 0) Divisor()
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(m.monto, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodyMedium)
                Text(
                    m.detalle, style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f).padding(start = 12.dp),
                )
            }
        }
    }

    Tarjeta("Recibir") {
        Copiable("Tu dirección stagenet", dir)
        ComoFunciona("¿No aparece lo que mandaron?") {
            Ayuda("El scan no ve monedas más viejas que lo que ya miramos: si el faucet es viejo, pedí mirar más atrás en «Nodo / Avanzado».")
        }
    }

    Tarjeta("Enviar") {
        OutlinedTextField(destino, { destino = it }, label = { Text("Dirección de stagenet") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
        OutlinedTextField(monto, { monto = it }, label = { Text("Monto en XMR") }, placeholder = { Text("0.04") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Ayuda("Se reservan 0,001 XMR para el fee.")
            androidx.compose.foundation.layout.Spacer(Modifier.weight(1f))
            TextoBoton("Usar el máximo") {
                acciones.pedir({ app.maximoEnvio() }) { m ->
                    if (m == null) banner.error.value = "No hay saldo libre suficiente para el fee." else monto = m
                }
            }
        }
        Primario("Enviar", enabled = !b.enviando) { acciones.correr("Envío pedido. Mirá el estado arriba.") { app.enviar(destino, monto) } }
        // Último envío: una línea fija (el estado en curso va arriba).
        Ayuda(b.ultimo ?: "Sin envíos desde que abriste la app.", maxLines = 1)
    }

    Plegable("Respaldos y recuperación", "25 palabras, shares y obras") {
        Secundario("Guardar las 25 palabras") { guardarPalabras("konstruado-semilla.txt") }
        Ayuda("Junto a las palabras queda la altura de bloque del nodo; al recuperar, el scan parte de ahí (no desde el génesis).")
        Divisor()
        Restaurar(acciones, banner)
    }

    NodoPlegable(b.daemon, esDefecto, b.tip, b.visto, pruebaNodo, acciones)

    if (b.cajas.isNotEmpty()) {
        Plegable("Cajas de obras (2-de-2)", "${b.cajas.size} caja(s)") {
            b.cajas.forEachIndexed { i, c ->
                if (i > 0) Divisor()
                Text(c.obraNombre, fontWeight = FontWeight.SemiBold)
                CajaDatos(c.obraId, c.direccion, c.mirada, acciones)
                CajaRespaldo(c.obraId, c.direccion != null, acciones, banner)
            }
        }
    }
}

@Composable
private fun NodoPlegable(
    daemon: String,
    esDefecto: Boolean,
    tip: ULong?,
    visto: String,
    prueba: DaemonPrueba?,
    acciones: Acciones,
) {
    val app = AppHolder.a
    Plegable("Nodo / Avanzado", if (esDefecto) "Nodo público" else "Nodo propio") {
        Copiable(if (esDefecto) "Nodo (público)" else "Nodo (propio · cambialo en Cuenta)", daemon)
        PruebaRpcFila(prueba, false, "RPC sin probar · Cuenta → «Probar RPC del nodo»")
        DatoFila("Punta del nodo", tip?.toString() ?: "—", mono = true)
        Ayuda(visto, maxLines = 2)
        Secundario("Actualizar saldo") { acciones.correr { app.actualizarSaldo() } }
        Secundario("Mirar 200 bloques más atrás") { acciones.correr("Sumé 200 bloques hacia atrás.") { app.mirarAtras() } }
        ComoFunciona {
            Ayuda("El saldo sale de mirar la cadena con tu view key en el nodo activo. Lo nuevo se mira solo; lo anterior al primer bloque mirado, solo si lo pedís.")
            Ayuda("Un nodo de tu red local o Tailscale va directo, nunca por Tor ni por el SOCKS de Orbot.")
        }
    }
}

