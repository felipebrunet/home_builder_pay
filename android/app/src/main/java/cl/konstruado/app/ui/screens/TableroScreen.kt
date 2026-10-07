package cl.konstruado.app.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Acciones
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.Nav
import cl.konstruado.app.ui.Pantalla
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Seccion
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.sondear
import uniffi.konstruado_ffi.RedVista

@Composable
fun RedTarjeta(red: RedVista) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = if (red.conectado) MaterialTheme.colorScheme.primaryContainer
            else MaterialTheme.colorScheme.surfaceVariant
        ),
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
    ) {
        Column(Modifier.padding(12.dp)) {
            Text("Red ${red.red}", style = MaterialTheme.typography.labelMedium)
            Text(red.linea, fontWeight = FontWeight.SemiBold)
            Text(
                "Sesiones vivas ${red.sesionesVivas} · pares ${red.pares}" +
                    (red.socks?.let { " · SOCKS $it" } ?: ""),
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

@Composable
private fun Fila(titulo: String, sub: String, onClick: () -> Unit) {
    Card(Modifier.fillMaxWidth().padding(vertical = 4.dp).clickable(onClick = onClick)) {
        Column(Modifier.padding(12.dp)) {
            Text(titulo, fontWeight = FontWeight.SemiBold)
            Text(sub, style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
fun TableroScreen(nav: Nav, banner: Banner) {
    val acciones: Acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val r = sondear { app.tablero() to app.perfil() } ?: run { Pista("Cargando…"); return }
    val (t, perfil) = r.getOrElse { ErrorTexto(it.message ?: "error"); return }
    RedTarjeta(t.red)
    Pista(t.pista)
    Secundario("Buscar ofertas") { acciones.correr("Pedí lo último a la red.") { app.buscar() } }
    if (t.avisos.isNotEmpty()) {
        Seccion("Te toca")
        t.avisos.forEach { a ->
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer),
                modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp).clickable {
                    val p = a.partida
                    if (p != null) nav.ir(Pantalla.Partida(a.obraId, p)) else nav.ir(Pantalla.Obra(a.obraId))
                },
            ) { Text(a.texto, Modifier.padding(12.dp)) }
        }
    }
    if (t.obras.isNotEmpty()) {
        Seccion("Mis obras")
        t.obras.forEach { o -> Fila(o.nombre, "${o.estadoLabel} · ${o.conQuien}") { nav.ir(Pantalla.Obra(o.id)) } }
    }
    if (perfil.rol == "mandante") {
        Seccion("Mis ofertas publicadas")
        if (t.misOfertas.isEmpty()) Pista("Todavía no publicaste.")
        t.misOfertas.forEach { o -> Fila(o.nombre, o.resumen) {} }
        Primario("Publicar obra") { nav.ir(Pantalla.Publicar) }
    }
    Seccion("Ofertas en la red")
    if (t.ofertas.isEmpty()) Pista("No hay ofertas de otros todavía.")
    t.ofertas.forEach { o -> Fila(o.nombre, o.resumen) { nav.ir(Pantalla.Oferta(o.id)) } }
}
