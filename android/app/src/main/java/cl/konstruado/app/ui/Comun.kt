package cl.konstruado.app.ui

import android.content.Context
import android.net.Uri
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.konstruado_ffi.FfiException

/** Mensaje humano de una excepción del motor. */
fun Throwable.humano(): String = when (this) {
    is FfiException.Fallo -> this.msg
    else -> this.message ?: this.javaClass.simpleName
}

/** Lee del motor cada segundo en Dispatchers.IO. */
@Composable
fun <T> sondear(vararg keys: Any?, leer: () -> T): Result<T>? {
    // El idioma va en las claves: al cambiarlo se vuelve a leer al toque (textos del motor).
    val en = Idioma.en
    val estado = remember(*keys, en) { mutableStateOf<Result<T>?>(null) }
    LaunchedEffect(*keys, en) {
        while (true) {
            estado.value = withContext(Dispatchers.IO) { runCatching { leer() } }
            delay(1000)
        }
    }
    return estado.value
}

/** Banner de errores/avisos compartido por toda la app. */
class Banner {
    val error: MutableState<String?> = mutableStateOf(null)
    val ok: MutableState<String?> = mutableStateOf(null)
}

/** Corre una acción del motor fuera del hilo de UI y muestra el resultado. */
class Acciones(private val scope: CoroutineScope, private val banner: Banner) {
    fun correr(
        okMsg: String? = null,
        alTerminar: () -> Unit = {},
        /** Textos que ya se ven en la pantalla; si el error coincide, no abrimos el banner. */
        yaEnPantalla: () -> List<String> = { emptyList() },
        f: () -> Unit,
    ) {
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { f() } }
            r.onSuccess {
                banner.error.value = null
                if (okMsg != null) banner.ok.value = okMsg
                alTerminar()
            }.onFailure { e ->
                val msg = e.humano()
                if (!mensajeYaVisible(msg, yaEnPantalla())) {
                    banner.error.value = msg
                }
            }
        }
    }

    fun <T> pedir(f: () -> T, alFinal: () -> Unit = {}, ok: (T) -> Unit) {
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { f() } }
            alFinal()
            r.onSuccess { banner.error.value = null; ok(it) }.onFailure { banner.error.value = it.humano() }
        }
    }
}

/** true si el error ya está explicado en el cuerpo (misma frase o se contiene). */
fun mensajeYaVisible(msg: String, visibles: List<String>): Boolean {
    val m = msg.trim()
    if (m.isEmpty()) return false
    return visibles.any { v ->
        val t = v.trim()
        t.isNotEmpty() && (
            m.equals(t, ignoreCase = true)
                || m.contains(t, ignoreCase = true)
                || t.contains(m, ignoreCase = true)
            )
    }
}

@Composable
fun rememberAcciones(banner: Banner): Acciones {
    val scope = rememberCoroutineScope()
    return remember(banner) { Acciones(scope, banner) }
}

@Composable
fun BannerVista(banner: Banner) {
    banner.error.value?.let { e ->
        Card(
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer),
            modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
        ) {
            Row(Modifier.padding(12.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(e, color = MaterialTheme.colorScheme.onErrorContainer, modifier = Modifier.weight(1f))
                androidx.compose.material3.TextButton(onClick = { banner.error.value = null }) { Text("OK") }
            }
        }
    }
    banner.ok.value?.let { m ->
        Card(modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp)) {
            Row(Modifier.padding(12.dp)) {
                Text(m, modifier = Modifier.weight(1f))
                androidx.compose.material3.TextButton(onClick = { banner.ok.value = null }) { Text("OK") }
            }
        }
    }
}

@Composable
fun Titulo(t: String) = Text(t, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)

@Composable
fun Seccion(t: String) {
    Spacer(Modifier.height(16.dp))
    Text(t, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
    Spacer(Modifier.height(4.dp))
}

@Composable
fun Pista(t: String) = Text(t, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)

@Composable
fun Lead(t: String) = Text(t, style = MaterialTheme.typography.bodyLarge)

@Composable
fun ErrorTexto(t: String) = Text(t, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)

fun escribirUri(c: Context, uri: Uri, texto: String) {
    c.contentResolver.openOutputStream(uri, "wt")?.use { it.write(texto.toByteArray()) }
        ?: error(tr("No pude abrir el archivo elegido", "Could not open the chosen file"))
}

fun leerUri(c: Context, uri: Uri): String =
    c.contentResolver.openInputStream(uri)?.use { it.readBytes().toString(Charsets.UTF_8) }
        ?: error(tr("No pude leer el archivo elegido", "Could not read the chosen file"))

/**
 * Guardar un respaldo con el selector de Android (SAF). `contenido` se pide al
 * motor recién cuando el usuario eligió el destino.
 */
@Composable
fun rememberGuardarArchivo(acciones: Acciones, banner: Banner, contenido: () -> String, okMsg: String): (String) -> Unit {
    val ctx = LocalContext.current
    val lanzador = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/plain")) { uri ->
        if (uri != null) acciones.correr(okMsg) { escribirUri(ctx, uri, contenido()) }
    }
    return { nombre -> banner.error.value = null; lanzador.launch(nombre) }
}

/** Abrir un respaldo con el selector y pasarle el texto al motor. */
@Composable
fun rememberAbrirArchivo(acciones: Acciones, usar: (String) -> String): () -> Unit {
    val ctx = LocalContext.current
    val lanzador = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) acciones.pedir({ usar(leerUri(ctx, uri)) }) { msg -> acciones.avisar(msg) }
    }
    return { lanzador.launch(arrayOf("*/*")) }
}

fun Acciones.avisar(msg: String) = this.correr(okMsg = msg) {}

/**
 * Guardar bytes (respaldo completo) con el selector de Android (SAF, «crear
 * documento»). `contenido` se arma recién cuando hay destino; `alGuardar` corre
 * después de escribir el archivo entero.
 */
@Composable
fun rememberGuardarBytes(
    acciones: Acciones,
    banner: Banner,
    contenido: () -> ByteArray,
    alGuardar: () -> Unit,
    okMsg: String,
): (String) -> Unit {
    val ctx = LocalContext.current
    val lanzador = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri != null) acciones.correr(okMsg) {
            val b = contenido()
            ctx.contentResolver.openOutputStream(uri, "wt")?.use { it.write(b); it.flush() }
                ?: error(tr("No pude abrir el archivo elegido", "Could not open the chosen file"))
            alGuardar()
        }
    }
    return { nombre -> banner.error.value = null; lanzador.launch(nombre) }
}

/** Abrir un archivo binario con el selector (SAF) y pasarle los bytes a `usar`. */
@Composable
fun rememberAbrirBytes(acciones: Acciones, usar: (nombre: String, bytes: ByteArray) -> Unit): () -> Unit {
    val ctx = LocalContext.current
    val lanzador = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) acciones.pedir({
            val b = ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                ?: error(tr("No pude leer el archivo elegido", "Could not read the chosen file"))
            Pair(uri.lastPathSegment?.substringAfterLast('/') ?: "respaldo", b)
        }) { (n, b) -> usar(n, b) }
    }
    return { lanzador.launch(arrayOf("*/*")) }
}
