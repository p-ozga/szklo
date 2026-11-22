package pl.szklo.audio

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.audiofx.AudioEffect

/**
 * Odbiera rozgłoszenia odtwarzaczy o sesjach efektów:
 * - `ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION` – odtwarzacz zaczął sesję i zaprasza aplikacje-korektory,
 * - `ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION` – sesja się skończyła.
 *
 * Sam tylko zapisuje sesje w [AudioHub]; podpinaniem efektów zajmuje się [GlobalEffectsService].
 * Rejestrowany w manifeście (dla nadawców jawnych) i dynamicznie w usłudze (Android 8+ nie dostarcza
 * niejawnych rozgłoszeń do odbiorników z manifestu).
 *
 * Uwaga: odbiornik jest eksportowany (musi – nadawcą jest inna aplikacja), więc dowolna aplikacja może
 * zgłosić dowolny numer sesji. Skutek jest nieszkodliwy (próba podpięcia efektu do nieistniejącej sesji
 * kończy się błędem obsłużonym w [GlobalEffectsBridge]).
 */
class EffectSessionReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        val session = intent.getIntExtra(AudioEffect.EXTRA_AUDIO_SESSION, NO_SESSION)
        if (session <= 0) return // brak lub 0 = mikser globalny (przestarzałe)
        when (intent.action) {
            AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION ->
                AudioHub.addSession(session, intent.getStringExtra(AudioEffect.EXTRA_PACKAGE_NAME))
            AudioEffect.ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION ->
                AudioHub.removeSession(session)
        }
    }

    companion object {
        private const val NO_SESSION = -1

        fun filter() = IntentFilter().apply {
            addAction(AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION)
            addAction(AudioEffect.ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION)
        }
    }
}
