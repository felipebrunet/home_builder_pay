package cl.konstruado.app.ui.screens

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Card
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
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.Lead
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Seccion
import cl.konstruado.app.ui.Titulo
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.sondear

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

    Pista(p.obraNombre)
    Titulo("${indice + 1u}  ${p.titulo}")
    AssistChip(onClick = {}, label = { Text(p.label) })
    Lead(p.lead)
    // Una sola línea de estado de caja/fondeo (sin repetir "Encerrando").
    val estadoCaja = p.saldoEstado ?: p.linea?.takeIf { !p.lineaFreno }
    estadoCaja?.let { Lead(it) }
    p.saldoDetalle?.let { Pista(it) }
    p.candado?.let { Pista(it) }
    p.xmrPorLado?.let { Pista(it) }
    p.cajaDireccion?.let { Copiable("Caja stagenet", it) }
    p.fondeoTxid?.let { Copiable("Fondeo (txid)", it) }
    p.pagoTxid?.let { Copiable("Pago (txid)", it) }
    // Freno del motor: solo en el cuerpo (rojo). El banner no lo repite.
    if (p.lineaFreno) p.linea?.let { ErrorTexto(it) }
    else if (p.linea != null && p.linea != estadoCaja) Pista(p.linea!!)
    if (p.sincronizando) Pista("Sincronizando el trato… las acciones esperan a bajar el estado del otro.")
    p.recibo?.let { Card(Modifier.fillMaxWidth().padding(vertical = 4.dp)) { Text(it, Modifier.padding(12.dp)) } }
    p.cerradoTexto?.let { Pista(it) }
    p.encerro?.let { Pista(it) }
    val yaVisible = {
        listOfNotNull(
            p.linea,
            p.pista,
            p.saldoEstado,
            p.saldoDetalle,
            if (p.sincronizando) "Sincronizando el trato" else null,
            if (p.sincronizando) "El otro no está en línea" else null,
        )
    }

    if (p.puedeEditar) {
        OutlinedTextField(detalle, { detalle = it }, label = { Text("Texto") }, modifier = Modifier.fillMaxWidth())
        Secundario("Guardar texto") { acciones.correr("Texto guardado.", yaEnPantalla = yaVisible) { app.editarDetalle(obraId, indice, detalle) } }
    }
    if (p.notas.isNotEmpty()) {
        Seccion("Hilo")
        p.notas.forEach { n ->
            Card(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
                Column(Modifier.padding(10.dp)) {
                    Text(n.cabeza, fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.bodySmall)
                    if (n.cifrada) Pista(n.cuerpo) else if (n.cuerpo.isNotEmpty()) Text(n.cuerpo)
                }
            }
        }
    }
    p.pista?.let { Pista(it) }
    if (p.puedeProponerEncerrar) {
        if (confirmaEncerrar) {
            Primario("Proponer encerrar") {
                acciones.correr("Propuesta enviada. Falta que el otro confirme y fondee.", alTerminar = { confirmaEncerrar = false }, yaEnPantalla = yaVisible) {
                    app.proponerEncerrar(obraId, indice)
                }
            }
            Secundario("No") { confirmaEncerrar = false }
        } else {
            Primario("Encerrar esta partida") { confirmaEncerrar = true }
        }
    }
    if (p.puedeConfirmarFondear) {
        Primario("Confirmar y fondear") {
            acciones.correr("Armando el fondeo con las dos billeteras…", yaEnPantalla = yaVisible) { app.confirmarYFondear(obraId, indice) }
        }
    }
    if (p.puedeEmpezarFondeoDeNuevo || p.puedeReintentarFondeo) {
        Primario("Empezar el fondeo de nuevo") {
            acciones.correr(
                "Reinicié el fondeo. Se arman anillos frescos con el otro…",
                yaEnPantalla = yaVisible,
            ) { app.empezarFondeoDeNuevo(obraId, indice) }
        }
        Pista("Sirve si el nodo rechazó la tx (decoys viejos). La obra y el encierre siguen; solo se arma de nuevo el fondeo.")
    }
    if (p.puedeCancelarPropuesta) {
        Secundario("Cancelar propuesta") { acciones.correr(yaEnPantalla = yaVisible) { app.cancelarEncerrar(obraId, indice) } }
    } else if (p.puedeNoEncerrar) {
        Secundario("No encerrar") { acciones.correr(yaEnPantalla = yaVisible) { app.cancelarEncerrar(obraId, indice) } }
    }
    if (p.puedeAvisarTermino) {
        Seccion("Avisar que terminé")
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
        p.esperaA?.let { Pista("Esperando a $it.") }
        if (p.miTurno) {
            Primario("Aceptar ${p.propuesto ?: 0u}% y pagar") {
                acciones.correr("Firmando el pago 2-de-2 con el otro…", yaEnPantalla = yaVisible) { app.aceptarYPagar(obraId, indice) }
            }
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
    Pista("Encerrada y Pagada se marcan solo cuando el motor ve la transacción en la cadena.")
}
