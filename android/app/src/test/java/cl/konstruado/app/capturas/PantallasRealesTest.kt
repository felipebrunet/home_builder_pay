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
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Nav
import cl.konstruado.app.ui.screens.BienvenidaScreen
import cl.konstruado.app.ui.screens.ModoBienvenida
import cl.konstruado.app.ui.screens.BilleteraScreen
import cl.konstruado.app.ui.screens.CuentaScreen
import cl.konstruado.app.ui.screens.ObraScreen
import cl.konstruado.app.ui.screens.PartidaScreen
import cl.konstruado.app.ui.screens.TableroScreen
import cl.konstruado.app.ui.screens.PublicarScreen
import cl.konstruado.app.ui.Idioma
import cl.konstruado.app.ui.tr
import cl.konstruado.app.ui.rememberAcciones
import cl.konstruado.app.ui.screens.VerSemilla
import cl.konstruado.app.ui.theme.KonstruadoTheme
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import uniffi.konstruado_ffi.KonstruadoApp
import java.io.File

/**
 * Pantallas reales (motor Rust por JNA, lib del host en target/debug) sobre un
 * perfil de demo. Solo corre con `-Pcapturas=DIR -Pdemo=DIR_DATOS`.
 * Sirve para el antes/después: mismo perfil, mismas pantallas.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "w393dp-h1800dp-xxhdpi")
class PantallasRealesTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private val salida: String? = System.getProperty("konstruado.capturas")
    private val demo: String? = System.getProperty("konstruado.demo")

    private fun app(): KonstruadoApp {
        assumeTrue("sin -Pcapturas/-Pdemo", salida != null && demo != null)
        AppHolder.appOrNull()?.let { return it }
        val dir = File(System.getProperty("java.io.tmpdir"), "konstruado-capturas-" + System.nanoTime())
        File(demo!!).copyRecursively(dir, overwrite = true)
        val a = KonstruadoApp.nuevo(dir.absolutePath, null, null, emptyList(), false)
        // -Pidioma=en: capturas en inglés (UI y textos del motor).
        if (idioma == "en") a.fijarIdioma("en")
        Idioma.en = idioma == "en"
        val f = AppHolder::class.java.getDeclaredField("app")
        f.isAccessible = true
        f.set(AppHolder, a)
        return a
    }

    private val oscuro = System.getProperty("konstruado.oscuro") == "1"
    private val idioma: String? = System.getProperty("konstruado.idioma")

    private fun foto(nombre: String, titulo: String, contenido: @Composable () -> Unit) {
        compose.mainClock.autoAdvance = false
        compose.setContent {
            KonstruadoTheme(dark = oscuro) {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    Column(Modifier.fillMaxSize().padding(16.dp)) {
                        Text("Konstruado · $titulo", style = MaterialTheme.typography.titleLarge)
                        contenido()
                    }
                }
            }
        }
        // Deja correr los sondeos (1 s) y las lecturas en Dispatchers.IO.
        repeat(12) {
            compose.mainClock.advanceTimeBy(250)
            Thread.sleep(120)
        }
        val sufijo = (if (oscuro) "-oscuro" else "") + (if (idioma == "en") "-en" else "")
        compose.onRoot().captureRoboImage(File(salida!!, "$nombre$sufijo.png").absolutePath)
    }

    @Test fun billetera() { app(); foto("android-billetera", tr("Billetera","Wallet")) { BilleteraScreen(Banner()) } }

    @Test fun cuenta() { app(); foto("android-cuenta", tr("Cuenta","Account")) { CuentaScreen(Banner()) } }

    @Test fun tablero() { app(); foto("android-tablero", tr("Tablero","Board")) { TableroScreen(Nav(), Banner()) } }

    @Test fun obra() {
        val id = app().tablero().obras.first().id
        foto("android-obra", tr("Obra","Job")) { ObraScreen(id, Nav(), Banner()) }
    }

    @Test fun partida() {
        val id = app().tablero().obras.first().id
        foto("android-partida", tr("Partida","Stage")) { PartidaScreen(id, 0u, Banner()) }
    }

    @Test fun publicarUsd() {
        // Precio real (Kraken, Bitfinex, CoinGecko, CoinPaprika) antes de dibujar; sin red queda el aviso.
        val a = app()
        runCatching { a.actualizarPrecio() }
        foto("android-publicar-usd", tr("Publicar","Post a job")) { PublicarScreen(Nav(), Banner()) }
    }

    @Test fun bienvenida() { app(); foto("android-bienvenida", tr("Bienvenida","Welcome")) { BienvenidaScreen(Banner()) {} } }

    @Test fun bienvenidaRestaurar() {
        app(); foto("android-bienvenida-restaurar", tr("Restaurar","Restore")) { BienvenidaScreen(Banner(), ModoBienvenida.Restaurar) {} }
    }

    @Test fun semillaOculta() {
        app()
        foto("android-semilla-oculta", tr("Semilla","Seed")) {
            val b = Banner()
            VerSemilla(rememberAcciones(b), b, pasoInicial = 0, verVkInicial = true)
        }
    }

    @Test fun semillaAviso() {
        app()
        foto("android-semilla-aviso", tr("Semilla","Seed")) {
            val b = Banner()
            VerSemilla(rememberAcciones(b), b, pasoInicial = 1)
        }
    }

    @Test fun semillaRevelada() {
        app()
        foto("android-semilla-revelada", tr("Semilla","Seed")) {
            val b = Banner()
            VerSemilla(rememberAcciones(b), b, pasoInicial = 2, verVkInicial = true)
        }
    }
}

