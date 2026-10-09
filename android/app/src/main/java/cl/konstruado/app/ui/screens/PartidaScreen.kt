package cl.konstruado.app.ui.screens

import cl.konstruado.app.ui.tr
import cl.konstruado.app.ui.PanelPrecio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
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
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Chip
import cl.konstruado.app.ui.Chips
import cl.konstruado.app.ui.ComoFunciona
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.Divisor
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.EstadoFila
import cl.konstruado.app.ui.EstadoTarjeta
import cl.konstruado.app.ui.Lead
import cl.konstruado.app.ui.Peligro
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Plegable
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Tarjeta
import cl.konstruado.app.ui.TextoBoton
import cl.konstruado.app.ui.Tono
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.sondear
import uniffi.konstruado_ffi.PartidaVista

private val SINCRONIZANDO: String get() = tr("Sincronizando el trato… las acciones esperan a bajar el estado del otro.", "Syncing the deal… actions wait for the other side's state.")

/** Estado de la partida: tono, título de una línea y detalle (2 líneas reservadas). */
private data class EstadoPartida(val tono: Tono, val titulo: String, val detalle: String, val enCurso: Boolean)

private fun estadoPartida(p: PartidaVista): EstadoPartida {
    // Una sola línea de estado de caja/fondeo (sin repetir "Encerrando"); misma regla que el escritorio.
    val estadoCaja = p.saldoEstado ?: p.linea?.takeIf { !p.lineaFreno }
    if (p.lineaFreno) return EstadoPartida(Tono.Error, tr("Frenado", "Stopped"), p.linea ?: "", false)
    val enCurso = p.enCurso != null || p.pagoEnCurso || p.sincronizando
    val titulo = p.enCurso ?: estadoCaja ?: p.label
    val detalle = listOfNotNull(
        p.saldoDetalle,
        p.linea?.takeIf { it != titulo && it != estadoCaja },
        if (p.sincronizando) SINCRONIZANDO else null,
        estadoCaja?.takeIf { it != titulo },
    ).firstOrNull() ?: p.lead
    val tono = when {
        enCurso -> Tono.Espera
        else -> tonoPartida(p.estado)
    }
    return EstadoPartida(tono, titulo, detalle, enCurso)
}

