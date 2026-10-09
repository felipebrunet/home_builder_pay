package cl.konstruado.app.ui

import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp

/**
 * Octicon `mark-github` (primer/octicons, MIT), 16×16. Monocromo: `Icon` lo
 * tiñe con el color del contenido, así sigue al tema claro u oscuro.
 * Fuente: assets/icon/mark-github.svg.
 */
private const val MARK_GITHUB =
    "M6.766 11.328c-2.063-.25-3.516-1.734-3.516-3.656 0-.781.281-1.625.75-2.188-.203-.515-.172-1.609.063-2.062" +
        ".625-.078 1.468.25 1.968.703.594-.187 1.219-.281 1.985-.281.765 0 1.39.094 1.953.265.484-.437 1.344-.765" +
        " 1.969-.687.218.422.25 1.515.046 2.047.5.593.766 1.39.766 2.203 0 1.922-1.453 3.375-3.547 3.64.531.344.89" +
        " 1.094.89 1.954v1.625c0 .468.391.734.86.547C13.781 14.359 16 11.53 16 8.03 16 3.61 12.406 0 7.984 0 3.563" +
        " 0 0 3.61 0 8.031a7.88 7.88 0 0 0 5.172 7.422c.422.156.828-.125.828-.547v-1.25c-.219.094-.5.156-.75.156" +
        "-1.031 0-1.64-.562-2.078-1.609-.172-.422-.36-.672-.719-.719-.187-.015-.25-.093-.25-.187 0-.188.313-.328" +
        ".625-.328.453 0 .844.281 1.25.86.313.452.64.655 1.031.655s.641-.14 1-.5c.266-.265.47-.5.657-.656"

val IconoGithub: ImageVector by lazy {
    ImageVector.Builder("mark-github", 16.dp, 16.dp, 16f, 16f)
        .addPath(pathData = addPathNodes(MARK_GITHUB), fill = SolidColor(Color.Black))
        .build()
}

/** «Código en GitHub» / «Source on GitHub»: abre el repo en el navegador del sistema. */
@Composable
fun EnlaceGithub(url: String) {
    val ctx = LocalContext.current
    OutlinedButton(onClick = {
        try {
            ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (_: ActivityNotFoundException) {
        }
    }) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(IconoGithub, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(tr("Código en GitHub", "Source on GitHub"))
        }
    }
}
