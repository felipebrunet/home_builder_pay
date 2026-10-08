package cl.konstruado.app.ui.screens

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.BannerVista
import cl.konstruado.app.ui.Divisor
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Tarjeta
import cl.konstruado.app.ui.TextoBoton
import cl.konstruado.app.ui.Lead
import cl.konstruado.app.ui.Pista
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Seccion
import cl.konstruado.app.ui.Titulo
import cl.konstruado.app.ui.rememberAcciones

/** Qué eligió en la primera pantalla: nada todavía, crear cuenta o restaurar. */
enum class ModoBienvenida { Elegir, Crear, Restaurar }

@Composable
fun BienvenidaScreen(banner: Banner, modoInicial: ModoBienvenida = ModoBienvenida.Elegir, entrar: () -> Unit) {
    val acciones = rememberAcciones(banner)
    var modo by remember { mutableStateOf(modoInicial) }
    var nombre by remember { mutableStateOf("") }
    var rol by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp)) {
        Spacer(Modifier.height(32.dp))
        Titulo("Konstruado")
        Lead("Garantía de obra entre dos personas, con una caja 2-de-2 de Monero (stagenet).")
        Spacer(Modifier.height(16.dp))
        BannerVista(banner)
        when (modo) {
            ModoBienvenida.Elegir -> {
                Tarjeta {
                    Primario("Crear cuenta nueva") { banner.error.value = null; modo = ModoBienvenida.Crear }
                    Pista("Elegís tu nombre y si pagás la obra o la construís.")
                    Divisor()
                    Secundario("Restaurar desde respaldo") { banner.error.value = null; modo = ModoBienvenida.Restaurar }
                    Pista("Traés todo del archivo cifrado: semilla, obras, cajas, nombre y rol.")
                }
                return@Column
            }
            ModoBienvenida.Restaurar -> {
                TextoBoton("← Volver") { banner.error.value = null; modo = ModoBienvenida.Elegir }
                Tarjeta { RestaurarRespaldo(acciones, banner) }
                return@Column
            }
            ModoBienvenida.Crear -> TextoBoton("← Volver") { banner.error.value = null; modo = ModoBienvenida.Elegir }
        }
        Seccion("1 · Tu nombre")
        OutlinedTextField(nombre, { nombre = it }, label = { Text("Nombre") }, modifier = Modifier.fillMaxWidth())
        Seccion("2 · ¿Qué vas a hacer?")
        FilterChip(selected = rol == "mandante", onClick = { rol = "mandante" },
            label = { Text("Pago la obra (mandante)") })
        Pista("Publicás el trabajo y la garantía. El otro la ve.")
        FilterChip(selected = rol == "contratista", onClick = { rol = "contratista" },
            label = { Text("La construyo (contratista)") })
        Pista("Buscás lo publicado y aceptás, o proponés otra garantía.")
        Spacer(Modifier.height(16.dp))
        Primario("Entrar") {
            if (rol.isEmpty()) {
                banner.error.value = "Elegí si pagás la obra o la construís."
            } else {
                acciones.correr(alTerminar = entrar) { AppHolder.a.crearCuenta(nombre, rol) }
            }
        }
        Pista("En el teléfono la red va por Orbot (SOCKS 127.0.0.1:9050). Lo podés cambiar después en Cuenta.")
    }
}
