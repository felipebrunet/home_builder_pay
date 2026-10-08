package cl.konstruado.app.ui.screens

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
        Ayuda("La caja 2-de-2 todavía no está armada.")
        return
    }
    Copiable("Caja stagenet (2-de-2)", direccion)
    mirada?.let { MiradaFila(it) }
    TextoBoton(if (verVk) "Ocultar view key" else "Mostrar view key de la caja") { verVk = !verVk }
    if (verVk) {
        app.viewKeyCaja(obraId)?.let { Copiable("View key de la caja", it) }
        Ayuda("Junto con la dirección, esta view key muestra los movimientos de la caja. No alcanza para gastar.")
    }
    TextoBoton("Mirar 200 bloques más atrás en la caja") {
        acciones.correr("Sumé 200 bloques a la historia de la caja.") { app.mirarAtrasCaja(obraId) }
    }
}

/** Importar un share suelto de 0.2.7 o antes. Desde 0.2.8 el share va en el respaldo completo. */
@Composable
fun CajaRespaldo(acciones: Acciones) {
    val app = AppHolder.a
    val abrirShare = rememberAbrirArchivo(acciones) { app.restaurarShare(it) }
    Secundario("Importar un share suelto (.share)") { abrirShare() }
    ComoFunciona("Dónde está el share") {
        Ayuda("Desde 0.2.8 el share de cada caja va en el respaldo completo (Billetera → Respaldos y recuperación).")
        Ayuda("Esto es para archivos .share de 0.2.7 o antes. Tiene que ser el tuyo y la obra tiene que seguir en este equipo.")
    }
}

