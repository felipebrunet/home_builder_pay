package cl.konstruado.app.ui.screens

import cl.konstruado.app.ui.tr
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.orbotInstalado
import cl.konstruado.app.ui.Acciones
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Chip
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.EstadoFila
import cl.konstruado.app.ui.EstadoTarjeta
import cl.konstruado.app.ui.Nav
import cl.konstruado.app.ui.Pantalla
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Tarjeta
import cl.konstruado.app.ui.TextoBoton
import cl.konstruado.app.ui.Tono
import cl.konstruado.app.ui.mensajeYaVisible
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.sondear
import cl.konstruado.app.ui.tonoDe
import uniffi.konstruado_ffi.RedVista
import uniffi.konstruado_ffi.SalaEstado

/** Estados de la sala que todavía se están resolviendo (spinner en vez de punto). */
fun salaEnCurso(s: SalaEstado) = s.tipo in setOf("probando", "socks_ok", "tcp")

/**
 * Estado de la red: qué pasa con Orbot y la sala, en alto fijo
 * (título de una línea + detalle de dos). Debajo, una línea con los números.
 */
@Composable
fun RedTarjeta(red: RedVista, sala: SalaEstado = red.sala) {
    EstadoTarjeta(tonoDe(sala.tono), sala.titulo, sala.detalle, salaEnCurso(sala))
    Ayuda(
        tr("Red ${red.red} · sesiones ${red.sesionesVivas} · pares ${red.pares}", "Network ${red.red} · sessions ${red.sesionesVivas} · peers ${red.pares}") +
            (red.socks?.let { " · SOCKS $it" } ?: ""),
        maxLines = 1,
    )
}

/** Fila tocable dentro de una tarjeta: título, subtítulo y un chip opcional. */
@Composable
fun FilaIr(titulo: String, sub: String, chip: (@Composable () -> Unit)? = null, onClick: (() -> Unit)?) {
    Surface(
        color = MaterialTheme.colorScheme.surface,
        shape = RoundedCornerShape(10.dp),
        modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp)
            .let { if (onClick != null) it.clickable(onClick = onClick) else it },
    ) {
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(titulo, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(
                    sub, style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis,
                )
                if (chip != null) Row(Modifier.padding(top = 4.dp)) { chip() }
            }
            if (onClick != null) {
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

@Composable
fun TableroScreen(nav: Nav, banner: Banner) {
    val acciones: Acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val ctx = LocalContext.current
    val r = sondear { Triple(app.tablero(), app.perfil(), app.estadoSala(orbotInstalado(ctx))) }
        ?: run { Pista(tr("Cargando…", "Loading…")); return }
    val (t, perfil, sala) = r.getOrElse { ErrorTexto(it.message ?: "error"); return }
    Tarjeta(tr("Red", "Network")) {
        RedTarjeta(t.red, sala)
        // La pista solo suma si no repite el estado de la sala.
        if (!mensajeYaVisible(t.pista, listOf(sala.titulo))) Ayuda(t.pista)
        TextoBoton(tr("Buscar ofertas", "Look for offers")) { acciones.correr(tr("Pedí lo último a la red.", "Asked the network for the latest.")) { app.buscar() } }
    }
    // Recordatorio del respaldo completo (obra nueva, caja armada o nunca exportado).
    val resp = sondear { app.estadoRespaldo() }?.getOrNull()
    if (resp?.falta == true && (resp.ultimo != null || t.obras.isNotEmpty())) {
        Tarjeta {
            EstadoFila(Tono.Espera, resp.linea)
            TextoBoton(tr("Ir a Respaldos", "Go to Backups")) { nav.raiz(Pantalla.Billetera) }
        }
    }
    if (t.avisos.isNotEmpty()) {
        Tarjeta(tr("Te toca", "Your turn"), resaltada = true) {
            t.avisos.forEach { a ->
                FilaIr(a.texto, tr("Tocá para abrir", "Tap to open"), null) {
                    val p = a.partida
                    if (p != null) nav.ir(Pantalla.Partida(a.obraId, p)) else nav.ir(Pantalla.Obra(a.obraId))
                }
            }
        }
    }
    if (t.obras.isNotEmpty()) {
        Tarjeta(tr("Mis obras", "My jobs")) {
            t.obras.forEach { o ->
                FilaIr(o.nombre, o.conQuien, {
                    Chip(o.estadoLabel, if (o.enCurso) Tono.Ok else Tono.Apagado)
                }) { nav.ir(Pantalla.Obra(o.id)) }
            }
        }
    }
    if (perfil.rol == "mandante") {
        Tarjeta(tr("Mis ofertas publicadas", "My posted offers")) {
            if (t.misOfertas.isEmpty()) Ayuda(tr("Todavía no publicaste.", "You have not posted yet."))
            t.misOfertas.forEach { o ->
                FilaIr(o.nombre, o.resumen, { Chip(tr("esperando contratista", "waiting for a contractor"), Tono.Espera) }, null)
                // Solo mientras nadie la tomó (las tomadas ya son obras).
                TextoBoton(tr("Quitar oferta", "Remove offer")) {
                    acciones.correr(tr("Quité la oferta. Tampoco va a aparecer en el tablero del contratista.", "Offer removed. It will not show on the contractor's board either.")) { app.quitarMiOferta(o.id) }
                }
            }
            Primario(tr("Publicar obra", "Post a job")) { nav.ir(Pantalla.Publicar) }
        }
    }
    Tarjeta(tr("Ofertas en la red", "Offers on the network")) {
        if (t.ofertas.isEmpty()) Ayuda(tr("No hay ofertas de otros todavía.", "No offers from others yet."))
        t.ofertas.forEach { o -> FilaIr(o.nombre, o.resumen, null) { nav.ir(Pantalla.Oferta(o.id)) } }
    }
}

