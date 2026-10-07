package cl.konstruado.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * Mantiene el proceso vivo en segundo plano: el bucle de Rust (gossip, Caja,
 * scan de Monero) sigue corriendo y la notificación muestra el estado de red.
 */
class SyncService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val nm = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 26) {
            nm.createNotificationChannel(
                NotificationChannel(CANAL, "Sincronización", NotificationManager.IMPORTANCE_LOW)
            )
        }
        val n = notificacion("Conectando…")
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(ID, n)
        }
        scope.launch {
            var ultima = ""
            while (isActive) {
                val linea = runCatching { AppHolder.appOrNull()?.red()?.linea }.getOrNull() ?: "Sin motor"
                if (linea != ultima) {
                    nm.notify(ID, notificacion(linea))
                    ultima = linea
                }
                delay(5000)
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == PARAR) {
            stopSelf()
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun notificacion(texto: String): Notification {
        val abrir = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CANAL)
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle("Konstruado sincronizando")
            .setContentText(texto)
            .setOngoing(true)
            .setContentIntent(abrir)
            .build()
    }

    companion object {
        private const val CANAL = "konstruado_sync"
        private const val ID = 7
        private const val PARAR = "cl.konstruado.app.PARAR"

        fun iniciar(c: Context) {
            runCatching { ContextCompat.startForegroundService(c, Intent(c, SyncService::class.java)) }
        }

        fun parar(c: Context) {
            runCatching { c.startService(Intent(c, SyncService::class.java).setAction(PARAR)) }
        }
    }
}
