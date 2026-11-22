package pl.szklo

import android.content.Intent
import android.graphics.Color
import android.media.audiofx.AudioEffect
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import pl.szklo.audio.AudioHub
import pl.szklo.audio.GlobalEffectsService
import pl.szklo.audio.SpotifyConnectService
import pl.szklo.ui.SzkloRoot
import pl.szklo.ui.SzkloViewModel
import pl.szklo.ui.theme.SzkloTheme

class MainActivity : ComponentActivity() {

    private val vm: SzkloViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Edge-to-edge: treść pod paskami systemowymi, jasne ikony (aplikacja zawsze ciemna).
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        handleEffectPanelIntent(intent)

        setContent {
            SzkloTheme {
                SzkloRoot(vm)
            }
        }
    }

    override fun onStart() {
        super.onStart()
        // Usługa trybu „inne aplikacje” startuje tylko z pierwszego planu (Android 12+).
        val g = AudioHub.global.value
        if (g.requested && !g.running) GlobalEffectsService.start(this)
        // Spotify Connect wraca sam, jeśli był włączony przy zamknięciu aplikacji.
        if (AudioHub.spotifyWanted && !AudioHub.spotify.value.running) SpotifyConnectService.start(this)
    }

    override fun onStop() {
        // Ustawienia na dysk od razu – zamknięcie aplikacji tuż po zmianie suwaka niczego nie gubi.
        AudioHub.flush()
        super.onStop()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleEffectPanelIntent(intent)
    }

    /**
     * Przycisk „Korektor” w odtwarzaczu (np. Spotify) wysyła ACTION_DISPLAY_AUDIO_EFFECT_CONTROL_PANEL
     * z numerem sesji. To wyraźna prośba użytkownika o efekt, więc zapamiętujemy sesję i włączamy tryb
     * „inne aplikacje” (przybliżenie przez DynamicsProcessing).
     */
    private fun handleEffectPanelIntent(intent: Intent?) {
        if (intent == null || intent.action != AudioEffect.ACTION_DISPLAY_AUDIO_EFFECT_CONTROL_PANEL) return
        val session = intent.getIntExtra(AudioEffect.EXTRA_AUDIO_SESSION, -1)
        if (session > 0) {
            AudioHub.addSession(session, intent.getStringExtra(AudioEffect.EXTRA_PACKAGE_NAME) ?: callingPackage)
            vm.setGlobalEffects(true)
        }
    }
}
