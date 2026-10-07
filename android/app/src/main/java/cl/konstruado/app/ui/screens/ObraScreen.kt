package cl.konstruado.app.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Acciones
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.Lead
import cl.konstruado.app.ui.Nav
import cl.konstruado.app.ui.Pantalla
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Seccion
import cl.konstruado.app.ui.Titulo
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAbrirArchivo
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.rememberGuardarArchivo
import cl.konstruado.app.ui.sondear
import uniffi.konstruado_ffi.MiradaVista

/** Dirección de la caja 2-de-2, view key y respaldo del share FROST (como el escritorio). */
@Composable
fun CajaPanel(obraId: String, direccion: String?, mirada: MiradaVista?, acciones: Acciones, banner: Banner) {
    val app = AppHolder.a
    var verVk by remember(obraId) { mutableStateOf(false) }
    val guardarShare = rememberGuardarArchivo(
        acciones, banner, { app.exportarShare(obraId) },
        "Guardé el share. Esa copia puede gastar, junto con la del otro.",
    )
    val abrirShare = rememberAbrirArchivo(acciones) { app.restaurarShare(it) }
    if (direccion != null) {
        Copiable("Caja stagenet (2-de-2)", direccion)
        Secundario(if (verVk) "Ocultar view key" else "Mostrar view key de la caja") { verVk = !verVk }
        if (verVk) {
            app.viewKeyCaja(obraId)?.let { Copiable("View key de la caja", it) }
            Pista("Junto con la dirección, esta view key muestra los movimientos de la caja. No alcanza para gastar.")
        }
        Secundario("Guardar el share de la caja") { guardarShare("konstruado-$obraId.share") }
        Pista("Esta copia puede gastar, junto con el share del otro. Guardala aparte y no la pegues en un chat.")
        mirada?.let { m ->
            Pista(m.bloques)
            m.retro?.let { Pista(it) }
            m.aviso?.let { ErrorTexto(it) }
        }
        Secundario("Mirar 200 bloques más atrás en la caja") {
            acciones.correr("Sumé 200 bloques a la historia de la caja.") { app.mirarAtrasCaja(obraId) }
        }
    }
    Pista("Si perdiste el share de esta obra, recuperalo desde el archivo que guardaste. Tiene que ser el tuyo y la obra tiene que seguir en este equipo.")
    Secundario("Recuperar un share") { abrirShare() }
}

