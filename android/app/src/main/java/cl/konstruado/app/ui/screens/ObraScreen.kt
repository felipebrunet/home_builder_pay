package cl.konstruado.app.ui.screens

import cl.konstruado.app.ui.tr
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
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
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Acciones
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Chip
import cl.konstruado.app.ui.Chips
import cl.konstruado.app.ui.ComoFunciona
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.EstadoFila
import cl.konstruado.app.ui.Lead
import cl.konstruado.app.ui.Nav
import cl.konstruado.app.ui.Pantalla
import cl.konstruado.app.ui.Peligro
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Plegable
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Tarjeta
import cl.konstruado.app.ui.TextoBoton
import cl.konstruado.app.ui.Tono
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAbrirArchivo
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.sondear
import uniffi.konstruado_ffi.MiradaVista
import uniffi.konstruado_ffi.ObraVista

/** Tono del chip de una partida según su estado (mismo criterio que el escritorio). */
fun tonoPartida(estado: String): Tono = when (estado) {
    "pagada", "en obra" -> Tono.Ok
    "en fondeo", "en trato" -> Tono.Espera
    else -> Tono.Apagado
}

fun tonoObra(estado: String): Tono = when (estado) {
    "enmarcha", "acordada" -> Tono.Ok
    "publicada", "contra" -> Tono.Espera
    "rechazada" -> Tono.Error
    else -> Tono.Apagado
}

/** Historia de la caja en una fila de alto fijo. */
@Composable
fun MiradaFila(m: MiradaVista) {
    when {
        m.aviso != null -> EstadoFila(Tono.Error, m.aviso!!)
        m.retro != null -> EstadoFila(Tono.Espera, m.retro!!, enCurso = true)
        else -> EstadoFila(Tono.Ok, m.bloques)
    }
}

/** Dirección de la caja 2-de-2, view key e historia (como el escritorio). */
@Composable
fun CajaDatos(obraId: String, direccion: String?, mirada: MiradaVista?, acciones: Acciones) {
    val app = AppHolder.a
    var verVk by remember(obraId) { mutableStateOf(false) }
    if (direccion == null) {
        Ayuda(tr("La caja 2-de-2 todavía no está armada.", "The 2-of-2 box is not set up yet."))
        return
    }
    Copiable(tr("Caja stagenet (2-de-2)", "Stagenet box (2-of-2)"), direccion)
    mirada?.let { MiradaFila(it) }
    TextoBoton(if (verVk) tr("Ocultar view key", "Hide view key") else tr("Mostrar view key de la caja", "Show the box view key")) { verVk = !verVk }
    if (verVk) {
        app.viewKeyCaja(obraId)?.let { Copiable(tr("View key de la caja", "Box view key"), it) }
        Ayuda(tr("Junto con la dirección, esta view key muestra los movimientos de la caja. No alcanza para gastar.", "Together with the address, this view key shows the box's movements. It cannot spend."))
    }
    TextoBoton(tr("Mirar 200 bloques más atrás en la caja", "Scan 200 more blocks back in the box")) {
        acciones.correr(tr("Sumé 200 bloques a la historia de la caja.", "Added 200 blocks to the box history.")) { app.mirarAtrasCaja(obraId) }
    }
}

/** Importar un share suelto de 0.2.7 o antes. Desde 0.2.8 el share va en el respaldo completo. */
@Composable
fun CajaRespaldo(acciones: Acciones) {
    val app = AppHolder.a
    val abrirShare = rememberAbrirArchivo(acciones) { app.restaurarShare(it) }
    Secundario(tr("Importar un share suelto (.share)", "Import a standalone share (.share)")) { abrirShare() }
    ComoFunciona(tr("Dónde está el share", "Where the share is")) {
        Ayuda(tr("Desde 0.2.8 el share de cada caja va en el respaldo completo (Billetera → Respaldos y recuperación).", "Since 0.2.8 each box's share is inside the full backup (Wallet → Backups and recovery)."))
        Ayuda(tr("Esto es para archivos .share de 0.2.7 o antes. Tiene que ser el tuyo y la obra tiene que seguir en este equipo.", "This is for .share files from 0.2.7 or earlier. It must be yours and the job must still be on this device."))
    }
}

