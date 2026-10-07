package cl.konstruado.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.konstruado_ffi.FfiException
import uniffi.konstruado_ffi.KonstruadoApp
import java.nio.file.Files

/**
 * Bindings Kotlin + JNA + libkonstruado_ffi (host, target/debug). No toca la red
 * de Monero para pasar: crea perfil, publica, crea semilla y exporta respaldo.
 * Si SALA_TCP está definido (p. ej. 127.0.0.1:17432), espera la sesión viva.
 */
class FfiJvmTest {
    @Test
    fun fachadaDesdeKotlin() {
        val dir = Files.createTempDirectory("konstruado-jvm").toString()
        val sala = System.getenv("SALA_TCP")
        val app = KonstruadoApp.nuevo(dir, null, null, listOfNotNull(sala), false)
        assertTrue(app.version().contains("konstruado-red-1"))
        assertFalse(app.perfil().tieneCuenta)
        // Error del motor llega como FfiException.Fallo con texto en español.
        val e = runCatching { app.crearCuenta("Kotlin", "nadie") }.exceptionOrNull()
        assertTrue(e is FfiException.Fallo && e.msg.contains("Elegí"))
        val p = app.crearCuenta("Kotlin JVM", "mandante")
        assertTrue(p.tieneCuenta)
        assertEquals("mandante", p.rol)
        val o = app.publicarOferta("Casa JVM", "10000", "2000", listOf("Fundaciones"))
        assertEquals(5u, o.nPartidas)
        assertEquals(1, app.tablero().misOfertas.size)
        assertFalse(app.billetera().tieneSemilla)
        val addr = app.crearSemilla()
        assertTrue(addr.startsWith("5"))
        val respaldo = app.exportarSemilla()
        assertTrue(respaldo.length > 100)
        // Restaurar las mismas palabras: el motor lo reconoce.
        val msg = app.restaurarSemilla(respaldo)
        assertTrue(msg, msg.contains("ya son las de esta billetera"))
        // Daemon configurable: público por defecto, propio persistido, volver.
        assertTrue(app.daemonEsDefecto())
        assertEquals(app.daemonPorDefecto(), app.daemonActivo())
        val propio = app.fijarDaemon("http://100.64.0.2:38081")
        assertEquals("http://100.64.0.2:38081", propio)
        assertFalse(app.daemonEsDefecto())
        assertEquals(propio, app.daemonActivo())
        assertEquals(propio, app.billetera().daemon)
        val otra = KonstruadoApp.nuevo(dir, null, null, emptyList(), false)
        assertEquals(propio, otra.daemonActivo())
        assertEquals(app.daemonPorDefecto(), app.usarDaemonPorDefecto())
        assertTrue(app.daemonEsDefecto())
        // Fondear sin obra: error honesto.
        val f = runCatching { app.confirmarYFondear("no-existe", 0u) }.exceptionOrNull()
        assertTrue(f is FfiException.Fallo)
        if (sala != null) {
            var ok = false
            repeat(20) {
                if (!ok) {
                    ok = app.red().conectado
                    if (!ok) Thread.sleep(500)
                }
            }
            assertTrue("sesión viva con $sala: ${app.red().linea}", ok)
            println("RED ${app.red().linea}")
        }
        println("JVM OK ${app.version()} addr=${addr.take(12)}…")
    }
}