@Composable
fun PartidaScreen(obraId: String, indice: UInt, banner: Banner) {
    val acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val r = sondear(obraId, indice) { app.partidaVista(obraId, indice) } ?: run { Pista(tr("Cargando…", "Loading…")); return }
    val p = r.getOrElse { ErrorTexto(it.humano()); return }
    var pct by remember(obraId, indice) { mutableStateOf("100") }
    var nota by remember(obraId, indice) { mutableStateOf("") }
    var detalle by remember(obraId, indice) { mutableStateOf(p.detalle) }
    var confirmaEncerrar by remember(obraId, indice) { mutableStateOf(false) }
    val yaVisible = {
        listOfNotNull(
            p.linea, p.pista, p.saldoEstado, p.saldoDetalle,
            if (p.sincronizando) tr("Sincronizando el trato", "Syncing the deal") else null,
            if (p.sincronizando) tr("El otro no está en línea", "The other person is not online") else null,
        )
    }
    val e = estadoPartida(p)

    Tarjeta(tr("Ahora", "Now")) {
        Ayuda(p.obraNombre, maxLines = 1)
        Text("${indice + 1u}  ${p.titulo}", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
        Chips {
            // Lo que está en curso va en la tarjeta de estado de abajo (sin repetirlo acá).
            Chip(p.label, tonoPartida(p.estado))
            if (p.lineaFreno) Chip(tr("frenado", "stopped"), Tono.Error)
            p.trabaCorta?.takeIf { p.enCurso == null && !p.lineaFreno }?.let { Chip(it, Tono.Espera) }
            if (p.miTurno && !p.lineaFreno && !p.pagoEnCurso && p.enCurso == null && p.trabaCorta == null) Chip(tr("te toca", "your turn"), Tono.Ok)
            if (!p.miTurno && !p.pagoEnCurso) p.esperaA?.let { Chip(tr("esperando a $it", "waiting for $it"), Tono.Espera) }
        }
        if (e.detalle != p.lead) Ayuda(p.lead)
        // Obras en USD: XMR fijo (o aproximado antes de encerrar).
        p.xmrPartida?.let { Ayuda(it) }
        EstadoTarjeta(e.tono, e.titulo, e.detalle, e.enCurso, maxLineas = if (p.lineaFreno) 8 else 3)
        p.pista?.let { Ayuda(it) }

        // Solo las acciones válidas ahora (flags de `acciones_partida`, compartidas con el escritorio).
        if (p.puedeProponerEncerrar) {
            p.estadoPrecio?.let { _ ->
                Ayuda(tr("Al proponer, el XMR de esta partida queda fijo con el precio de ahora. El otro lo ve antes de confirmar.", "When you propose, this stage's XMR is fixed at the current price. The other side sees it before confirming."))
                PanelPrecio(banner)
            }
            if (confirmaEncerrar) {
                Primario(tr("Proponer encerrar", "Propose locking")) {
                    acciones.correr(tr("Propuesta enviada. Falta que el otro confirme y fondee.", "Proposal sent. The other side still has to confirm and fund."), alTerminar = { confirmaEncerrar = false }, yaEnPantalla = yaVisible) {
                        app.proponerEncerrar(obraId, indice)
                    }
                }
                TextoBoton(tr("No", "No")) { confirmaEncerrar = false }
            } else {
                Primario(tr("Encerrar esta partida", "Lock this stage")) { confirmaEncerrar = true }
            }
        }
        if (p.puedeConfirmarFondear) {
            p.precioPropuesto?.let { Lead(it) }
            p.avisoPrecio?.let { EstadoTarjeta(Tono.Espera, tr("Precio distinto", "Price moved"), it, enCurso = false, maxLineas = 4) }
            Primario(tr("Confirmar y fondear", "Confirm and fund")) {
                acciones.correr(tr("Armando el fondeo con las dos billeteras…", "Building the funding with both wallets…"), yaEnPantalla = yaVisible) { app.confirmarYFondear(obraId, indice) }
            }
        }
        if (p.puedeEmpezarFondeoDeNuevo || p.puedeReintentarFondeo) {
            Primario(tr("Empezar el fondeo de nuevo", "Start funding again")) {
                acciones.correr(tr("Reinicié el fondeo. Se arman anillos frescos con el otro…", "Funding restarted. Fresh rings are being built with the other side…"), yaEnPantalla = yaVisible) {
                    app.empezarFondeoDeNuevo(obraId, indice)
                }
            }
            ComoFunciona(tr("¿Cuándo sirve?", "When does this help?")) {
                Ayuda(tr("Si el nodo rechazó la tx (decoys viejos). La obra y el encierre siguen; solo se arma de nuevo el fondeo.", "If the node rejected the tx (stale decoys). The job and the lock stay; only the funding is rebuilt."))
            }
        }
        if (p.puedeCancelarPropuesta) {
            Secundario(tr("Cancelar propuesta", "Cancel proposal")) { acciones.correr(yaEnPantalla = yaVisible) { app.cancelarEncerrar(obraId, indice) } }
        } else if (p.puedeNoEncerrar) {
            Secundario(tr("No encerrar", "Do not lock")) { acciones.correr(yaEnPantalla = yaVisible) { app.cancelarEncerrar(obraId, indice) } }
        }
        // Fondeo sin 10 confirmaciones: el botón se ve, deshabilitado, con la cuenta regresiva.
        p.traba?.let { EstadoTarjeta(Tono.Espera, p.trabaCorta ?: tr("Fondos trabados", "Funds locked"), it, enCurso = true, maxLineas = 3) }
        if (p.terminoTrabado) {
            Primario(tr("Avisar que terminé", "Report finished"), enabled = false) {}
        }
        if (p.puedeAvisarTermino) {
            Divisor()
            Text(tr("Avisar que terminé", "Report finished"), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
            OutlinedTextField(pct, { pct = it }, label = { Text(tr("Porcentaje a cobrar", "Percent to collect")) },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
            OutlinedTextField(nota, { if (it.length <= p.maxNota.toInt()) nota = it },
                label = { Text(tr("Nota (${nota.length}/${p.maxNota})", "Note (${nota.length}/${p.maxNota})")) }, placeholder = { Text(tr("Terminé las fundaciones", "Foundations done")) },
                modifier = Modifier.fillMaxWidth())
            Primario(tr("Avisar que terminé", "Report finished")) {
                acciones.correr(tr("Aviso enviado.", "Report sent."), alTerminar = { nota = "" }, yaEnPantalla = yaVisible) { app.avisarTermino(obraId, indice, pct, nota) }
            }
        }
        if (p.enTrato) {
            p.propuestoTexto?.let { Lead(it) }
            // Con el pago ya andando no se ofrece aceptar ni contraofertar otra vez.
            if (p.pagoTrabado) {
                Primario(tr("Aceptar ${p.propuesto ?: 0u}% y pagar", "Accept ${p.propuesto ?: 0u}% and pay"), enabled = false) {}
            }
            if (p.puedeAceptarPago) {
                Primario(tr("Aceptar ${p.propuesto ?: 0u}% y pagar", "Accept ${p.propuesto ?: 0u}% and pay")) {
                    acciones.correr(tr("Firmando el pago 2-de-2 con el otro…", "Signing the 2-of-2 payment with the other side…"), yaEnPantalla = yaVisible) { app.aceptarYPagar(obraId, indice) }
                }
            }
            if (p.puedeContraofertar) {
                OutlinedTextField(pct, { pct = it }, label = { Text(tr("Otro porcentaje", "Other percent")) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                OutlinedTextField(nota, { if (it.length <= p.maxNota.toInt()) nota = it },
                    label = { Text(tr("Nota (${nota.length}/${p.maxNota})", "Note (${nota.length}/${p.maxNota})")) }, placeholder = { Text(tr("Falta la entrada de auto", "Driveway still missing")) },
                    modifier = Modifier.fillMaxWidth())
                Secundario(tr("Proponer este porcentaje", "Propose this percent")) {
                    acciones.correr(tr("Contra enviada.", "Counteroffer sent."), alTerminar = { nota = "" }, yaEnPantalla = yaVisible) { app.contraPago(obraId, indice, pct, nota) }
                }
            }
        }
    }

    val hayCaja = listOf(p.candado, p.xmrPorLado, p.cajaDireccion, p.fondeoTxid, p.pagoTxid, p.recibo, p.cerradoTexto, p.encerro)
        .any { it != null }
    if (hayCaja) {
        Tarjeta(tr("Caja y pagos", "Box and payments")) {
            p.xmrPorLado?.let { Ayuda(it) }
            p.candado?.let { Ayuda(it) }
            p.encerro?.let { Ayuda(it) }
            p.cajaDireccion?.let { Copiable(tr("Caja stagenet", "Stagenet box"), it) }
            p.fondeoTxid?.let { Copiable(tr("Fondeo (txid)", "Funding (txid)"), it) }
            p.pagoTxid?.let { Copiable(tr("Pago (txid)", "Payment (txid)"), it) }
            p.recibo?.let {
                Card(
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(it, Modifier.padding(12.dp), style = MaterialTheme.typography.bodyMedium) }
            }
            p.cerradoTexto?.let { Ayuda(it) }
            ComoFunciona {
                Ayuda(tr("Encerrada y Pagada se marcan solo cuando el motor ve la transacción en la cadena.", "Locked and Paid are set only when the engine sees the transaction on chain."))
                Ayuda(tr("La caja es 2-de-2: ninguna de las dos personas puede mover el dinero sola.", "The box is 2-of-2: neither person can move the money alone."))
            }
        }
    }

    if (p.notas.isNotEmpty()) {
        Tarjeta(tr("Hilo", "Thread")) {
            p.notas.forEachIndexed { i, n ->
                if (i > 0) Divisor()
                Text(n.cabeza, fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.bodySmall)
                if (n.cifrada) Ayuda(n.cuerpo) else if (n.cuerpo.isNotEmpty()) Text(n.cuerpo, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }

    if (p.cajaDireccion != null) {
        Plegable(tr("Importar share suelto", "Import standalone share"), tr("Avanzado · archivos de 0.2.7 o antes", "Advanced · files from 0.2.7 or earlier")) {
            CajaRespaldo(acciones)
        }
    }

    if (p.puedeEditar || p.puedeSalirLocal) {
        Plegable(tr("Avanzado", "Advanced"), listOfNotNull(
            if (p.puedeEditar) tr("Texto", "Text") else null,
            if (p.puedeSalirLocal) tr("salir en este equipo", "leave on this device") else null,
        ).joinToString(" · ")) {
            if (p.puedeEditar) {
                OutlinedTextField(detalle, { detalle = it }, label = { Text(tr("Texto de la partida", "Stage text")) }, modifier = Modifier.fillMaxWidth())
                Secundario(tr("Guardar texto", "Save text")) { acciones.correr(tr("Texto guardado.", "Text saved."), yaEnPantalla = yaVisible) { app.editarDetalle(obraId, indice, detalle) } }
            }
            if (p.puedeSalirLocal) {
                Peligro(tr("Abandonar partida (solo este equipo)", "Leave stage (this device only)")) {
                    acciones.correr(tr("Cancelé fondeo/propuesta locales. Fondos en cadena intactos.", "Cancelled local funding/proposal. On-chain funds untouched."), yaEnPantalla = yaVisible) {
                        app.salirPartidaLocal(obraId, indice)
                    }
                }
                Ayuda(tr("Cancela fondeo o propuesta locales. No mueve monedas ni firma por el otro.", "Cancels a local funding or proposal. It does not move coins or sign for the other side."))
            }
        }
    }
}
