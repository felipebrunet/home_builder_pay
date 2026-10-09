package cl.konstruado.app.ui.screens

import cl.konstruado.app.ui.tr
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
        Lead(tr("Garantía de obra entre dos personas, con una caja 2-de-2 de Monero (stagenet).", "A building-work guarantee between two people, with a 2-of-2 Monero box (stagenet)."))
        Spacer(Modifier.height(16.dp))
        BannerVista(banner)
        when (modo) {
            ModoBienvenida.Elegir -> {
                Tarjeta {
                    Primario(tr("Crear cuenta nueva", "Create a new account")) { banner.error.value = null; modo = ModoBienvenida.Crear }
                    Pista(tr("Elegís tu nombre y si pagás la obra o la construís.", "Pick your name and whether you pay for the job or build it."))
                    Divisor()
                    Secundario(tr("Restaurar desde respaldo", "Restore from backup")) { banner.error.value = null; modo = ModoBienvenida.Restaurar }
                    Pista(tr("Traés todo del archivo cifrado: semilla, obras, cajas, nombre y rol.", "Bring everything from the encrypted file: seed, jobs, boxes, name and role."))
                }
                return@Column
            }
            ModoBienvenida.Restaurar -> {
                TextoBoton(tr("← Volver", "← Back")) { banner.error.value = null; modo = ModoBienvenida.Elegir }
                Tarjeta { RestaurarRespaldo(acciones) }
                return@Column
            }
            ModoBienvenida.Crear -> TextoBoton(tr("← Volver", "← Back")) { banner.error.value = null; modo = ModoBienvenida.Elegir }
        }
        Seccion(tr("1 · Tu nombre", "1 · Your name"))
        OutlinedTextField(nombre, { nombre = it }, label = { Text(tr("Nombre", "Name")) }, modifier = Modifier.fillMaxWidth())
        Seccion(tr("2 · ¿Qué vas a hacer?", "2 · What will you do?"))
        FilterChip(selected = rol == "mandante", onClick = { rol = "mandante" },
            label = { Text(tr("Pago la obra (mandante)", "I pay for the job (client)")) })
        Pista(tr("Publicás el trabajo y la garantía. El otro la ve.", "You post the job and the guarantee. The other side sees it."))
        FilterChip(selected = rol == "contratista", onClick = { rol = "contratista" },
            label = { Text(tr("La construyo (contratista)", "I build it (contractor)")) })
        Pista(tr("Buscás lo publicado y aceptás, o proponés otra garantía.", "You look at posted jobs and accept, or propose another guarantee."))
        Spacer(Modifier.height(16.dp))
        Primario(tr("Entrar", "Enter")) {
            if (rol.isEmpty()) {
                banner.error.value = tr("Elegí si pagás la obra o la construís.", "Choose whether you pay for the job or build it.")
            } else {
                acciones.correr(alTerminar = entrar) { AppHolder.a.crearCuenta(nombre, rol) }
            }
        }
        Pista(tr("En el teléfono la red va por Orbot (SOCKS 127.0.0.1:9050). Lo podés cambiar después en Cuenta.", "On the phone the network goes through Orbot (SOCKS 127.0.0.1:9050). You can change it later in Account."))
    }
}
