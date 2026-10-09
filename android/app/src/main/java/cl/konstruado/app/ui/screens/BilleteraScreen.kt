package cl.konstruado.app.ui.screens

import cl.konstruado.app.ui.tr
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
import cl.konstruado.app.ui.sondear
import cl.konstruado.app.ui.tonoDe
import uniffi.konstruado_ffi.DaemonPrueba

/** Importación de los respaldos sueltos de 0.2.7 o antes (Avanzado). */
@Composable
private fun RestaurarViejos(acciones: Acciones) {
    val app = AppHolder.a
    val abrirSemilla = rememberAbrirArchivo(acciones) { app.restaurarSemilla(it) }
    val abrirShare = rememberAbrirArchivo(acciones) { app.restaurarShare(it) }
    val abrirObras = rememberAbrirArchivo(acciones) { app.importarObras(it) }
    Secundario(tr("Importar las 25 palabras (.txt)", "Import the 25 words (.txt)")) { abrirSemilla() }
    Secundario(tr("Importar un share (.share)", "Import a share (.share)")) { abrirShare() }
    Secundario(tr("Importar obras (.json)", "Import jobs (.json)")) { abrirObras() }
    ComoFunciona(tr("Qué trae cada archivo viejo", "What each old file contains")) {
        Ayuda(tr("Las 25 palabras traen tu dirección personal. No traen la caja de la obra ni tu nombre en el trato.", "The 25 words bring your personal address. They do not bring the job's box or your name in the deal."))
        Ayuda(tr("Un share trae la caja de una obra que ya está en este equipo. Tiene que ser el tuyo: el del otro lado no sirve.", "A share brings the box of a job already on this device. It must be yours: the other side's does not work."))
        Ayuda(tr("El JSON de obras trae obras y ofertas. Puede estar desfasado respecto al otro; la cadena y el share mandan para el dinero.", "The jobs JSON brings jobs and offers. It may lag behind the other side; the chain and the share rule for money."))
    }
}

/** «Respaldos y recuperación»: respaldo completo, restaurar y lo viejo en Avanzado. */
@Composable
private fun Respaldos(acciones: Acciones, banner: Banner, hayCuenta: Boolean) {
    if (hayCuenta) {
        RespaldoCompleto(acciones, banner)
        Divisor()
        VerSemilla(acciones, banner)
        Divisor()
    }
    RestaurarRespaldo(acciones)
    Divisor()
    Plegable(tr("Avanzado", "Advanced"), tr("Importar respaldos sueltos (0.2.7 o antes)", "Import standalone backups (0.2.7 or earlier)")) {
        RestaurarViejos(acciones)
    }
}

/** Resultado de la última prueba RPC en una fila de alto fijo. */
@Composable
fun PruebaRpcFila(p: DaemonPrueba?, probando: Boolean, sinProbar: String) {
    when {
        probando -> EstadoFila(Tono.Espera, tr("Consultando la punta del nodo…", "Asking the node for its tip…"), enCurso = true)
        p == null -> EstadoFila(Tono.Apagado, sinProbar)
        p.ok -> EstadoFila(Tono.Ok, tr("RPC OK · bloque ", "RPC OK · block ") + "${p.tip ?: "?"} · ${p.ms} ms")
        else -> EstadoFila(Tono.Error, tr("RPC falló (${p.ms} ms): ${p.mensaje}", "RPC failed (${p.ms} ms): ${p.mensaje}"))
    }
}

