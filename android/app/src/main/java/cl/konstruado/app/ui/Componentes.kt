package cl.konstruado.app.ui

import android.widget.Toast
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import cl.konstruado.app.ui.theme.LocalTonos
import cl.konstruado.app.ui.theme.ParTono

// ---------------------------------------------------------------- estado

/** Mismos tonos que el escritorio (`caja::Tono`): ok / espera / error / apagado. */
enum class Tono { Ok, Espera, Error, Apagado }

fun tonoDe(codigo: String): Tono = when (codigo) {
    "ok" -> Tono.Ok
    "espera" -> Tono.Espera
    "error" -> Tono.Error
    else -> Tono.Apagado
}

@Composable
fun colores(t: Tono): ParTono {
    val c = LocalTonos.current
    return when (t) {
        Tono.Ok -> c.ok
        Tono.Espera -> c.espera
        Tono.Error -> c.error
        Tono.Apagado -> c.apagado
    }
}

@Composable
private fun Indicador(enCurso: Boolean, color: androidx.compose.ui.graphics.Color) {
    Box(Modifier.size(16.dp), contentAlignment = Alignment.Center) {
        if (enCurso) {
            CircularProgressIndicator(Modifier.size(14.dp), color = color, strokeWidth = 2.dp)
        } else {
            Box(Modifier.size(10.dp).clip(CircleShape).background(color))
        }
    }
}

/**
 * Fila de estado de alto fijo: un punto (o un spinner chico si hay algo en curso)
 * y una sola línea. Cambiar de estado nunca agrega ni quita renglones.
 */
