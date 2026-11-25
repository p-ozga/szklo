package pl.szklo

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import pl.szklo.audio.AudioHub

class SzkloApp : Application() {

    override fun onCreate() {
        super.onCreate()
        Dziennik.start(this)
        AudioHub.init(this)
        createChannels()
    }

    private fun createChannels() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_PLAYER, getString(R.string.channel_player), NotificationManager.IMPORTANCE_LOW).apply {
                description = getString(R.string.channel_player_desc)
                setShowBadge(false)
            },
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_GLOBAL, getString(R.string.channel_global), NotificationManager.IMPORTANCE_MIN).apply {
                description = getString(R.string.channel_global_desc)
                setShowBadge(false)
            },
        )
    }

    companion object {
        const val CHANNEL_PLAYER = "odtwarzacz"
        const val CHANNEL_GLOBAL = "inne_aplikacje"
    }
}
