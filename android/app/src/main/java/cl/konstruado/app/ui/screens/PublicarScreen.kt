package cl.konstruado.app.ui.screens

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Nav
import cl.konstruado.app.ui.Pantalla
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Titulo
import cl.konstruado.app.ui.rememberAcciones

@Composable
fun PublicarScreen(nav: Nav, banner: Banner) {
    val acciones = rememberAcciones(banner)
    var nombre by remember { mutableStateOf("Casa El Quisco") }
    var trabajo by remember { mutableStateOf("10000") }
    var garantia by remember { mutableStateOf("2000") }
    var detalles by remember { mutableStateOf("") }
    Titulo("Publicar obra")
    OutlinedTextField(nombre, { nombre = it }, label = { Text("Nombre de la obra") }, modifier = Modifier.fillMaxWidth())
    OutlinedTextField(trabajo, { trabajo = it }, label = { Text("Trabajo (unidades)") },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
    OutlinedTextField(garantia, { garantia = it }, label = { Text("Garantía sugerida por partida") },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
    Pista("En stagenet, 1 unidad son 0,00002 XMR. La garantía de 2000 son 0,04 XMR por lado.")
    OutlinedTextField(detalles, { detalles = it }, label = { Text("Partidas (una por línea, opcional)") },
        minLines = 3, modifier = Modifier.fillMaxWidth())
    Primario("Publicar en la red") {
        val d = detalles.lines().map { it.trim() }
        acciones.correr("Oferta publicada.", alTerminar = { nav.raiz(Pantalla.Tablero) }) {
            AppHolder.a.publicarOferta(nombre, trabajo, garantia, d)
        }
    }
}
