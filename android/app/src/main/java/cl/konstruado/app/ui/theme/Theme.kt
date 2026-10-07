package cl.konstruado.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val VivoLight = lightColorScheme(
    primary = Color(0xFF1B6B4A),
    secondary = Color(0xFFC45C26),
    tertiary = Color(0xFF2F5D8C),
    background = Color(0xFFF6F1E8),
    surface = Color(0xFFFFFBF4),
)

private val VivoDark = darkColorScheme(
    primary = Color(0xFF6FCB9F),
    secondary = Color(0xFFE3955E),
    tertiary = Color(0xFF8BB7E0),
)

@Composable
fun KonstruadoTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    MaterialTheme(
        colorScheme = if (dark) VivoDark else VivoLight,
        content = content,
    )
}
