package cl.konstruado.app.capturas

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Chip
import cl.konstruado.app.ui.Chips
import cl.konstruado.app.ui.ComoFunciona
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.DatoFila
import cl.konstruado.app.ui.EstadoFila
import cl.konstruado.app.ui.Peligro
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.SaldoGrande
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.Tarjeta
import cl.konstruado.app.ui.TextoBoton
import cl.konstruado.app.ui.Tono
import cl.konstruado.app.ui.screens.RedTarjeta
import cl.konstruado.app.ui.theme.KonstruadoTheme
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import uniffi.konstruado_ffi.RedVista
import uniffi.konstruado_ffi.SalaEstado
import java.io.File

/**
 * Galería de estados con datos falsos (sin motor): los estados de Orbot/sala,
 * la fila de escaneo, saldos largos, chips y jerarquía de botones, en claro y
 * oscuro. Solo corre con `-Pcapturas=DIR`.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "w393dp-h2400dp-xxhdpi")
class GaleriaEstadosTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()
    private val salida: String? = System.getProperty("konstruado.capturas")

    private fun red(sala: SalaEstado, vivos: UInt = 0u) = RedVista(
        linea = sala.titulo, sala = sala, conectado = vivos > 0u, pares = vivos, sesionesVivas = vivos,
        socks = sala.socks, destinos = listOf("sala vhirdvyvk6…:17432 (Orbot)"), otros = emptyList(),
        red = "konstruado-red-1", onionSala = "vhirdvyvk6.onion",
    )

    private fun sala(tipo: String, tono: String, titulo: String, detalle: String) =
        SalaEstado(tipo, tono, titulo, detalle, "127.0.0.1:9050")

    @Composable
    private fun Galeria() {
        Tarjeta("Red · los cuatro casos") {
            Ayuda("Orbot no instalado")
            RedTarjeta(red(sala("sin_orbot", "error", "Orbot no está instalado", "Instalalo desde F-Droid o Google Play y encendelo.")))
            Ayuda("SOCKS no responde")
            RedTarjeta(red(sala("socks_caido", "error", "Orbot no responde en 127.0.0.1:9050", "Abrí Orbot y tocá Iniciar, o revisá el puerto SOCKS (Connection refused).")))
            Ayuda("SOCKS OK, PC apagado (el caso de la captura)")
            RedTarjeta(red(sala("sala_no_responde", "espera", "La sala no responde", "Orbot funciona. ¿Está abierto Konstruado en el PC? (onion sin respuesta)")))
            Ayuda("SOCKS OK, Tor buscando la sala")
            RedTarjeta(red(sala("socks_ok", "espera", "Orbot responde · llamando a la sala…", "Tor puede tardar hasta un minuto en encontrar la sala.")))
            Ayuda("Conectado")
            RedTarjeta(red(sala("conectado", "ok", "Conectado a la sala", "Sesiones vivas: 1."), 1u))
        }
        Tarjeta("Billetera · fila de estado (alto fijo)") {
            EstadoFila(Tono.Espera, "Mirando la cadena… quedan 1675 bloques", enCurso = true)
            EstadoFila(Tono.Espera, "Mirando la cadena… quedan 200 bloques hacia atrás", enCurso = true)
            EstadoFila(Tono.Ok, "Al día · bloque 2224778")
            EstadoFila(Tono.Error, "El nodo no responde: revisá la URL en Cuenta")
            EstadoFila(Tono.Apagado, "Sin billetera en este equipo")
        }
        Tarjeta("Saldo que entra") {
            SaldoGrande("0.524868019997")
            SaldoGrande("0.4750")
            SaldoGrande("12345.678901234567")
            DatoFila("Libre", "0.524868019997 XMR", mono = true)
        }
        Tarjeta("Chips y botones") {
            Chips {
                Chip("en curso", Tono.Espera, enCurso = true)
                Chip("frenado", Tono.Error)
                Chip("esperando a caco", Tono.Espera)
                Chip("pagada", Tono.Ok)
                Chip("pendiente", Tono.Apagado)
            }
            Primario("Primario") {}
            Secundario("Secundario") {}
            Peligro("Peligro") {}
            TextoBoton("Texto") {}
            Copiable("Pago (txid)", "6f3c9a1be0d24c55a1f8e2b7c4d9a0e1f2b3c4d5e6f708192a3b4c5d6e7f8091")
            ComoFunciona { Ayuda("Ayuda plegada.") }
        }
    }

    private fun foto(nombre: String, oscuro: Boolean) {
        assumeTrue("sin -Pcapturas", salida != null)
        compose.mainClock.autoAdvance = false
        compose.setContent {
            KonstruadoTheme(dark = oscuro) {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    Column(Modifier.fillMaxSize().padding(16.dp)) {
                        Text("Konstruado · estados", style = MaterialTheme.typography.titleLarge)
                        Galeria()
                    }
                }
            }
        }
        repeat(4) { compose.mainClock.advanceTimeBy(250); Thread.sleep(50) }
        compose.onRoot().captureRoboImage(File(salida!!, "$nombre.png").absolutePath)
    }

    @Test fun claro() = foto("android-estados-claro", false)

    @Test fun oscuro() = foto("android-estados-oscuro", true)
}
