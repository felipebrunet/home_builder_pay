package cl.konstruado.app.ui.screens

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.Lead
import cl.konstruado.app.ui.Nav
import cl.konstruado.app.ui.Pantalla
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Seccion
import cl.konstruado.app.ui.Titulo
import cl.konstruado.app.ui.humano
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.sondear
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

@Composable
fun OfertaScreen(id: String, nav: Nav, banner: Banner) {
    val acciones = rememberAcciones(banner)
    val app = AppHolder.a
    val r = sondear(id) { app.oferta(id) } ?: run { Pista("Cargando…"); return }
    val o = r.getOrElse { ErrorTexto(it.humano()); return }
    var garantia by remember(id) { mutableStateOf(o.garantiaSugerida.toString()) }
    val detalles = remember(id) { mutableStateListOf<String>() }
    var previa by remember { mutableStateOf<uniffi.konstruado_ffi.PreviaAceptar?>(null) }
    LaunchedEffect(garantia) {
        val p = withContext(Dispatchers.IO) { app.previaAceptar(id, garantia) }
        previa = p
        if (p.ok) {
            val n = p.nPartidas.toInt()
            while (detalles.size < n) detalles.add(p.detalles.getOrElse(detalles.size) { "" })
            while (detalles.size > n) detalles.removeAt(detalles.lastIndex)
        }
    }
    Titulo(o.nombre)
    Lead(o.resumen)
    OutlinedTextField(garantia, { garantia = it }, label = { Text("Tu garantía") },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
    previa?.let { p -> if (p.ok) Pista(p.texto) else ErrorTexto(p.texto) }
    val contra = previa?.contra == true
    Seccion("Partidas")
    if (contra) {
        Pista("Al cambiar la garantía, el número de partidas cambia. Completá o ajustá los textos.")
        detalles.forEachIndexed { i, d ->
            OutlinedTextField(d, { detalles[i] = it }, label = { Text("Partida ${i + 1}") }, modifier = Modifier.fillMaxWidth())
        }
    } else {
        o.detalles.forEachIndexed { i, d -> Text("${i + 1}. ${d.ifBlank { "Partida ${i + 1}" }}") }
    }
    Primario(if (contra) "Proponer esta garantía" else "Aceptar condiciones", enabled = previa?.ok == true) {
        val d = detalles.toList()
        acciones.pedir({ app.aceptarOferta(id, garantia, d) }) { obra ->
            banner.ok.value = if (contra) "Contra enviada. El mandante tiene que confirmar." else "Aceptada. Se arma la caja 2-de-2."
            nav.raiz(Pantalla.Tablero)
            nav.ir(Pantalla.Obra(obra))
        }
    }
}
