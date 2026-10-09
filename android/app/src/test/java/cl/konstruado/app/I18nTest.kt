package cl.konstruado.app

import cl.konstruado.app.ui.Idioma
import cl.konstruado.app.ui.tr
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Sin claves faltantes: cada texto de la UI va como `tr("es", "en")`, con las
 * dos versiones no vacías, y no queda ningún texto en español suelto.
 * Los textos que arma el motor se prueban en Rust y en [FfiJvmTest].
 */
class I18nTest {
    private val raiz = File("src/main/java/cl/konstruado/app")

    /** Literales de string de Kotlin (incluye los anidados en `${…}`): inicio, fin, texto. */
    private fun literales(src: String): List<Triple<Int, Int, String>> {
        val res = mutableListOf<Triple<Int, Int, String>>()
        fun cadena(i0: Int): Pair<Int, String> {
            var j = i0 + 1
            val sb = StringBuilder()
            while (j < src.length) {
                val c = src[j]
                when {
                    c == '\\' -> { sb.append(src, j, minOf(j + 2, src.length)); j += 2 }
                    c == '"' -> return (j + 1) to sb.toString()
                    c == '$' && j + 1 < src.length && src[j + 1] == '{' -> {
                        var depth = 1
                        var k = j + 2
                        while (k < src.length && depth > 0) {
                            when (src[k]) {
                                '"' -> { val (e, t) = cadena(k); res.add(Triple(k, e, t)); k = e; continue }
                                '{' -> depth++
                                '}' -> depth--
                            }
                            k++
                        }
                        sb.append(src, j, k); j = k
                    }
                    else -> { sb.append(c); j++ }
                }
            }
            return j to sb.toString()
        }
        var i = 0
        while (i < src.length) {
            when {
                src.startsWith("//", i) -> { val e = src.indexOf('\n', i); i = if (e < 0) src.length else e }
                src.startsWith("/*", i) -> i = src.indexOf("*/", i) + 2
                src.startsWith("\"\"\"", i) -> { val e = src.indexOf("\"\"\"", i + 3) + 3; res.add(Triple(i, e, src.substring(i + 3, e - 3))); i = e }
                src[i] == '\'' && Regex("""^'(\\.|[^\\'])'""").find(src.substring(i, minOf(i + 4, src.length))) != null ->
                    i += Regex("""^'(\\.|[^\\'])'""").find(src.substring(i, minOf(i + 4, src.length)))!!.value.length
                src[i] == '"' -> { val (e, t) = cadena(i); res.add(Triple(i, e, t)); i = e }
                else -> i++
            }
        }
        return res
    }

    private val espanol = Regex(
        """[áéíóúñ¿¡«]|\b(el|la|los|las|de|del|que|una|un|en|por|para|con|sin|no|es|tu|te|se|al|hay|ya|obra|partida|caja|billetera|cuenta|nodo|red|sala|respaldo|fondeo|garantía|tablero|oferta|ofertas|obras|partidas|monto|saldo|pago|ver|mostrar|guardar|probar|usar|abrir|cerrar|copiar|volver|aceptar|proponer|cancelar|enviar|recibir|importar|exportar|restaurar|buscar|quitar|agregar|archivar|abandonar|nombre|bloque|bloques|libre|trabado|hilo|nota|texto|cargando|esperando|activo|propio|ruta|puerto|destino|avanzado|ahora|hecho|entrar|ocultar|precio|marcando)\b""",
        RegexOption.IGNORE_CASE,
    )

    /** Claves internas (no se muestran): tonos, estados, roles, preferencias, paquetes. */
    private val internas = setOf(
        "ok", "espera", "error", "apagado", "mandante", "contratista", "sala", "pid", "respaldo", "wt", "tcp",
        "socks_ok", "pagada", "en obra", "en fondeo", "en trato", "enmarcha", "acordada", "publicada", "contra",
        "rechazada", "pendiente", "konstruado-semilla", "orbot", "destinos", "konstruado_red", "socks_host",
        "socks_port", "es", "en", "probando",
        // Los nombres de idioma van siempre en su propio idioma.
        "Español",
    )

    private fun fuentes() = raiz.walkTopDown()
        .filter { it.isFile && it.extension == "kt" && it.name != "I18n.kt" && !it.path.contains("uniffi") }
        .toList()

    @Test
    fun noQuedaEspanolSinTraducir() {
        assertTrue("no encuentro ${raiz.absolutePath}", raiz.isDirectory)
        val sueltos = mutableListOf<String>()
        for (f in fuentes()) {
            val src = f.readText()
            for ((ini, _, t) in literales(src)) {
                if (t in internas) continue
                val plano = t.replace(Regex("""\$\{[^}]*}|\$\w+"""), "")
                if (!espanol.containsMatchIn(plano)) continue
                val antes = src.substring(maxOf(0, ini - 600), ini)
                val esPrimero = Regex("""(tr|Pantalla)\(\s*$""").containsMatchIn(antes)
                val esSegundo = Regex("""(tr|Pantalla)\(\s*"(?:[^"\\]|\\.)*",\s*$""").containsMatchIn(antes)
                if (!esPrimero && !esSegundo) {
                    sueltos += "${f.name}:${src.substring(0, ini).count { it == '\n' } + 1}: $t"
                }
            }
        }
        assertTrue("Textos sin tr(es, en):\n" + sueltos.joinToString("\n"), sueltos.isEmpty())
    }

    @Test
    fun cadaTextoTieneSuIngles() {
        val llamada = Regex("""\btr\(\s*"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)"\s*\)""")
        var n = 0
        val malos = mutableListOf<String>()
        for (f in fuentes()) {
            for (m in llamada.findAll(f.readText())) {
                n++
                val (es, en) = m.destructured
                if (es.isBlank() || en.isBlank()) malos += "${f.name}: vacío «$es» / «$en»"
                // Las mismas variables en las dos versiones.
                val vars = { s: String -> Regex("""\$\{[^}]*}|\$\w+""").findAll(s).map { it.value }.toSet() }
                if (vars(es) != vars(en)) malos += "${f.name}: variables distintas «$es» / «$en»"
            }
        }
        assertTrue("muy pocas llamadas a tr: $n", n > 250)
        assertTrue(malos.joinToString("\n"), malos.isEmpty())
    }

    @Test
    fun cambiaAlToque() {
        Idioma.en = false
        assertEquals("Guardar", tr("Guardar", "Save"))
        Idioma.en = true
        assertEquals("Save", tr("Guardar", "Save"))
        assertEquals("en", Idioma.codigo)
        Idioma.en = false
    }
}
