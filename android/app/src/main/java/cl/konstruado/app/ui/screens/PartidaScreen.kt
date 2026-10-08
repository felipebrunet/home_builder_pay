package cl.konstruado.app.ui.screens

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

private const val SINCRONIZANDO = "Sincronizando el trato… las acciones esperan a bajar el estado del otro."

/** Estado de la partida: tono, título de una línea y detalle (2 líneas reservadas). */
private data class EstadoPartida(val tono: Tono, val titulo: String, val detalle: String, val enCurso: Boolean)

private fun estadoPartida(p: PartidaVista): EstadoPartida {
    // Una sola línea de estado de caja/fondeo (sin repetir "Encerrando"); misma regla que el escritorio.
    val estadoCaja = p.saldoEstado ?: p.linea?.takeIf { !p.lineaFreno }
    if (p.lineaFreno) return EstadoPartida(Tono.Error, "Frenado", p.linea ?: "", false)
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
    val r = sondear(obraId, indice) { app.partidaVista(obraId, indice) } ?: run { Pista("Cargando…"); return }
    val p = r.getOrElse { ErrorTexto(it.humano()); return }
    var pct by remember(obraId, indice) { mutableStateOf("100") }
    var nota by remember(obraId, indice) { mutableStateOf("") }
    var detalle by remember(obraId, indice) { mutableStateOf(p.detalle) }
    var confirmaEncerrar by remember(obraId, indice) { mutableStateOf(false) }
    val yaVisible = {
        listOfNotNull(
            p.linea, p.pista, p.saldoEstado, p.saldoDetalle,
            if (p.sincronizando) "Sincronizando el trato" else null,
            if (p.sincronizando) "El otro no está en línea" else null,
        )
    }
    val e = estadoPartida(p)

    Tarjeta("Ahora") {
        Ayuda(p.obraNombre, maxLines = 1)
        Text("${indice + 1u}  ${p.titulo}", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
        Chips {
            // Lo que está en curso va en la tarjeta de estado de abajo (sin repetirlo acá).
            Chip(p.label, tonoPartida(p.estado))
            if (p.lineaFreno) Chip("frenado", Tono.Error)
            p.trabaCorta?.takeIf { p.enCurso == null && !p.lineaFreno }?.let { Chip(it, Tono.Espera) }
            if (p.miTurno && !p.lineaFreno && !p.pagoEnCurso && p.enCurso == null && p.trabaCorta == null) Chip("te toca", Tono.Ok)
            if (!p.miTurno && !p.pagoEnCurso) p.esperaA?.let { Chip("esperando a $it", Tono.Espera) }
        }
        if (e.detalle != p.lead) Ayuda(p.lead)
        // Obras en USD: XMR fijo (o aproximado antes de encerrar).
        p.xmrPartida?.let { Ayuda(it) }
        EstadoTarjeta(e.tono, e.titulo, e.detalle, e.enCurso, maxLineas = if (p.lineaFreno) 8 else 3)
        p.pista?.let { Ayuda(it) }

        // Solo las acciones válidas ahora (flags de `acciones_partida`, compartidas con el escritorio).
        if (p.puedeProponerEncerrar) {
            p.estadoPrecio?.let {
                Ayuda("Al proponer, el XMR de esta partida queda fijo con el precio de ahora. El otro lo ve antes de confirmar.")
                Ayuda(it)
            }
            if (confirmaEncerrar) {
                Primario("Proponer encerrar") {
                    acciones.correr("Propuesta enviada. Falta que el otro confirme y fondee.", alTerminar = { confirmaEncerrar = false }, yaEnPantalla = yaVisible) {
                        app.proponerEncerrar(obraId, indice)
                    }
                }
                TextoBoton("No") { confirmaEncerrar = false }
            } else {
                Primario("Encerrar esta partida") { confirmaEncerrar = true }
            }
        }
        if (p.puedeConfirmarFondear) {
            p.precioPropuesto?.let { Lead(it) }
            p.avisoPrecio?.let { EstadoTarjeta(Tono.Espera, "Precio distinto", it, enCurso = false, maxLineas = 4) }
            Primario("Confirmar y fondear") {
                acciones.correr("Armando el fondeo con las dos billeteras…", yaEnPantalla = yaVisible) { app.confirmarYFondear(obraId, indice) }
            }
        }
        if (p.puedeEmpezarFondeoDeNuevo || p.puedeReintentarFondeo) {
            Primario("Empezar el fondeo de nuevo") {
                acciones.correr("Reinicié el fondeo. Se arman anillos frescos con el otro…", yaEnPantalla = yaVisible) {
                    app.empezarFondeoDeNuevo(obraId, indice)
                }
            }
            ComoFunciona("¿Cuándo sirve?") {
                Ayuda("Si el nodo rechazó la tx (decoys viejos). La obra y el encierre siguen; solo se arma de nuevo el fondeo.")
            }
        }
        if (p.puedeCancelarPropuesta) {
            Secundario("Cancelar propuesta") { acciones.correr(yaEnPantalla = yaVisible) { app.cancelarEncerrar(obraId, indice) } }
        } else if (p.puedeNoEncerrar) {
            Secundario("No encerrar") { acciones.correr(yaEnPantalla = yaVisible) { app.cancelarEncerrar(obraId, indice) } }
        }
        // Fondeo sin 10 confirmaciones: el botón se ve, deshabilitado, con la cuenta regresiva.
        p.traba?.let { EstadoTarjeta(Tono.Espera, p.trabaCorta ?: "Fondos trabados", it, enCurso = true, maxLineas = 3) }
        if (p.terminoTrabado) {
            Primario("Avisar que terminé", enabled = false) {}
        }
        if (p.puedeAvisarTermino) {
            Divisor()
            Text("Avisar que terminé", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
            OutlinedTextField(pct, { pct = it }, label = { Text("Porcentaje a cobrar") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
            OutlinedTextField(nota, { if (it.length <= p.maxNota.toInt()) nota = it },
                label = { Text("Nota (${nota.length}/${p.maxNota})") }, placeholder = { Text("Terminé las fundaciones") },
                modifier = Modifier.fillMaxWidth())
            Primario("Avisar que terminé") {
                acciones.correr("Aviso enviado.", alTerminar = { nota = "" }, yaEnPantalla = yaVisible) { app.avisarTermino(obraId, indice, pct, nota) }
            }
        }
        if (p.enTrato) {
            p.propuestoTexto?.let { Lead(it) }
            // Con el pago ya andando no se ofrece aceptar ni contraofertar otra vez.
            if (p.pagoTrabado) {
                Primario("Aceptar ${p.propuesto ?: 0u}% y pagar", enabled = false) {}
            }
            if (p.puedeAceptarPago) {
                Primario("Aceptar ${p.propuesto ?: 0u}% y pagar") {
                    acciones.correr("Firmando el pago 2-de-2 con el otro…", yaEnPantalla = yaVisible) { app.aceptarYPagar(obraId, indice) }
                }
            }
            if (p.puedeContraofertar) {
                OutlinedTextField(pct, { pct = it }, label = { Text("Otro porcentaje") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                OutlinedTextField(nota, { if (it.length <= p.maxNota.toInt()) nota = it },
                    label = { Text("Nota (${nota.length}/${p.maxNota})") }, placeholder = { Text("Falta la entrada de auto") },
                    modifier = Modifier.fillMaxWidth())
                Secundario("Proponer este porcentaje") {
                    acciones.correr("Contra enviada.", alTerminar = { nota = "" }, yaEnPantalla = yaVisible) { app.contraPago(obraId, indice, pct, nota) }
                }
            }
        }
    }

    val hayCaja = listOf(p.candado, p.xmrPorLado, p.cajaDireccion, p.fondeoTxid, p.pagoTxid, p.recibo, p.cerradoTexto, p.encerro)
        .any { it != null }
    if (hayCaja) {
        Tarjeta("Caja y pagos") {
            p.xmrPorLado?.let { Ayuda(it) }
            p.candado?.let { Ayuda(it) }
            p.encerro?.let { Ayuda(it) }
            p.cajaDireccion?.let { Copiable("Caja stagenet", it) }
            p.fondeoTxid?.let { Copiable("Fondeo (txid)", it) }
            p.pagoTxid?.let { Copiable("Pago (txid)", it) }
            p.recibo?.let {
                Card(
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(it, Modifier.padding(12.dp), style = MaterialTheme.typography.bodyMedium) }
            }
            p.cerradoTexto?.let { Ayuda(it) }
            ComoFunciona {
                Ayuda("Encerrada y Pagada se marcan solo cuando el motor ve la transacción en la cadena.")
                Ayuda("La caja es 2-de-2: ninguna de las dos personas puede mover el dinero sola.")
            }
        }
    }

    if (p.notas.isNotEmpty()) {
        Tarjeta("Hilo") {
            p.notas.forEachIndexed { i, n ->
                if (i > 0) Divisor()
                Text(n.cabeza, fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.bodySmall)
                if (n.cifrada) Ayuda(n.cuerpo) else if (n.cuerpo.isNotEmpty()) Text(n.cuerpo, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }

    if (p.cajaDireccion != null) {
        Plegable("Importar share suelto", "Avanzado · archivos de 0.2.7 o antes") {
            CajaRespaldo(acciones)
        }
    }

    if (p.puedeEditar || p.puedeSalirLocal) {
        Plegable("Avanzado", listOfNotNull(
            if (p.puedeEditar) "Texto" else null,
            if (p.puedeSalirLocal) "salir en este equipo" else null,
        ).joinToString(" · ")) {
            if (p.puedeEditar) {
                OutlinedTextField(detalle, { detalle = it }, label = { Text("Texto de la partida") }, modifier = Modifier.fillMaxWidth())
                Secundario("Guardar texto") { acciones.correr("Texto guardado.", yaEnPantalla = yaVisible) { app.editarDetalle(obraId, indice, detalle) } }
            }
            if (p.puedeSalirLocal) {
                Peligro("Abandonar partida (solo este equipo)") {
                    acciones.correr("Cancelé fondeo/propuesta locales. Fondos en cadena intactos.", yaEnPantalla = yaVisible) {
                        app.salirPartidaLocal(obraId, indice)
                    }
                }
                Ayuda("Cancela fondeo o propuesta locales. No mueve monedas ni firma por el otro.")
            }
        }
    }
}
