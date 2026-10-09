package cl.konstruado.app

import cl.konstruado.app.ui.tr
import cl.konstruado.app.ui.Idioma
import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import cl.konstruado.app.ui.KonstruadoNav
import cl.konstruado.app.ui.theme.KonstruadoTheme
import uniffi.konstruado_ffi.KonstruadoApp

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val error = AppHolder.init(applicationContext)
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }
        if (error == null) SyncService.iniciar(this)
        enableEdgeToEdge()
        setContent {
            KonstruadoTheme {
                Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    KonstruadoNav(errorArranque = error)
                }
            }
        }
    }

    override fun onPause() {
        super.onPause()
        AppHolder.appOrNull()?.guardar()
    }
}

/** Preferencias de red del teléfono (no secretas). */
object Prefs {
    private const val NOMBRE = "konstruado_red"
    fun usarOrbot(c: Context) = c.getSharedPreferences(NOMBRE, 0).getBoolean("orbot", true)
    fun socksHost(c: Context) = c.getSharedPreferences(NOMBRE, 0).getString("socks_host", "127.0.0.1") ?: "127.0.0.1"
    fun socksPort(c: Context) = c.getSharedPreferences(NOMBRE, 0).getInt("socks_port", 9050)
    fun destinos(c: Context): List<String> =
        (c.getSharedPreferences(NOMBRE, 0).getString("destinos", "") ?: "")
            .split('\n').map { it.trim() }.filter { it.isNotEmpty() }

    fun guardarOrbot(c: Context, usar: Boolean, host: String, port: Int) {
        c.getSharedPreferences(NOMBRE, 0).edit()
            .putBoolean("orbot", usar).putString("socks_host", host).putInt("socks_port", port).apply()
    }

    fun guardarDestinos(c: Context, d: List<String>) {
        c.getSharedPreferences(NOMBRE, 0).edit().putString("destinos", d.joinToString("\n")).apply()
    }
}

/**
 * Una sola instancia por proceso: la red y el motor de Monero viven en Rust
 * (runtime tokio propio). El servicio en primer plano mantiene vivo el proceso
 * para que el gossip y la caja sigan sincronizando con la app en segundo plano.
 */
object AppHolder {
    @Volatile private var app: KonstruadoApp? = null

    fun appOrNull(): KonstruadoApp? = app
    val a: KonstruadoApp get() = app!!

    /** Devuelve un mensaje si no pudo arrancar. */
    @Synchronized
    fun init(c: Context): String? {
        if (app != null) return null
        return try {
            System.loadLibrary("konstruado_ffi")
            val datos = c.filesDir.resolve("konstruado").absolutePath
            val orbot = Prefs.usarOrbot(c)
            app = KonstruadoApp.nuevo(
                datosDir = datos,
                socksHost = if (orbot) Prefs.socksHost(c) else null,
                socksPort = if (orbot) Prefs.socksPort(c).toUShort() else null,
                destinos = Prefs.destinos(c),
                escritorio = false,
            )
            // Idioma: el elegido en el perfil; si no, el del teléfono (es/en); si no, ES.
            Idioma.en = app!!.idiomaInicial(java.util.Locale.getDefault().language) == "en"
            null
        } catch (t: Throwable) {
            tr("No pude arrancar el motor: ${t.message ?: t.javaClass.simpleName}", "Could not start the engine: ${t.message ?: t.javaClass.simpleName}")
        }
    }
}

const val ORBOT_PAQUETE = "org.torproject.android"

/** ¿Está Orbot instalado? (el manifest declara el paquete en <queries>). */
fun orbotInstalado(c: Context): Boolean = try {
    c.packageManager.getPackageInfo(ORBOT_PAQUETE, 0)
    true
} catch (_: PackageManager.NameNotFoundException) {
    false
} catch (_: Throwable) {
    false
}

fun abrirOrbot(c: Context): Boolean {
    val i = c.packageManager.getLaunchIntentForPackage(ORBOT_PAQUETE) ?: return false
    i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    c.startActivity(i)
    return true
}

/**
 * `true` si la red por defecto de esta app es una VPN (p. ej. Orbot en modo VPN
 * capturando a Konstruado), `false` si no, `null` si no se puede saber.
 *
 * Android devuelve la red por defecto *de este UID*: si Orbot está en modo
 * «Elegir aplicaciones» y Konstruado no está marcada, da Wi‑Fi y no la VPN.
 * No se puede saltar la VPN de Orbot con bindSocket: Orbot no llama a
 * VpnService.Builder.allowBypass(), así que netd rechaza el socket (EPERM).
 */
fun appEnVpn(c: Context): Boolean? {
    return try {
        val cm = c.getSystemService(ConnectivityManager::class.java) ?: return null
        val red = cm.activeNetwork ?: return false
        val caps = cm.getNetworkCapabilities(red) ?: return null
        caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)
    } catch (t: Throwable) {
        null
    }
}
