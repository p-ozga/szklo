package pl.szklo.audio

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.annotation.RequiresApi
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import pl.szklo.MainActivity
import pl.szklo.R
import pl.szklo.SzkloApp

/**
 * Usługa pierwszoplanowa trzymająca [GlobalEffectsBridge] przy życiu, gdy użytkownik słucha
 * innej aplikacji. Rejestruje dynamicznie [EffectSessionReceiver] (jedyna pewna droga na Androidzie 8+)
 * i synchronizuje efekty z [AudioHub] (sesje + parametry).
 *
 * Typ FGS: `specialUse` (Android 14+) – efekt dla innych aplikacji nie pasuje do żadnego typu
 * standardowego; w Google Play wymaga uzasadnienia w deklaracji (patrz raport w spec/).
 *
 * Bateria: usługa sama nic nie liczy (DSP robi audioserwer w ramach DynamicsProcessing), koszt to
 * obecność procesu i powiadomienia; efekt systemowy kosztuje kilka % CPU jednego rdzenia podczas odtwarzania.
 */
class GlobalEffectsService : Service() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val receiver = EffectSessionReceiver()
    private var receiverRegistered = false
    private var bridge: GlobalEffectsBridge? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        startInForeground(0)

        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            AudioHub.updateGlobal { it.copy(running = false, supported = false, error = "Wymaga Androida 9 (DynamicsProcessing)") }
            stopSelf()
            return
        }
        startBridge()
    }

    @RequiresApi(Build.VERSION_CODES.P)
    private fun startBridge() {
        val b = GlobalEffectsBridge()
        bridge = b

        ContextCompat.registerReceiver(this, receiver, EffectSessionReceiver.filter(), ContextCompat.RECEIVER_EXPORTED)
        receiverRegistered = true
        AudioHub.updateGlobal { it.copy(running = true, supported = true, error = null) }

        // Parametry: najpierw stan, potem sesje (nowa sesja dostaje od razu aktualne ustawienia).
        // StateFlow sam scala wartości; krótka przerwa = najwyżej ~25 aktualizacji/s do audioserwera podczas przeciągania.
        scope.launch {
            AudioHub.params.collect {
                b.apply(it)
                delay(40)
            }
        }
        scope.launch {
            kotlinx.coroutines.flow.combine(
                AudioHub.global.map { it.sessions.keys },
                AudioHub.spotify.map { it.running },
            ) { s, sp -> s to sp }.distinctUntilChanged().collect { (sessions, spotifyOn) ->
                val err = b.syncSessions(sessions, allowGlobal = !spotifyOn)
                val attached = b.attachedSessions
                AudioHub.updateGlobal { it.copy(attached = attached, error = err) }
                startInForeground(attached.size) // odśwież licznik w powiadomieniu
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_DISABLE) {
            AudioHub.setGlobalRequested(false)
            stopSelf()
        }
        // START_NOT_STICKY: ponowny start po ubiciu procesu z tła mógłby skończyć się
        // ForegroundServiceStartNotAllowedException (Android 12+). Usługę wznawia MainActivity
        // przy następnym otwarciu, jeśli użytkownik ma włączony tryb „inne aplikacje”.
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        if (receiverRegistered) runCatching { unregisterReceiver(receiver) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) bridge?.release()
        bridge = null
        AudioHub.updateGlobal { it.copy(running = false, attached = emptySet()) }
        super.onDestroy()
    }

    private fun startInForeground(sessions: Int) {
        val n = buildNotification(sessions)
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
        } else {
            0 // < API 34: typ z manifestu nie jest wymagany przy wywołaniu
        }
        ServiceCompat.startForeground(this, NOTIFICATION_ID, n, type)
    }

    private fun buildNotification(sessions: Int): Notification {
        val open = PendingIntent.getActivity(
            this, 10, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val disable = PendingIntent.getService(
            this, 11, Intent(this, GlobalEffectsService::class.java).setAction(ACTION_DISABLE),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, SzkloApp.CHANNEL_GLOBAL)
            .setSmallIcon(R.drawable.ic_stat_szklo)
            .setContentTitle(getString(R.string.notif_global_title))
            .setContentText(getString(R.string.notif_global_text, sessions))
            .setContentIntent(open)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .addAction(NotificationCompat.Action(0, getString(R.string.action_disable), disable))
            .build()
    }

    companion object {
        private const val NOTIFICATION_ID = 102
        private const val ACTION_DISABLE = "pl.szklo.action.GLOBAL_DISABLE"

        /** Wołać z UI na pierwszym planie (Android 12+ zabrania startu FGS z tła). */
        fun start(context: Context) {
            runCatching {
                ContextCompat.startForegroundService(context, Intent(context, GlobalEffectsService::class.java))
            }.onFailure { e ->
                AudioHub.updateGlobal { it.copy(error = "Nie można uruchomić usługi: ${e.message}") }
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, GlobalEffectsService::class.java))
        }
    }
}
