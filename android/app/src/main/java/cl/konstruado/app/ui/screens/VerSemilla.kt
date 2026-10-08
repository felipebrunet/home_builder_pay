package cl.konstruado.app.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.WindowManager
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import cl.konstruado.app.AppHolder
import cl.konstruado.app.ui.Acciones
import cl.konstruado.app.ui.Ayuda
import cl.konstruado.app.ui.Banner
import cl.konstruado.app.ui.Copiable
import cl.konstruado.app.ui.Divisor
import cl.konstruado.app.ui.Primario
import cl.konstruado.app.ui.Secundario
import cl.konstruado.app.ui.TextoBoton
import uniffi.konstruado_ffi.SemillaVistaFfi

/**
 * «Ver las 25 palabras» de la billetera personal + dirección/view key.
 * FLAG_SECURE mientras se ven las palabras; portapapeles marcado sensible y
 * borrado a los N segundos si todavía tiene la semilla.
 */
@Composable
fun VerSemilla(
    acciones: Acciones,
    banner: Banner,
    /** Solo para capturas: arranca en aviso (1) o revelado (2). */
    pasoInicial: Int = 0,
    verVkInicial: Boolean = false,
) {
    val app = AppHolder.a
    val ctx = LocalContext.current
    val view = LocalView.current
    var paso by remember { mutableStateOf(pasoInicial) } // 0 oculto, 1 aviso, 2 mostrado
    var semilla by remember {
        mutableStateOf(
            if (pasoInicial == 2) runCatching { app.verSemilla() }.getOrNull() else null,
        )
    }
    var verVk by remember { mutableStateOf(verVkInicial) }
    val llaves = remember { app.llavesBilletera() }
    val seg = remember { app.semillaPortapapelesSeg().toLong() }

    // Al salir de esta sección (o de la pantalla) se oculta y se quita FLAG_SECURE.
    DisposableEffect(Unit) {
        onDispose {
            paso = 0
            semilla = null
            (view.context as? android.app.Activity)?.window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
    }
    DisposableEffect(paso) {
        val win = (view.context as? android.app.Activity)?.window
        if (paso == 2) {
            win?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        } else {
            win?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
        onDispose { }
    }

    Text("Las 25 palabras y la view key", style = MaterialTheme.typography.titleSmall)
    Ayuda("Para abrir esta billetera personal en Feather o monero-wallet-cli (stagenet).")

    when (paso) {
        0 -> Secundario("Ver las 25 palabras") { paso = 1; banner.error.value = null }
        1 -> {
            Column(
                Modifier
                    .fillMaxWidth()
                    .background(MaterialTheme.colorScheme.errorContainer, RoundedCornerShape(10.dp))
                    .padding(12.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                // Los avisos vienen del motor (caja::aviso_ver_semilla); no leen la semilla.
                remember { app.avisosVerSemilla() }.forEachIndexed { i, t ->
                    Text(
                        t,
                        color = MaterialTheme.colorScheme.onErrorContainer,
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = if (i == 0) FontWeight.SemiBold else FontWeight.Normal,
                    )
                }
            }
            Primario("Mostrar") {
                acciones.pedir({ app.verSemilla() }) {
                    semilla = it
                    paso = 2
                }
            }
            TextoBoton("Cancelar") { paso = 0 }
        }
        2 -> {
            val v = semilla
            if (v != null) {
                GrillaPalabras(v.palabras)
                Text(
                    "Altura de restauración (bloque)",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    v.altura?.toString() ?: "—",
                    fontFamily = FontFamily.Monospace,
                    fontSize = 14.sp,
                )
                Ayuda("En la otra billetera elegí «restaurar desde semilla», red stagenet, y poné este bloque.")
                Secundario("Copiar las 25 palabras") {
                    copiarSensible(ctx, v.palabras, seg)
                    banner.ok.value = v.avisoCopia
                }
            }
            Primario("Ocultar") {
                paso = 0
                semilla = null
            }
        }
    }

    val l = llaves
    if (l != null) {
        Divisor()
        Copiable("Tu dirección stagenet", l.direccion)
        TextoBoton(if (verVk) "Ocultar view key" else "Mostrar view key") { verVk = !verVk }
        if (verVk) {
            Copiable("View key de la billetera", l.viewKey)
            Ayuda(l.ayuda)
        }
    }
}

@Composable
private fun GrillaPalabras(palabras: String) {
    val items = palabras.split(Regex("\\s+")).filter { it.isNotEmpty() }
    Column(
        Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceContainerHighest, RoundedCornerShape(10.dp))
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        items.chunked(2).forEachIndexed { fila0, fila ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                fila.forEachIndexed { j, w ->
                    // Número por posición (la palabra de checksum repite una anterior).
                    val n = fila0 * 2 + j + 1
                    Box(Modifier.weight(1f)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                "$n",
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                fontSize = 12.sp,
                                modifier = Modifier.padding(end = 8.dp),
                            )
                            Text(w, fontFamily = FontFamily.Monospace, fontSize = 14.sp)
                        }
                    }
                }
                if (fila.size == 1) Box(Modifier.weight(1f)) {}
            }
        }
    }
}

/** Copia marcada como sensible (API 33+) y borra el portapapeles a los `segundos` si sigue igual. */
fun copiarSensible(ctx: Context, texto: String, segundos: Long) {
    val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    val clip = ClipData.newPlainText("konstruado-semilla", texto)
    if (Build.VERSION.SDK_INT >= 33) {
        clip.description.extras = android.os.PersistableBundle().apply {
            putBoolean(android.content.ClipDescription.EXTRA_IS_SENSITIVE, true)
        }
    }
    cm.setPrimaryClip(clip)
    Handler(Looper.getMainLooper()).postDelayed({
        val actual = runCatching { cm.primaryClip?.getItemAt(0)?.coerceToText(ctx)?.toString() }.getOrNull()
        if (actual == texto) {
            cm.setPrimaryClip(ClipData.newPlainText("", ""))
            if (Build.VERSION.SDK_INT >= 28) cm.clearPrimaryClip()
        }
    }, segundos * 1000)
}