@Composable
fun BilleteraScreen(banner: Banner) {
    val acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val r = sondear { app.billetera() } ?: run { Pista(tr("Cargando…", "Loading…")); return }
    val b = r.getOrElse { ErrorTexto(it.humano()); return }
    var destino by remember { mutableStateOf("") }
    var monto by remember { mutableStateOf("") }
    val esDefecto = app.daemonEsDefecto()
    val pruebaNodo = sondear { app.ultimaPruebaDaemon() }?.getOrNull()
    val ocupado = b.buscando || b.enviando || b.retro != null

    // Saldo + estado (la fila de estado tiene alto fijo: escanear no mueve nada).
    Tarjeta {
        EstadoFila(tonoDe(b.estadoTono), b.estadoLinea, enCurso = ocupado)
        val dir = b.direccion
        if (dir == null) {
            Text(tr("Todavía no hay billetera", "No wallet yet"), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Ayuda(tr("Se crean 25 palabras nuevas y quedan en el almacenamiento privado de la app. Tu Monero personal de stagenet; la caja de una obra es otra dirección, de las dos personas.", "25 new words are created and kept in the app's private storage. Your personal stagenet Monero; a job's box is a different address, owned by both people."))
            Primario(tr("Crear billetera de stagenet", "Create stagenet wallet")) { acciones.correr(tr("Billetera creada. Exportá el respaldo completo (abajo, en Respaldos y recuperación).", "Wallet created. Export the full backup (below, in Backups and recovery).")) { app.crearSemilla() } }
            Ayuda(b.escala)
        } else {
            Ayuda(tr("Saldo", "Balance"))
            SaldoGrande(b.total)
            DatoFila(tr("Libre", "Free"), "${b.libre} XMR", mono = true)
            DatoFila(tr("Trabado (10 bloques)", "Locked (10 blocks)"), "${b.trabado} XMR", mono = true)
        }
    }
    val dir = b.direccion
    if (dir == null) {
        Plegable(tr("Respaldos y recuperación", "Backups and recovery"), tr("Restaurar desde el respaldo completo", "Restore from the full backup"), abierta = true) {
            Respaldos(acciones, banner, hayCuenta = true)
        }
        NodoPlegable(b.daemon, esDefecto, b.tip, b.visto, pruebaNodo, acciones)
        return
    }

    Tarjeta(tr("Entradas", "Incoming")) {
        if (b.movs.isEmpty()) Ayuda(tr("Todavía no vimos entradas en esta billetera.", "No incoming funds seen in this wallet yet."))
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

    Tarjeta(tr("Recibir", "Receive")) {
        Copiable(tr("Tu dirección stagenet", "Your stagenet address"), dir)
        ComoFunciona(tr("¿No aparece lo que mandaron?", "Not seeing what was sent?")) {
            Ayuda(tr("El scan no ve monedas más viejas que lo que ya miramos: si el faucet es viejo, pedí mirar más atrás en «Nodo / Avanzado».", "The scan does not see coins older than what was already scanned: if the faucet payment is old, scan further back under “Node / Advanced”."))
        }
    }

    Tarjeta(tr("Enviar", "Send")) {
        OutlinedTextField(destino, { destino = it }, label = { Text(tr("Dirección de stagenet", "Stagenet address")) }, modifier = Modifier.fillMaxWidth(), singleLine = true)
        OutlinedTextField(monto, { monto = it }, label = { Text(tr("Monto en XMR", "Amount in XMR")) }, placeholder = { Text("0.04") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
        Ayuda(b.ayudaEnvio)
        Row(verticalAlignment = Alignment.CenterVertically) {
            androidx.compose.foundation.layout.Spacer(Modifier.weight(1f))
            TextoBoton(tr("Usar el máximo", "Use the maximum")) {
                acciones.pedir({ app.maximoEnvio() }) { m ->
                    if (m == null) banner.error.value = tr("Todavía no hay saldo libre para enviar.", "No free balance to send yet.") else monto = m
                }
            }
        }
        Primario(tr("Enviar", "Send"), enabled = !b.enviando) { acciones.correr(tr("Envío pedido. Mirá el estado arriba.", "Send requested. Check the status above.")) { app.enviar(destino, monto) } }
        // Último envío: una línea fija (el estado en curso va arriba).
        Ayuda(b.ultimo ?: tr("Sin envíos desde que abriste la app.", "No sends since you opened the app."), maxLines = 1)
    }

    val estResp = sondear { app.estadoRespaldo() }?.getOrNull()
    Plegable(tr("Respaldos y recuperación", "Backups and recovery"), estResp?.linea ?: tr("Respaldo completo", "Full backup"), abierta = estResp?.falta == true) {
        Respaldos(acciones, banner, hayCuenta = true)
    }

    NodoPlegable(b.daemon, esDefecto, b.tip, b.visto, pruebaNodo, acciones)

    if (b.cajas.isNotEmpty()) {
        Plegable(tr("Cajas de obras (2-de-2)", "Job boxes (2-of-2)"), tr("${b.cajas.size} caja(s)", "${b.cajas.size} box(es)")) {
            b.cajas.forEachIndexed { i, c ->
                if (i > 0) Divisor()
                Text(c.obraNombre, fontWeight = FontWeight.SemiBold)
                CajaDatos(c.obraId, c.direccion, c.mirada, acciones)
                CajaRespaldo(acciones)
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
    Plegable(tr("Nodo / Avanzado", "Node / Advanced"), if (esDefecto) tr("Nodo público", "Public node") else tr("Nodo propio", "Own node")) {
        Copiable(if (esDefecto) tr("Nodo (público)", "Node (public)") else tr("Nodo (propio · cambialo en Cuenta)", "Node (own · change it in Account)"), daemon)
        PruebaRpcFila(prueba, false, tr("RPC sin probar · Cuenta → «Probar RPC del nodo»", "RPC not tested · Account → “Test RPC”"))
        DatoFila(tr("Punta del nodo", "Node tip"), tip?.toString() ?: "—", mono = true)
        Ayuda(visto, maxLines = 2)
        Secundario(tr("Actualizar saldo", "Refresh balance")) { acciones.correr { app.actualizarSaldo() } }
        Secundario(tr("Mirar 200 bloques más atrás", "Scan 200 more blocks back")) { acciones.correr(tr("Sumé 200 bloques hacia atrás.", "Added 200 blocks back.")) { app.mirarAtras() } }
        ComoFunciona {
            Ayuda(tr("El saldo sale de mirar la cadena con tu view key en el nodo activo. Lo nuevo se mira solo; lo anterior al primer bloque mirado, solo si lo pedís.", "The balance comes from scanning the chain with your view key on the active node. New blocks are scanned automatically; blocks before the first scanned one only if you ask."))
            Ayuda(tr("Un nodo de tu red local o Tailscale va directo, nunca por Tor ni por el SOCKS de Orbot.", "A node on your LAN or Tailscale is reached directly, never over Tor or Orbot's SOCKS."))
        }
    }
}

