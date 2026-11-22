package pl.szklo.audio

import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import pl.szklo.MainActivity
import pl.szklo.R
import pl.szklo.SzkloApp
import pl.szklo.core.NativeEngine

/**
 * Odbiornik Spotify Connect „Szkło”: w aplikacji Spotify wybierasz urządzenie „Szkło”, a muzyka płynie
 * Spotify → librespot (w `libszklo_jni.so`) → PEŁNY silnik Szkła (te same moduły co na PC) → AAudio → słuchawki.
 *
 * Silnik dostaje parametry z [AudioHub] (suwaki działają na żywo) i zasila mierniki w UI.
 * Logowanie: przy pierwszym uruchomieniu rdzeń podaje adres logowania Spotify (stan `login <url>`), UI otwiera go
 * w przeglądarce; po zalogowaniu poświadczenia zostają w pamięci aplikacji. Po zerwaniu sieci – ponowna próba co 5 s.
 * Gdy odbiornik działa, efekt systemowy na sesji 0 jest wyłączany (żeby nie przetwarzać dźwięku dwa razy).
 */
class SpotifyConnectService : Service() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var engine: NativeEngine? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        startInForeground("Łączenie ze Spotify…")
        val e = NativeEngine.create(SAMPLE_RATE)
        if (e == null) {
            AudioHub.updateSpotify { it.copy(running = false, status = "error", error = "Brak rdzenia DSP") }
            stopSelf()
            return
        }
        engine = e
        AudioHub.attachEngine(e)
        acquireLocks()
        AudioHub.updateSpotify { SpotifyState(running = true, status = "connecting") }
        runCatching { NativeEngine.nativeInitAndroid(applicationContext) } // cpal/AAudio potrzebuje kontekstu
        e.spotifyStart(filesDir.absolutePath, DEVICE_NAME)
        scope.launch { pollStatus() }
    }

    private suspend fun pollStatus() {
        var last = ""
        var errorSince = 0L
        while (scope.isActive) {
            val raw = runCatching { NativeEngine.nativeSpotifyStatus() }.getOrDefault("error rdzeń")
            if (raw != last) {
                last = raw
                val (st, rest) = raw.split(' ', limit = 2).let { it[0] to it.getOrNull(1) }
                AudioHub.updateSpotify {
                    it.copy(
                        status = st,
                        loginUrl = if (st == "login") rest else null,
                        error = if (st == "error") rest else null,
                    )
                }
                startInForeground(
                    when (st) {
                        "login" -> "Zaloguj się do Spotify w przeglądarce"
                        "ready" -> "Gotowe – w Spotify wybierz urządzenie „$DEVICE_NAME”"
                        "playing" -> "Gra przez pełny silnik Szkła"
                        "error" -> "Błąd: ${rest ?: ""} (ponowna próba…)"
                        else -> "Łączenie ze Spotify…"
                    },
                )
                errorSince = if (st == "error" || st == "idle") System.currentTimeMillis() else 0L
            }
            // automatyczne ponowne łączenie po błędzie (np. zerwana sieć)
            if (errorSince != 0L && System.currentTimeMillis() - errorSince > 5000) {
                errorSince = 0L
                last = ""
                engine?.spotifyStart(filesDir.absolutePath, DEVICE_NAME)
            }
            delay(400)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                AudioHub.setSpotifyWanted(false) // „Wyłącz” z powiadomienia
                stopSelf()
            }
            ACTION_LOGOUT -> {
                NativeEngine.nativeSpotifyLogout(filesDir.absolutePath)
                engine?.spotifyStart(filesDir.absolutePath, DEVICE_NAME)
            }
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        runCatching { NativeEngine.nativeSpotifyStop() }
        engine?.let {
            AudioHub.detachEngine(it)
            it.close()
        }
        engine = null
        releaseLocks()
        AudioHub.updateSpotify { SpotifyState() }
        super.onDestroy()
    }

    private fun acquireLocks() {
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Szklo:SpotifyConnect").apply {
            setReferenceCounted(false)
            acquire(12 * 60 * 60 * 1000L)
        }
        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        @Suppress("DEPRECATION")
        wifiLock = wm.createWifiLock(
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) WifiManager.WIFI_MODE_FULL_LOW_LATENCY else WifiManager.WIFI_MODE_FULL_HIGH_PERF,
            "Szklo:SpotifyConnect",
        ).apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    private fun releaseLocks() {
        runCatching { wakeLock?.release() }
        runCatching { wifiLock?.release() }
        wakeLock = null
        wifiLock = null
    }

    private fun startInForeground(text: String) {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, SpotifyConnectService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n = NotificationCompat.Builder(this, SzkloApp.CHANNEL_PLAYER)
            .setSmallIcon(R.drawable.ic_stat_szklo)
            .setContentTitle("Szkło · Spotify Connect")
            .setContentText(text)
            .setContentIntent(open)
            .setOngoing(true)
            .setSilent(true)
            .addAction(0, "Wyłącz", stop)
            .build()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK else 0
        ServiceCompat.startForeground(this, NOTIFICATION_ID, n, type)
    }

    companion object {
        /** librespot dekoduje 44,1 kHz – silnik Szkła pracuje na tej częstotliwości. */
        private const val SAMPLE_RATE = 44_100
        /** „(telefon)” – żeby w Spotify na PC nie mylić odbiornika w telefonie ze Szkłem na komputerze. */
        const val DEVICE_NAME = "Szkło (telefon)"
        private const val NOTIFICATION_ID = 103
        private const val ACTION_STOP = "pl.szklo.spotify.STOP"
        private const val ACTION_LOGOUT = "pl.szklo.spotify.LOGOUT"

        fun start(context: Context) {
            AudioHub.setSpotifyWanted(true)
            ContextCompat.startForegroundService(context, Intent(context, SpotifyConnectService::class.java))
        }

        fun stop(context: Context) {
            AudioHub.setSpotifyWanted(false)
            context.stopService(Intent(context, SpotifyConnectService::class.java))
        }

        fun logout(context: Context) {
            context.startService(Intent(context, SpotifyConnectService::class.java).setAction(ACTION_LOGOUT))
        }
    }
}
