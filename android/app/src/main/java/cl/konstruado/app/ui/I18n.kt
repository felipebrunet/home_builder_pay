package cl.konstruado.app.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Idioma de la interfaz: el mismo mecanismo que el escritorio (`lang.t(es, en)`).
 * Cada texto va con sus dos versiones en el lugar donde se usa, así no puede
 * faltar una traducción. Los textos que arma el motor Rust salen en el mismo
 * idioma (`KonstruadoApp.fijarIdioma`). Cambiar `en` recompone la UI al toque.
 */
object Idioma {
    var en by mutableStateOf(false)

    val codigo: String get() = if (en) "en" else "es"
}

/** Texto en el idioma activo. */
fun tr(es: String, en: String): String = if (Idioma.en) en else es