/** Una sola línea con lo que pasa ahora en la obra. */
@Composable
private fun EstadoObra(o: ObraVista) {
    when {
        o.armandoCaja -> EstadoFila(Tono.Espera, tr("Armando la caja 2-de-2 · los dos en línea", "Setting up the 2-of-2 box · both online"), enCurso = true)
        o.sincronizando -> EstadoFila(Tono.Espera, tr("Sincronizando el trato con el otro…", "Syncing the deal with the other side…"), enCurso = true)
        o.cierreMio -> EstadoFila(Tono.Espera, tr("Esperando que acepten cortar el trato", "Waiting for them to accept ending the deal"))
        o.cierreDe != null -> EstadoFila(Tono.Error, tr("${o.cierreDe} quiere cortar el trato", "${o.cierreDe} wants to end the deal"))
        o.contra -> EstadoFila(Tono.Espera, tr("Falta confirmar la contra", "The counteroffer needs confirming"))
        o.abandonada -> EstadoFila(Tono.Apagado, tr("Obra abandonada · el trato quedó cortado", "Job abandoned · the deal is over"))
        o.abierta -> EstadoFila(Tono.Ok, tr("En marcha · al día con el otro", "Underway · in sync with the other side"))
        else -> EstadoFila(Tono.Apagado, o.estadoLabel)
    }
}

