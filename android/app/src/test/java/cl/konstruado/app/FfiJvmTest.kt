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
 * de Monero para pasar: crea perfil, publica, crea semilla y exporta el respaldo completo.
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
        // Montos en USD (0.2.10): la vista previa y la oferta publicada.
        val pv = app.previaPublicar("10000", "2000")
        assertTrue(pv.ok)
        assertEquals(5u, pv.nPartidas)
        assertFalse(app.previaPublicar("10000", "0").ok)
        assertTrue(app.notaPrecio().contains("mainnet"))
        val o = app.publicarOferta("Casa JVM", "10000", "2000", listOf("Fundaciones"))
        assertEquals(5u, o.nPartidas)
        assertEquals(1, app.tablero().misOfertas.size)
        assertTrue(app.tablero().misOfertas[0].usd)
        assertFalse(app.billetera().tieneSemilla)
        val addr = app.crearSemilla()
        assertTrue(addr.startsWith("5"))
        // Respaldo completo: cifrado, con cabecera, y se revisa con la misma clave.
        assertTrue(app.estadoRespaldo().falta)
        val datos = app.exportarRespaldo("clave-de-prueba")
        assertEquals("KSTRBAK", String(datos.copyOfRange(0, 7)))
        val r = app.revisarRespaldo(datos, "clave-de-prueba")
        assertEquals("Kotlin JVM", r.nombre)
        assertEquals("mandante", r.rol)
        assertEquals(1u, r.nOfertas)
        assertEquals(addr, r.direccion)
        assertTrue(r.hayDatos)
        val mala = runCatching { app.revisarRespaldo(datos, "otra-clave-mala") }.exceptionOrNull()
        assertTrue(mala is FfiException.Fallo)
        app.respaldoGuardado()
        assertFalse(app.estadoRespaldo().falta)
        // Las 25 palabras: same as Feather / monero-wallet-cli; view key matches restore.
        assertEquals(3, app.avisosVerSemilla().size)
        assertTrue(app.semillaPortapapelesSeg() >= 30u)
        val sem = app.verSemilla()
        assertEquals(25, sem.palabras.trim().split(Regex("\\s+")).size)
        assertEquals(addr, sem.direccion)
        val llaves = app.llavesBilletera()!!
        assertEquals(addr, llaves.direccion)
        assertEquals(64, llaves.viewKey.length)
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
        // Idioma (0.3.0): el motor arma sus textos en ES o EN y queda en el perfil.
        assertEquals("es", app.idiomaInicial("es-CL"))
        app.fijarIdioma("en")
        assertEquals("en", app.idioma())
        assertTrue(app.previaPublicar("10000", "2000").texto.contains("stages"))
        assertTrue(app.notaPrecio().contains("US dollars"))
        val eErr = runCatching { app.confirmarYFondear("no-existe", 0u) }.exceptionOrNull()
        assertTrue(eErr is FfiException.Fallo && !eErr.msg.contains("todavía"))
        assertTrue(app.repositorio().endsWith("felipebrunet/konstruado"))
        // Elegido en el perfil: manda sobre el idioma del teléfono al volver a abrir.
        assertEquals("en", KonstruadoApp.nuevo(dir, null, null, emptyList(), false).idiomaInicial("es-CL"))
        app.fijarIdioma("es")
        assertTrue(app.previaPublicar("10000", "2000").texto.contains("partidas"))
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
