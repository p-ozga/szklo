package pl.szklo.audio

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.provider.OpenableColumns
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import pl.szklo.MainActivity
import pl.szklo.R
import pl.szklo.SzkloApp

/**
 * Usługa pierwszoplanowa odtwarzająca lokalny plik audio przez PEŁNY rdzeń Szkło (Rust, JNI).
 * To jedyna ścieżka, w której działa nasz własny DSP 1:1 – dźwięk innych aplikacji obsługuje
 * (w przybliżeniu) [GlobalEffectsService].
 *
 * Sterowanie: intencje [ACTION_PLAY] (z `data` = Uri pliku), [ACTION_PAUSE], [ACTION_RESUME], [ACTION_STOP].
 * Stan publikowany jest w [AudioHub.player].
 */
class LocalPlayerService : Service(), PcmPlayer.Listener {

    private val main = Handler(Looper.getMainLooper())
    private var player: PcmPlayer? = null
    private var pausedByFocus = false
    private var noisyRegistered = false

    private val audioManager by lazy { getSystemService(AudioManager::class.java) }

    private val focusRequest by lazy {
        AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setOnAudioFocusChangeListener({ change -> onFocusChange(change) }, main)
            .build()
    }

    /** Odłączenie słuchawek → pauza (standard Androida). */
    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) pause()
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_PLAY -> {
                val uri = intent?.data
                // Najpierw startForeground – wymagane w ciągu kilku sekund od startForegroundService.
                startInForeground(getString(R.string.notif_player_title), "Przygotowanie…", playing = false)
                if (uri == null) {
                    stopEverything()
                } else {
                    play(uri)
                }
            }
            ACTION_PAUSE -> pause()
            ACTION_RESUME -> resume()
            ACTION_STOP -> stopEverything()
        }
        // Polecenie dla nieistniejącego odtwarzania (np. stare powiadomienie) nie trzyma usługi przy życiu.
        if (player == null && intent?.action != ACTION_PLAY) stopSelf()
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        player?.requestStop()
        player = null
        abandonFocus()
        unregisterNoisy()
        AudioHub.updatePlayer { if (it.status == PlayerStatus.ERROR) it else PlayerState() }
        super.onDestroy()
    }

    // ------------------------------------------------------------------ sterowanie

    private fun play(uri: Uri) {
        player?.requestStop()
        pausedByFocus = false
        val title = displayName(uri)
        AudioHub.updatePlayer { PlayerState(status = PlayerStatus.PREPARING, title = title) }

        if (audioManager.requestAudioFocus(focusRequest) != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
            AudioHub.updatePlayer { it.copy(status = PlayerStatus.ERROR, error = "System nie przydzielił wyjścia audio (trwa rozmowa?)") }
            stopEverything()
            return
        }
        registerNoisy()
        player = PcmPlayer(applicationContext, uri, this).also { it.start() }
        updateNotification()
    }

    private fun pause() {
        val p = player ?: return
        p.pausePlayback()
        AudioHub.updatePlayer { it.copy(status = PlayerStatus.PAUSED) }
        updateNotification()
    }

    private fun resume() {
        val p = player ?: return
        if (audioManager.requestAudioFocus(focusRequest) != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) return
        pausedByFocus = false
        p.resumePlayback()
        AudioHub.updatePlayer { it.copy(status = PlayerStatus.PLAYING) }
        updateNotification()
    }

    private fun stopEverything() {
        player?.requestStop()
        player = null
        abandonFocus()
        unregisterNoisy()
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun onFocusChange(change: Int) {
        when (change) {
            AudioManager.AUDIOFOCUS_LOSS -> pause()
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                if (player?.isPaused == false) {
                    pausedByFocus = true
                    pause()
                }
            }
            AudioManager.AUDIOFOCUS_GAIN -> if (pausedByFocus) resume()
            // AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK: od API 26 system ścisza sam.
        }
    }

    private fun abandonFocus() {
        runCatching { audioManager.abandonAudioFocusRequest(focusRequest) }
    }

    private fun registerNoisy() {
        if (noisyRegistered) return
        ContextCompat.registerReceiver(
            this, noisyReceiver, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        noisyRegistered = true
    }

    private fun unregisterNoisy() {
        if (!noisyRegistered) return
        runCatching { unregisterReceiver(noisyReceiver) }
        noisyRegistered = false
    }

    // ------------------------------------------------------------------ PcmPlayer.Listener (wątek odtwarzacza)

    override fun onStarted(player: PcmPlayer, sampleRate: Int, channels: Int, durationMs: Long, processed: Boolean) {
        main.post {
            if (player !== this.player) return@post
            AudioHub.updatePlayer {
                it.copy(
                    status = if (player.isPaused) PlayerStatus.PAUSED else PlayerStatus.PLAYING,
                    sampleRate = sampleRate, channels = channels, durationMs = durationMs,
                    processed = processed, error = null,
                )
            }
            updateNotification()
        }
    }

    override fun onProgress(player: PcmPlayer, positionMs: Long) {
        if (player !== this.player) return
        AudioHub.updatePlayer { it.copy(positionMs = positionMs) }
    }

    override fun onCompleted(player: PcmPlayer) {
        main.post {
            if (player !== this.player) return@post
            AudioHub.updatePlayer { PlayerState(title = it.title) }
            stopEverything()
        }
    }

    override fun onError(player: PcmPlayer, message: String) {
        main.post {
            if (player !== this.player) return@post
            AudioHub.updatePlayer { it.copy(status = PlayerStatus.ERROR, error = message) }
            stopEverything()
        }
    }

    // ------------------------------------------------------------------ powiadomienie

    private fun startInForeground(title: String, text: String, playing: Boolean) {
        val n = buildNotification(title, text, playing)
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK else 0
        ServiceCompat.startForeground(this, NOTIFICATION_ID, n, type)
    }

    private fun updateNotification() {
        val st = AudioHub.player.value
        val text = st.title ?: "Plik audio"
        val playing = st.status == PlayerStatus.PLAYING || st.status == PlayerStatus.PREPARING
        val nm = getSystemService(android.app.NotificationManager::class.java)
        nm.notify(NOTIFICATION_ID, buildNotification(getString(R.string.notif_player_title), text, playing))
    }

    private fun buildNotification(title: String, text: String, playing: Boolean): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val toggle = if (playing) {
            NotificationCompat.Action(0, getString(R.string.action_pause), servicePending(ACTION_PAUSE, 1))
        } else {
            NotificationCompat.Action(0, getString(R.string.action_resume), servicePending(ACTION_RESUME, 2))
        }
        return NotificationCompat.Builder(this, SzkloApp.CHANNEL_PLAYER)
            .setSmallIcon(R.drawable.ic_stat_szklo)
            .setContentTitle(title)
            .setContentText(text)
            .setContentIntent(open)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .addAction(toggle)
            .addAction(NotificationCompat.Action(0, getString(R.string.action_stop), servicePending(ACTION_STOP, 3)))
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .build()
    }

    private fun servicePending(action: String, requestCode: Int): PendingIntent =
        PendingIntent.getService(
            this, requestCode, Intent(this, LocalPlayerService::class.java).setAction(action),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    private fun displayName(uri: Uri): String {
        val name = runCatching {
            contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull()
        return name ?: uri.lastPathSegment ?: "Plik audio"
    }

    companion object {
        const val ACTION_PLAY = "pl.szklo.action.PLAY"
        const val ACTION_PAUSE = "pl.szklo.action.PAUSE"
        const val ACTION_RESUME = "pl.szklo.action.RESUME"
        const val ACTION_STOP = "pl.szklo.action.STOP"
        private const val NOTIFICATION_ID = 101

        /** Start odtwarzania pliku wybranego przez użytkownika (wołać z UI na pierwszym planie). */
        fun play(context: Context, uri: Uri) {
            val i = Intent(context, LocalPlayerService::class.java)
                .setAction(ACTION_PLAY)
                .setData(uri)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            ContextCompat.startForegroundService(context, i)
        }

        /** Pauza / wznowienie / stop działającej usługi. */
        fun command(context: Context, action: String) {
            if (AudioHub.player.value.status == PlayerStatus.IDLE && action != ACTION_STOP) return
            runCatching { context.startService(Intent(context, LocalPlayerService::class.java).setAction(action)) }
        }
    }
}