@Composable
fun ObraScreen(id: String, nav: Nav, banner: Banner) {
    val acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val r = sondear(id) { app.obraVista(id) } ?: run { Pista("Cargando…"); return }
    val o = r.getOrElse {
        Pista("${it.humano()} Si la acabás de publicar, esperá al contratista.")
        return
    }
    var confirmaAbandono by remember(id) { mutableStateOf(false) }
    var confirmaSalidaLocal by remember(id) { mutableStateOf(false) }
    var extraTexto by remember(id) { mutableStateOf("") }
    var extraMonto by remember(id) { mutableStateOf("") }
    Row { Titulo(o.nombre) }
    AssistChip(onClick = {}, label = { Text(o.estadoLabel) })
    Lead(o.resumen)
    if (o.armandoCaja) Pista("Armando la caja 2-de-2. Los dos tienen que seguir en línea.")
    if (o.sincronizando) Pista("Sincronizando el trato… las acciones esperan a bajar el estado del otro.")
    if (o.abierta && (o.soyMandante || o.soyContratista)) {
        Seccion("Caja")
        CajaPanel(o.id, o.cajaDireccion, o.mirada, acciones, banner)
    }
    if (o.contra) {
        Seccion("Contra")
        o.contraTexto?.let { Lead(it) }
        if (o.soyMandante) {
            Primario("Confirmar contra") { acciones.correr("Contra confirmada.") { app.confirmarContra(id) } }
            Secundario("No aceptar esta garantía") {
                acciones.correr("La oferta volvió al tablero.", alTerminar = { nav.raiz(Pantalla.Tablero) }) { app.rechazarContra(id) }
            }
        }
    }
    if (o.abandonada) Pista("Esta obra se abandonó. El trato quedó cortado.")
    when {
        o.cierreMio -> Pista("Esperando que acepten cortar el trato.")
        o.cierreDe != null -> {
            Lead("${o.cierreDe} quiere cortar el trato.")
            Primario("Aceptar cierre") {
                acciones.correr("Trato cerrado.", alTerminar = { nav.raiz(Pantalla.Tablero) }) { app.aceptarCierre(id) }
            }
            Secundario("Seguir con la obra") { acciones.correr { app.rechazarCierre(id) } }
        }
        o.abierta && (o.soyMandante || o.soyContratista) -> {
            if (confirmaAbandono) {
                Pista(
                    if (o.hayRiesgo) "Hay partidas encerradas. El otro tiene que aceptar el cierre."
                    else "¿Abandonar? Se corta el trato y no se puede deshacer."
                )
                Primario(if (o.hayRiesgo) "Proponer cierre" else "Sí, abandonar") {
                    acciones.correr(alTerminar = {
                        confirmaAbandono = false
                        if (!o.hayRiesgo) nav.raiz(Pantalla.Tablero)
                    }) { app.abandonar(id) }
                }
                Secundario("No") { confirmaAbandono = false }
            } else {
                Secundario("Abandonar esta obra") { confirmaAbandono = true }
            }
            if (confirmaSalidaLocal) {
                Pista("¿Archivar en este equipo? Sale del tablero y de Mis obras. No mueve fondos; el share queda en disco.")
                Primario("Sí, archivar aquí") {
                    acciones.correr(
                        "Archivé la obra en este equipo. Fondos intactos.",
                        alTerminar = { confirmaSalidaLocal = false; nav.raiz(Pantalla.Tablero) },
                    ) { app.archivarObraLocal(id) }
                }
                Secundario("No") { confirmaSalidaLocal = false }
            } else {
                Secundario("Archivar esta obra") { confirmaSalidaLocal = true }
            }
        }
        o.abandonada -> {
            if (confirmaSalidaLocal) {
                Pista("¿Archivar? Sale de Mis obras en este equipo. El contexto queda en disco.")
                Primario("Sí, archivar") {
                    acciones.correr(
                        "Archivé la obra.",
                        alTerminar = { confirmaSalidaLocal = false; nav.raiz(Pantalla.Tablero) },
                    ) { app.archivarObraLocal(id) }
                }
                Secundario("No") { confirmaSalidaLocal = false }
            } else {
                Secundario("Archivar esta obra") { confirmaSalidaLocal = true }
            }
        }
    }
    Seccion("Partidas")
    Pista("Entrá a cada partida para encerrar, fondear, avisar que terminó y tratar el porcentaje.")
    o.partidas.forEach { p ->
        Card(
            colors = if (p.activa) CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer)
            else CardDefaults.cardColors(),
            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp).clickable { nav.ir(Pantalla.Partida(id, p.indice)) },
        ) {
            Column(Modifier.padding(12.dp)) {
                Text("${p.indice + 1u}  ${p.titulo}", fontWeight = FontWeight.SemiBold)
                Text("${p.porLado} · ${p.label}", style = MaterialTheme.typography.bodySmall)
                p.saldoCorto?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
            }
        }
    }
    if (o.abierta) {
        val ex = o.extra
        if (ex != null) {
            Seccion("Extra")
            if (ex.mia) {
                Pista("Esperando extra: ${ex.texto} (${ex.monto} por lado)")
            } else {
                Lead("${ex.por} propone extra: ${ex.texto} (+${ex.monto} por lado)")
                Primario("Aceptar extra") { acciones.correr("Extra agregada.") { app.aceptarExtra(id) } }
                Secundario("No agregar") { acciones.correr { app.rechazarExtra(id) } }
            }
        } else if (o.puedeExtra) {
            Seccion("Partida extra (opcional)")
            OutlinedTextField(extraTexto, { extraTexto = it }, label = { Text("P. ej. Techumbre extra") }, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(extraMonto, { extraMonto = it }, label = { Text("Monto por lado") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
            Secundario("Proponer extra") {
                acciones.correr("Extra propuesta.", alTerminar = { extraTexto = ""; extraMonto = "" }) {
                    app.proponerExtra(id, extraTexto, extraMonto)
                }
            }
        }
    }
}
