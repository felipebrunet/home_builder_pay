package cl.konstruado.app.ui.screens

import cl.konstruado.app.ui.tr
import cl.konstruado.app.ui.Idioma
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.ErrorTexto
import cl.konstruado.app.ui.Nav
import cl.konstruado.app.ui.PanelPrecio
import cl.konstruado.app.ui.Pantalla
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Titulo
import cl.konstruado.app.ui.rememberAcciones
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

@Composable
fun PublicarScreen(nav: Nav, banner: Banner) {
    val acciones = rememberAcciones(banner)
    var nombre by remember { mutableStateOf(tr("Casa El Quisco", "Casa El Quisco")) }
    // Obras nuevas en dólares: USD 1000 de trabajo, USD 200 por partida.
    var trabajo by remember { mutableStateOf("1000") }
    var garantia by remember { mutableStateOf("200") }
    var detalles by remember { mutableStateOf("") }
    var previa by remember { mutableStateOf<uniffi.konstruado_ffi.PreviaPublicar?>(null) }
    val app = AppHolder.a
    LaunchedEffect(trabajo, garantia, Idioma.en) {
        // El precio lo mantiene el motor (Orbot si está); la vista previa usa el último.
        while (true) {
            previa = withContext(Dispatchers.IO) { app.previaPublicar(trabajo, garantia) }
            delay(5_000)
        }
    }
    Titulo(tr("Publicar obra", "Post a job"))
    OutlinedTextField(nombre, { nombre = it }, label = { Text(tr("Nombre de la obra", "Job name")) }, modifier = Modifier.fillMaxWidth())
    OutlinedTextField(trabajo, { trabajo = it }, label = { Text(tr("Trabajo (USD)", "Job amount (USD)")) },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal), modifier = Modifier.fillMaxWidth())
    OutlinedTextField(garantia, { garantia = it }, label = { Text(tr("Garantía sugerida por partida (USD)", "Suggested guarantee per stage (USD)")) },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal), modifier = Modifier.fillMaxWidth())
    previa?.let { p -> if (p.ok) Pista(p.texto) else ErrorTexto(p.texto) }
    PanelPrecio(banner)
    Ayuda(app.notaPrecio())
    OutlinedTextField(detalles, { detalles = it }, label = { Text(tr("Partidas (una por línea, opcional)", "Stages (one per line, optional)")) },
        minLines = 3, modifier = Modifier.fillMaxWidth())
    Primario(tr("Publicar en la red", "Post to the network")) {
        val d = detalles.lines().map { it.trim() }
        acciones.correr(tr("Oferta publicada.", "Offer posted."), alTerminar = { nav.raiz(Pantalla.Tablero) }) {
            AppHolder.a.publicarOferta(nombre, trabajo, garantia, d)
        }
    }
}
