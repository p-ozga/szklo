package pl.szklo.ui.backside

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Kontrakt strony tylnej: sterowanie aplikacjami odtwarzającymi muzykę (przyszłość).
 *
 * Planowana implementacja: `MediaSessionManager.getActiveSessions()` + `MediaController`
 * (wymaga dostępu do powiadomień – NotificationListenerService – zgoda użytkownika w ustawieniach).
 * UI strony tylnej zależy wyłącznie od tego interfejsu, więc podmiana atrapy na prawdziwą
 * implementację nie wymaga zmian w widokach.
 */
interface PlayerControlsBackside {
    /** false = funkcja niedostępna (pokazujemy zaślepkę „wkrótce”). */
    val isAvailable: Boolean

    /** Bieżący stan aktywnego odtwarzacza (aplikacja, tytuł, wykonawca, czy gra). */
    val state: StateFlow<ExternalPlayerState>

    fun playPause()
    fun skipToNext()
    fun skipToPrevious()
}

data class ExternalPlayerState(
    val appLabel: String? = null,
    val title: String? = null,
    val artist: String? = null,
    val isPlaying: Boolean = false,
)

/** Atrapa na teraz – brak sterowania zewnętrznymi odtwarzaczami. */
object PlaceholderPlayerControls : PlayerControlsBackside {
    override val isAvailable: Boolean = false
    private val _state = MutableStateFlow(ExternalPlayerState())
    override val state: StateFlow<ExternalPlayerState> = _state.asStateFlow()
    override fun playPause() = Unit
    override fun skipToNext() = Unit
    override fun skipToPrevious() = Unit
}