/** Una sola línea con lo que pasa ahora en la obra. */
@Composable
private fun EstadoObra(o: ObraVista) {
    when {
        o.armandoCaja -> EstadoFila(Tono.Espera, "Armando la caja 2-de-2 · los dos en línea", enCurso = true)
        o.sincronizando -> EstadoFila(Tono.Espera, "Sincronizando el trato con el otro…", enCurso = true)
        o.cierreMio -> EstadoFila(Tono.Espera, "Esperando que acepten cortar el trato")
        o.cierreDe != null -> EstadoFila(Tono.Error, "${o.cierreDe} quiere cortar el trato")
        o.contra -> EstadoFila(Tono.Espera, "Falta confirmar la contra")
        o.abandonada -> EstadoFila(Tono.Apagado, "Obra abandonada · el trato quedó cortado")
        o.abierta -> EstadoFila(Tono.Ok, "En marcha · al día con el otro")
        else -> EstadoFila(Tono.Apagado, o.estadoLabel)
    }
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
    val soyParte = o.soyMandante || o.soyContratista

    Tarjeta("Ahora") {
        Text(o.nombre, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
        Chips {
            Chip(o.estadoLabel, tonoObra(o.estado))
            if (o.soyMandante) Chip("pagás la obra", Tono.Apagado)
            if (o.soyContratista) Chip("la construís", Tono.Apagado)
        }
        Ayuda(o.resumen)
        EstadoObra(o)
        if (o.contra) {
            o.contraTexto?.let { Lead(it) }
            if (o.soyMandante) {
                Primario("Confirmar contra") { acciones.correr("Contra confirmada.") { app.confirmarContra(id) } }
                Peligro("No aceptar esta garantía") {
                    acciones.correr("La oferta volvió al tablero.", alTerminar = { nav.raiz(Pantalla.Tablero) }) { app.rechazarContra(id) }
                }
            }
        }
        if (!o.cierreMio && o.cierreDe != null) {
            Peligro("Aceptar cierre", lleno = true) {
                acciones.correr("Trato cerrado.", alTerminar = { nav.raiz(Pantalla.Tablero) }) { app.aceptarCierre(id) }
            }
            Secundario("Seguir con la obra") { acciones.correr { app.rechazarCierre(id) } }
        }
    }

    Tarjeta("Partidas") {
        o.partidas.forEach { p ->
            FilaIr(
                "${p.indice + 1u}  ${p.titulo}",
                listOfNotNull(p.porLado, p.saldoCorto).joinToString(" · "),
                {
                    Chips {
                        Chip(p.label, tonoPartida(p.estado), enCurso = p.estado == "en fondeo")
                        if (p.activa) Chip("en curso", Tono.Espera)
                        p.trabaCorta?.let { Chip(it, Tono.Espera, enCurso = true) }
                    }
                },
            ) { nav.ir(Pantalla.Partida(id, p.indice)) }
        }
        Ayuda("Entrá a cada partida para encerrar, fondear, avisar que terminó y tratar el porcentaje.")
    }

    if (o.abierta) {
        val ex = o.extra
        if (ex != null) {
            Tarjeta("Extra") {
                if (ex.mia) {
                    EstadoFila(Tono.Espera, "Esperando respuesta: ${ex.texto} (${ex.monto} por lado)")
                } else {
                    Lead("${ex.por} propone extra: ${ex.texto} (+${ex.monto} por lado)")
                    Primario("Aceptar extra") { acciones.correr("Extra agregada.") { app.aceptarExtra(id) } }
                    Secundario("No agregar") { acciones.correr { app.rechazarExtra(id) } }
                }
            }
        } else if (o.puedeExtra) {
            Plegable("Partida extra (opcional)", "Proponer trabajo adicional") {
                OutlinedTextField(extraTexto, { extraTexto = it }, label = { Text("P. ej. Techumbre extra") }, modifier = Modifier.fillMaxWidth())
                OutlinedTextField(extraMonto, { extraMonto = it }, label = { Text(if (o.usd) "Monto por lado (USD)" else "Monto por lado") },
                    keyboardOptions = KeyboardOptions(keyboardType = if (o.usd) KeyboardType.Decimal else KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                Secundario("Proponer extra") {
                    acciones.correr("Extra propuesta.", alTerminar = { extraTexto = ""; extraMonto = "" }) {
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
                Ayuda("Billetera → Respaldos y recuperación → Exportar respaldo completo.")
            }
        }
        Tarjeta("Caja y pagos") { CajaDatos(o.id, o.cajaDireccion, o.mirada, acciones) }
        Plegable("Importar share suelto", "Avanzado · archivos de 0.2.7 o antes") {
            CajaRespaldo(acciones)
        }
    }

    val puedeAbandonar = o.abierta && soyParte && !o.cierreMio && o.cierreDe == null
    if (puedeAbandonar || o.abandonada) {
        Plegable("Avanzado", if (o.abandonada) "Archivar" else "Abandonar o archivar") {
            if (puedeAbandonar) {
                if (confirmaAbandono) {
                    EstadoFila(
                        Tono.Error,
                        if (o.hayRiesgo) "Hay partidas encerradas: el otro tiene que aceptar"
                        else "Se corta el trato y no se puede deshacer",
                    )
                    Peligro(if (o.hayRiesgo) "Proponer cierre" else "Sí, abandonar", lleno = true) {
                        acciones.correr(alTerminar = {
                            confirmaAbandono = false
                            if (!o.hayRiesgo) nav.raiz(Pantalla.Tablero)
                        }) { app.abandonar(id) }
                    }
                    TextoBoton("No") { confirmaAbandono = false }
                } else {
                    Peligro("Abandonar esta obra") { confirmaAbandono = true }
                }
            }
            if (confirmaSalidaLocal) {
                Ayuda(
                    if (o.abandonada) "¿Archivar? Sale de Mis obras en este equipo. El contexto queda en disco."
                    else "¿Archivar en este equipo? Sale del tablero y de Mis obras. No mueve fondos; el share queda en disco."
                )
                Primario(if (o.abandonada) "Sí, archivar" else "Sí, archivar aquí") {
                    acciones.correr(
                        if (o.abandonada) "Archivé la obra." else "Archivé la obra en este equipo. Fondos intactos.",
                        alTerminar = { confirmaSalidaLocal = false; nav.raiz(Pantalla.Tablero) },
                    ) { app.archivarObraLocal(id) }
                }
                TextoBoton("No") { confirmaSalidaLocal = false }
            } else {
                Secundario("Archivar esta obra") { confirmaSalidaLocal = true }
            }
        }
    }
}

