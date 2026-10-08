package cl.konstruado.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

// Paleta completa (sin los violetas por defecto de Material). Todos los pares
// texto/fondo dan ≥ 4,5:1 (WCAG AA) en claro y oscuro; ver android/README.md.
private val VivoLight = lightColorScheme(
    primary = Color(0xFF1B6B4A), onPrimary = Color.White,
    primaryContainer = Color(0xFFCDEBDB), onPrimaryContainer = Color(0xFF05301F),
    secondary = Color(0xFF9A4519), onSecondary = Color.White,
    secondaryContainer = Color(0xFFFBE0D0), onSecondaryContainer = Color(0xFF3A1606),
    tertiary = Color(0xFF2F5D8C), onTertiary = Color.White,
    tertiaryContainer = Color(0xFFD6E4F5), onTertiaryContainer = Color(0xFF0D2A47),
    background = Color(0xFFF6F1E8), onBackground = Color(0xFF1F1B16),
    surface = Color(0xFFFFFBF4), onSurface = Color(0xFF1F1B16),
    surfaceVariant = Color(0xFFECE5D8), onSurfaceVariant = Color(0xFF4D4639),
    surfaceContainerLowest = Color(0xFFFFFFFF), surfaceContainerLow = Color(0xFFFBF6EE),
    surfaceContainer = Color(0xFFF5EFE5), surfaceContainerHigh = Color(0xFFEFE9DE),
    surfaceContainerHighest = Color(0xFFE9E3D8),
    outline = Color(0xFF7E7667), outlineVariant = Color(0xFFD0C7B6),
    error = Color(0xFFB3261E), onError = Color.White,
    errorContainer = Color(0xFFF9DEDC), onErrorContainer = Color(0xFF410E0B),
)

private val VivoDark = darkColorScheme(
    primary = Color(0xFF6FCB9F), onPrimary = Color(0xFF003822),
    primaryContainer = Color(0xFF1F5139), onPrimaryContainer = Color(0xFFC2F0D8),
    secondary = Color(0xFFF0A877), onSecondary = Color(0xFF4A1F05),
    secondaryContainer = Color(0xFF6B300F), onSecondaryContainer = Color(0xFFFFDCC7),
    tertiary = Color(0xFF9CC3EA), onTertiary = Color(0xFF0D2F4F),
    tertiaryContainer = Color(0xFF24476B), onTertiaryContainer = Color(0xFFD6E7FA),
    background = Color(0xFF15130F), onBackground = Color(0xFFEAE2D6),
    surface = Color(0xFF1B1914), onSurface = Color(0xFFEAE2D6),
    surfaceVariant = Color(0xFF4D4639), onSurfaceVariant = Color(0xFFD0C6B5),
    surfaceContainerLowest = Color(0xFF100E0B), surfaceContainerLow = Color(0xFF1F1C17),
    surfaceContainer = Color(0xFF24211B), surfaceContainerHigh = Color(0xFF2A2721),
    surfaceContainerHighest = Color(0xFF35322B),
    outline = Color(0xFF998F80), outlineVariant = Color(0xFF4D4639),
    error = Color(0xFFFFB4AB), onError = Color(0xFF690005),
    errorContainer = Color(0xFF93000A), onErrorContainer = Color(0xFFFFDAD6),
)

/** Fondo y texto de cada tono de estado (chips y filas de estado). */
@Immutable
data class ParTono(val fondo: Color, val texto: Color)

@Immutable
data class Tonos(val ok: ParTono, val espera: ParTono, val error: ParTono, val apagado: ParTono)

private val TonosClaros = Tonos(
    ok = ParTono(Color(0xFFD3EFDF), Color(0xFF0B4F33)),
    espera = ParTono(Color(0xFFFDEFCF), Color(0xFF6B4500)),
    error = ParTono(Color(0xFFFADBD6), Color(0xFF8C1D10)),
    apagado = ParTono(Color(0xFFE9E3D8), Color(0xFF4A4640)),
)

private val TonosOscuros = Tonos(
    ok = ParTono(Color(0xFF1E4D38), Color(0xFFC8F0DA)),
    espera = ParTono(Color(0xFF4D3A10), Color(0xFFFFE1A8)),
    error = ParTono(Color(0xFF5C1F17), Color(0xFFFFD9D2)),
    apagado = ParTono(Color(0xFF38352F), Color(0xFFDDD8CF)),
)

val LocalTonos = staticCompositionLocalOf { TonosClaros }

@Composable
fun KonstruadoTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalTonos provides if (dark) TonosOscuros else TonosClaros) {
        MaterialTheme(
            colorScheme = if (dark) VivoDark else VivoLight,
            content = content,
        )
    }
}