@Composable
fun ObraScreen(id: String, nav: Nav, banner: Banner) {
    val acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val r = sondear(id) { app.obraVista(id) } ?: run { Pista(tr("Cargando…", "Loading…")); return }
    val o = r.getOrElse {
        Pista(tr("${it.humano()} Si la acabás de publicar, esperá al contratista.", "${it.humano()} If you just posted it, wait for the contractor."))
        return
    }
    var confirmaAbandono by remember(id) { mutableStateOf(false) }
    var confirmaSalidaLocal by remember(id) { mutableStateOf(false) }
    var extraTexto by remember(id) { mutableStateOf("") }
    var extraMonto by remember(id) { mutableStateOf("") }
    val soyParte = o.soyMandante || o.soyContratista

    Tarjeta(tr("Ahora", "Now")) {
        Text(o.nombre, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
        Chips {
            Chip(o.estadoLabel, tonoObra(o.estado))
            if (o.soyMandante) Chip(tr("pagás la obra", "you pay for the job"), Tono.Apagado)
            if (o.soyContratista) Chip(tr("la construís", "you build it"), Tono.Apagado)
        }
        Ayuda(o.resumen)
        EstadoObra(o)
        if (o.contra) {
            o.contraTexto?.let { Lead(it) }
            if (o.soyMandante) {
                Primario(tr("Confirmar contra", "Confirm counteroffer")) { acciones.correr(tr("Contra confirmada.", "Counteroffer confirmed.")) { app.confirmarContra(id) } }
                Peligro(tr("No aceptar esta garantía", "Do not accept this guarantee")) {
                    acciones.correr(tr("La oferta volvió al tablero.", "The offer is back on the board."), alTerminar = { nav.raiz(Pantalla.Tablero) }) { app.rechazarContra(id) }
                }
            }
        }
        if (!o.cierreMio && o.cierreDe != null) {
            Peligro(tr("Aceptar cierre", "Accept closing"), lleno = true) {
                acciones.correr(tr("Trato cerrado.", "Deal closed."), alTerminar = { nav.raiz(Pantalla.Tablero) }) { app.aceptarCierre(id) }
            }
            Secundario(tr("Seguir con la obra", "Keep the job going")) { acciones.correr { app.rechazarCierre(id) } }
        }
    }

    Tarjeta(tr("Partidas", "Stages")) {
        o.partidas.forEach { p ->
            FilaIr(
                "${p.indice + 1u}  ${p.titulo}",
                listOfNotNull(p.porLado, p.saldoCorto).joinToString(" · "),
                {
                    Chips {
                        Chip(p.label, tonoPartida(p.estado), enCurso = p.estado == "en fondeo")
                        if (p.activa) Chip(tr("en curso", "in progress"), Tono.Espera)
                        p.trabaCorta?.let { Chip(it, Tono.Espera, enCurso = true) }
                    }
                },
            ) { nav.ir(Pantalla.Partida(id, p.indice)) }
        }
        Ayuda(tr("Entrá a cada partida para encerrar, fondear, avisar que terminó y tratar el porcentaje.", "Open each stage to lock, fund, report finish and agree the percent."))
    }

    if (o.abierta) {
        val ex = o.extra
        if (ex != null) {
            Tarjeta("Extra") {
                if (ex.mia) {
                    EstadoFila(Tono.Espera, tr("Esperando respuesta: ${ex.texto} (${ex.monto} por lado)", "Waiting for an answer: ${ex.texto} (${ex.monto} per side)"))
                } else {
                    Lead(tr("${ex.por} propone extra: ${ex.texto} (+${ex.monto} por lado)", "${ex.por} proposes an extra: ${ex.texto} (+${ex.monto} per side)"))
                    Primario(tr("Aceptar extra", "Accept extra")) { acciones.correr(tr("Extra agregada.", "Extra added.")) { app.aceptarExtra(id) } }
                    Secundario(tr("No agregar", "Do not add")) { acciones.correr { app.rechazarExtra(id) } }
                }
            }
        } else if (o.puedeExtra) {
            Plegable(tr("Partida extra (opcional)", "Extra stage (optional)"), tr("Proponer trabajo adicional", "Propose additional work")) {
                OutlinedTextField(extraTexto, { extraTexto = it }, label = { Text(tr("P. ej. Techumbre extra", "E.g. Extra roofing")) }, modifier = Modifier.fillMaxWidth())
                OutlinedTextField(extraMonto, { extraMonto = it }, label = { Text(if (o.usd) tr("Monto por lado (USD)", "Amount per side (USD)") else tr("Monto por lado", "Amount per side")) },
                    keyboardOptions = KeyboardOptions(keyboardType = if (o.usd) KeyboardType.Decimal else KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                Secundario(tr("Proponer extra", "Propose extra")) {
                    acciones.correr(tr("Extra propuesta.", "Extra proposed."), alTerminar = { extraTexto = ""; extraMonto = "" }) {
                        app.proponerExtra(id, extraTexto, extraMonto)
                    }
                }
            }
        }
    }

    if (o.abierta && soyParte) {
        // Recordatorio: caja recién armada u obra nueva que el último respaldo no tiene.
        val resp = sondear { app.estadoRespaldo() }?.getOrNull()
        if (resp?.falta == true) {
            Tarjeta {
                EstadoFila(Tono.Espera, resp.linea)
                Ayuda(tr("Billetera → Respaldos y recuperación → Exportar respaldo completo.", "Wallet → Backups and recovery → Export full backup."))
            }
        }
        Tarjeta(tr("Caja y pagos", "Box and payments")) { CajaDatos(o.id, o.cajaDireccion, o.mirada, acciones) }
        Plegable(tr("Importar share suelto", "Import standalone share"), tr("Avanzado · archivos de 0.2.7 o antes", "Advanced · files from 0.2.7 or earlier")) {
            CajaRespaldo(acciones)
        }
    }

    val puedeAbandonar = o.abierta && soyParte && !o.cierreMio && o.cierreDe == null
    if (puedeAbandonar || o.abandonada) {
        Plegable(tr("Avanzado", "Advanced"), if (o.abandonada) tr("Archivar", "Archive") else tr("Abandonar o archivar", "Abandon or archive")) {
            if (puedeAbandonar) {
                if (confirmaAbandono) {
                    EstadoFila(
                        Tono.Error,
                        if (o.hayRiesgo) tr("Hay partidas encerradas: el otro tiene que aceptar", "Stages are locked: the other side has to accept")
                        else tr("Se corta el trato y no se puede deshacer", "The deal ends and cannot be undone"),
                    )
                    Peligro(if (o.hayRiesgo) tr("Proponer cierre", "Propose closing") else tr("Sí, abandonar", "Yes, abandon"), lleno = true) {
                        acciones.correr(alTerminar = {
                            confirmaAbandono = false
                            if (!o.hayRiesgo) nav.raiz(Pantalla.Tablero)
                        }) { app.abandonar(id) }
                    }
                    TextoBoton(tr("No", "No")) { confirmaAbandono = false }
                } else {
                    Peligro(tr("Abandonar esta obra", "Abandon this job")) { confirmaAbandono = true }
                }
            }
            if (confirmaSalidaLocal) {
                Ayuda(
                    if (o.abandonada) tr("¿Archivar? Sale de Mis obras en este equipo. El contexto queda en disco.", "Archive? It leaves My jobs on this device. The context stays on disk.")
                    else tr("¿Archivar en este equipo? Sale del tablero y de Mis obras. No mueve fondos; el share queda en disco.", "Archive on this device? It leaves the board and My jobs. No funds move; the share stays on disk.")
                )
                Primario(if (o.abandonada) tr("Sí, archivar", "Yes, archive") else tr("Sí, archivar aquí", "Yes, archive here")) {
                    acciones.correr(
                        if (o.abandonada) tr("Archivé la obra.", "Job archived.") else tr("Archivé la obra en este equipo. Fondos intactos.", "Job archived on this device. Funds untouched."),
                        alTerminar = { confirmaSalidaLocal = false; nav.raiz(Pantalla.Tablero) },
                    ) { app.archivarObraLocal(id) }
                }
                TextoBoton(tr("No", "No")) { confirmaSalidaLocal = false }
            } else {
                Secundario(tr("Archivar esta obra", "Archive this job")) { confirmaSalidaLocal = true }
            }
        }
    }
}