@Composable
fun EstadoFila(tono: Tono, texto: String, enCurso: Boolean = false, modifier: Modifier = Modifier) {
    val c = colores(tono)
    Surface(color = c.fondo, shape = RoundedCornerShape(10.dp), modifier = modifier.fillMaxWidth().height(40.dp)) {
        Row(Modifier.padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Indicador(enCurso, c.texto)
            Spacer(Modifier.width(10.dp))
            Text(
                texto, color = c.texto, style = MaterialTheme.typography.bodyMedium.copy(fontSize = 13.sp),
                maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/**
 * Estado con título (1 línea) y detalle (siempre 2 líneas reservadas). `maxLineas`
 * mayor deja crecer solo los textos largos (frenos), nunca achica el bloque.
 */
@Composable
fun EstadoTarjeta(tono: Tono, titulo: String, detalle: String, enCurso: Boolean = false, maxLineas: Int = 2) {
    val c = colores(tono)
    Surface(color = c.fondo, shape = RoundedCornerShape(12.dp), modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.Top) {
            Box(Modifier.padding(top = 2.dp)) { Indicador(enCurso, c.texto) }
            Spacer(Modifier.width(10.dp))
            Column {
                Text(
                    titulo, color = c.texto, style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis,
                )
                Text(
                    detalle, color = c.texto, style = MaterialTheme.typography.bodySmall,
                    minLines = 2, maxLines = maxOf(2, maxLineas), overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/** Chip de estado coloreado (en curso / frenado / esperando / ok). */
@Composable
fun Chip(texto: String, tono: Tono, enCurso: Boolean = false) {
    val c = colores(tono)
    Surface(color = c.fondo, shape = RoundedCornerShape(50)) {
        Row(Modifier.padding(horizontal = 10.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            if (enCurso) {
                CircularProgressIndicator(Modifier.size(10.dp), color = c.texto, strokeWidth = 1.5.dp)
                Spacer(Modifier.width(6.dp))
            }
            Text(texto, color = c.texto, style = MaterialTheme.typography.labelMedium, maxLines = 1)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun Chips(content: @Composable RowScope.() -> Unit) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
    ) { content() }
}

// ---------------------------------------------------------------- tarjetas

/** Sección como tarjeta, con título opcional. */
@Composable
fun Tarjeta(
    titulo: String? = null,
    resaltada: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = if (resaltada) MaterialTheme.colorScheme.primaryContainer
            else MaterialTheme.colorScheme.surfaceContainerLow,
        ),
        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            if (titulo != null) {
                Text(titulo, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            }
            content()
        }
    }
}

/** Tarjeta plegable (Respaldos, Nodo/Avanzado…). Arranca cerrada salvo que se pida. */
@Composable
fun Plegable(
    titulo: String,
    sub: String? = null,
    abierta: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    var ver by rememberSaveable(titulo) { mutableStateOfBool(abierta) }
    // `abierta` puede llegar tarde (sondeo): si pasa a true, se abre; nunca la cierra sola.
    LaunchedEffect(abierta) { if (abierta) ver = true }
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().clickable { ver = !ver }.padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(titulo, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                if (sub != null) Ayuda(sub, maxLines = 1)
            }
            Icon(
                if (ver) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore,
                contentDescription = if (ver) tr("Cerrar", "Close") else tr("Abrir", "Open"),
            )
        }
        AnimatedVisibility(ver) {
            Column(
                Modifier.padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) { content() }
        }
    }
}

private fun mutableStateOfBool(v: Boolean) = androidx.compose.runtime.mutableStateOf(v)

/** Explicación larga plegada: texto chico y apagado, se abre a pedido. */
@Composable
fun ComoFunciona(titulo: String = tr("Cómo funciona", "How it works"), content: @Composable ColumnScope.() -> Unit) {
    var ver by rememberSaveable(titulo) { mutableStateOfBool(false) }
    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier.clip(RoundedCornerShape(8.dp)).clickable { ver = !ver }.padding(vertical = 6.dp, horizontal = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Outlined.Info, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.width(6.dp))
            Text(
                titulo, style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Icon(
                if (ver) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, null,
                Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        AnimatedVisibility(ver) {
            Column(Modifier.padding(start = 22.dp, bottom = 4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                content()
            }
        }
    }
}

/** Ayuda estática: chica y apagada. */
@Composable
fun Ayuda(t: String, maxLines: Int = Int.MAX_VALUE) = Text(
    t, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
    maxLines = maxLines, overflow = TextOverflow.Ellipsis,
)

@Composable
fun Divisor() = HorizontalDivider(Modifier.padding(vertical = 4.dp), color = MaterialTheme.colorScheme.outlineVariant)

/** Etiqueta a la izquierda, valor a la derecha (una línea). */
@Composable
fun DatoFila(etiqueta: String, valor: String, mono: Boolean = false) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(
            etiqueta, style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(end = 12.dp),
        )
        Spacer(Modifier.weight(1f))
        Text(
            valor, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis,
            fontFamily = if (mono) FontFamily.Monospace else null,
        )
    }
}

/** Texto largo (dirección, txid, view key) en monoespaciada, seleccionable y con botón copiar. */
@Composable
fun Copiable(etiqueta: String, valor: String) {
    val clip = LocalClipboardManager.current
    val ctx = LocalContext.current
    Column(Modifier.fillMaxWidth()) {
        Text(etiqueta, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
            shape = RoundedCornerShape(8.dp),
            modifier = Modifier.fillMaxWidth().padding(top = 2.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                SelectionContainer(Modifier.weight(1f).padding(start = 10.dp, top = 8.dp, bottom = 8.dp)) {
                    Text(valor, fontFamily = FontFamily.Monospace, fontSize = 12.sp, lineHeight = 16.sp)
                }
                IconButton(onClick = {
                    clip.setText(AnnotatedString(valor))
                    Toast.makeText(ctx, tr("Copiado", "Copied"), Toast.LENGTH_SHORT).show()
                }) { Icon(Icons.Outlined.ContentCopy, contentDescription = tr("Copiar $etiqueta", "Copy $etiqueta")) }
            }
        }
    }
}

/**
 * Saldo que entra en pantalla: entero + 4 decimales grandes, el resto de los
 * decimales (hasta los 12 del piconero) más chicos, y «XMR» chico. La precisión
 * completa siempre está a la vista.
 */
@Composable
fun SaldoGrande(total: String) {
    val (grande, chico) = partirMonto(total)
    val tamGrande = if (grande.length > 11) 28.sp else 36.sp
    Text(
        buildAnnotatedString {
            withStyle(SpanStyle(fontSize = tamGrande, fontWeight = FontWeight.SemiBold)) { append(grande) }
            withStyle(SpanStyle(fontSize = 18.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)) { append(chico) }
            withStyle(SpanStyle(fontSize = 16.sp, fontWeight = FontWeight.Medium)) { append("  XMR") }
        },
        maxLines = 1,
        overflow = TextOverflow.Clip,
    )
}

/** "0.524868019997" → ("0.5248", "68019997"); "12.5" → ("12.5000", ""). */
fun partirMonto(total: String): Pair<String, String> {
    val t = total.trim().removeSuffix("XMR").trim()
    val punto = t.indexOf('.')
    if (punto < 0) return "$t.0000" to ""
    val ent = t.substring(0, punto)
    val dec = t.substring(punto + 1).padEnd(4, '0')
    return "$ent.${dec.take(4)}" to dec.drop(4)
}

// ---------------------------------------------------------------- botones

private val anchoBoton = Modifier.fillMaxWidth().padding(vertical = 2.dp)

/** Acción principal de la tarjeta (una por tarjeta). */
@Composable
fun Primario(t: String, enabled: Boolean = true, onClick: () -> Unit) =
    Button(onClick = onClick, enabled = enabled, modifier = anchoBoton) { Text(t) }

/** Acción secundaria. */
@Composable
fun Secundario(t: String, enabled: Boolean = true, onClick: () -> Unit) =
    OutlinedButton(onClick = onClick, enabled = enabled, modifier = anchoBoton) { Text(t) }

/** Acción que corta, borra o abandona algo. */
@Composable
fun Peligro(t: String, enabled: Boolean = true, lleno: Boolean = false, onClick: () -> Unit) {
    if (lleno) {
        Button(
            onClick = onClick, enabled = enabled, modifier = anchoBoton,
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.error,
                contentColor = MaterialTheme.colorScheme.onError,
            ),
        ) { Text(t) }
    } else {
        OutlinedButton(
            onClick = onClick, enabled = enabled, modifier = anchoBoton,
            colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
            border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.error),
        ) { Text(t) }
    }
}

/** Acción menor, sin borde (Usar el máximo, Abrir Orbot…). */
@Composable
fun TextoBoton(t: String, enabled: Boolean = true, onClick: () -> Unit) =
    TextButton(onClick = onClick, enabled = enabled) { Text(t) }
