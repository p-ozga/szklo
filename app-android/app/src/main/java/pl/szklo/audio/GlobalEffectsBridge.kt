package pl.szklo.audio

import android.media.audiofx.DynamicsProcessing
import android.media.audiofx.Virtualizer
import android.os.Build
import android.util.Log
import androidx.annotation.MainThread
import androidx.annotation.RequiresApi
import pl.szklo.model.EngineParams

/**
 * Most efektów dla INNYCH aplikacji (Spotify, YouTube Music, …).
 *
 * Mechanizm: odtwarzacz rozgłasza `AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION` z numerem swojej
 * sesji audio; my tworzymy na tej sesji systemowy efekt `DynamicsProcessing` (API 28+) i ustawiamy jego
 * etapy (preEQ/MBC/postEQ/limiter) według [DpMapping] – przybliżenia naszych 23 modułów.
 *
 * WAŻNE: własny DSP Szkła (Rust) NIE może przetwarzać strumienia innej aplikacji bez roota.
 * Android nie udostępnia „wtyczek” w torze cudzej sesji – można jedynie włączyć wbudowane efekty
 * systemowe. Pełna jakość rdzenia jest dostępna tylko w [LocalPlayerService].
 *
 * Sesja 0 (globalny mikser, przestarzała) – używana jako ZAPAS, gdy żaden odtwarzacz nie ogłosił sesji
 * (np. Spotify grał już przed startem Szkła). Gdy pojawi się sesja odtwarzacza, efekt na sesji 0 jest zwalniany
 * (inaczej dźwięk byłby przetwarzany dwa razy). Zmiana 2026-11-22: na Xperii 10 VI w Spotify nie było różnicy.
 *
 * Wszystkie metody – wątek główny.
 */
@RequiresApi(Build.VERSION_CODES.P)
class GlobalEffectsBridge {

    private val effects = HashMap<Int, DynamicsProcessing>()
    /** Systemowy wirtualizer (Przestrzeń/Szerokość) – tworzony dopiero, gdy potrzebny; nie każde urządzenie go ma. */
    private val virtualizers = HashMap<Int, Virtualizer>()
    private val virtualizerFailed = HashSet<Int>()
    private var lastParams: EngineParams? = null
    private var lastTargets: DpMapping.Targets? = null

    val attachedSessions: Set<Int> get() = effects.keys.toSet()

    /** Dopasowuje zestaw podpiętych sesji do [sessions]. Zwraca opis błędu ostatniej nieudanej próby (lub null). */
    @MainThread
    fun syncSessions(sessions: Set<Int>, allowGlobal: Boolean = true): String? {
        var error: String? = null
        // brak sesji odtwarzaczy → efekt globalny na sesji 0 (zapas); wyłączony, gdy gra odbiornik Spotify Connect
        // Szkła (jego dźwięk jest już przetworzony pełnym silnikiem – efekt globalny przetworzyłby go drugi raz)
        val target = if (sessions.isEmpty()) (if (allowGlobal) setOf(GLOBAL_SESSION) else emptySet()) else sessions
        (effects.keys - target).forEach { detach(it) }
        (target - effects.keys).forEach { id -> attach(id)?.let { error = it } }
        return error
    }

    @MainThread
    fun apply(params: EngineParams) {
        lastParams = params
        val t = DpMapping.compute(params)
        if (t == lastTargets) return
        lastTargets = t
        val broken = ArrayList<Int>()
        for ((id, dp) in effects) {
            if (!applyTo(dp, t)) broken += id else applyVirtualizer(id, t)
        }
        // sesja, na której efekt przestał działać (odtwarzacz zamknięty bez zgłoszenia), wypada z listy –
        // inaczej wisiałaby do końca i blokowała powrót do sesji ogólnej
        broken.forEach {
            detach(it)
            if (it != GLOBAL_SESSION) AudioHub.removeSession(it)
        }
    }

    @MainThread
    fun release() {
        effects.keys.toList().forEach { detach(it) }
    }

    private fun attach(sessionId: Int): String? {
        if (sessionId < 0) return null
        return try {
            val dp = DynamicsProcessing(PRIORITY, sessionId, buildConfig())
            effects[sessionId] = dp
            lastParams?.let { p -> DpMapping.compute(p).let { applyTo(dp, it); applyVirtualizer(sessionId, it) } }
            Log.i(TAG, "Podpięto DynamicsProcessing do sesji $sessionId (kontrola: ${dp.hasControl()})")
            null
        } catch (e: RuntimeException) {
            // UnsupportedOperationException / IllegalArgumentException / IllegalStateException:
            // efekt niedostępny na urządzeniu, sesja już nie istnieje albo brak uprawnień.
            Log.w(TAG, "Nie można podpiąć efektu do sesji $sessionId", e)
            "Nie można podpiąć efektu do sesji $sessionId: ${e.message ?: e.javaClass.simpleName}"
        }
    }

    /** Błąd wirtualizera nie psuje reszty efektów – zapis w dzienniku i dalej bez niego. */
    private fun applyVirtualizer(id: Int, t: DpMapping.Targets) {
        val strength = if (t.enabled) t.virtualizer else 0
        if (id in virtualizerFailed || (strength == 0 && id !in virtualizers)) return
        try {
            val v = virtualizers.getOrPut(id) {
                Virtualizer(PRIORITY, id).also { Log.i(TAG, "Wirtualizer na sesji $id (regulacja siły: ${it.strengthSupported})") }
            }
            if (strength > 0) v.setStrength(strength.toShort())
            v.setEnabled(strength > 0)
        } catch (e: RuntimeException) {
            Log.w(TAG, "Wirtualizer niedostępny na sesji $id", e)
            virtualizerFailed += id
            virtualizers.remove(id)?.let { runCatching { it.release() } }
        }
    }

    private fun detach(sessionId: Int) {
        virtualizerFailed -= sessionId
        virtualizers.remove(sessionId)?.let { v ->
            runCatching { v.setEnabled(false) }
            runCatching { v.release() }
        }
        effects.remove(sessionId)?.let { dp ->
            runCatching { dp.setEnabled(false) }
            runCatching { dp.release() }
        }
    }

    private fun buildConfig(): DynamicsProcessing.Config {
        val pre = DpMapping.PRE_EQ_CUTOFFS
        val mbcCut = DpMapping.MBC_CUTOFFS
        val post = DpMapping.POST_EQ_CUTOFFS
        val preEq = DynamicsProcessing.Eq(true, true, pre.size).apply {
            pre.forEachIndexed { i, f -> setBand(i, DynamicsProcessing.EqBand(true, f, 0f)) }
        }
        val mbc = DynamicsProcessing.Mbc(true, true, mbcCut.size).apply {
            mbcCut.forEachIndexed { i, f ->
                setBand(i, DynamicsProcessing.MbcBand(true, f, 3f, 80f, 1f, 0f, 6f, -90f, 1f, 0f, 0f))
            }
        }
        val postEq = DynamicsProcessing.Eq(true, true, post.size).apply {
            post.forEachIndexed { i, f -> setBand(i, DynamicsProcessing.EqBand(true, f, 0f)) }
        }
        val limiter = DynamicsProcessing.Limiter(true, true, 0, 1f, 60f, 10f, -1.3f, 0f)
        return DynamicsProcessing.Config.Builder(
            DynamicsProcessing.VARIANT_FAVOR_FREQUENCY_RESOLUTION,
            CHANNELS,
            true, pre.size,
            true, mbcCut.size,
            true, post.size,
            true,
        )
            .setPreferredFrameDuration(10f)
            .setPreEqAllChannelsTo(preEq)
            .setMbcAllChannelsTo(mbc)
            .setPostEqAllChannelsTo(postEq)
            .setLimiterAllChannelsTo(limiter)
            .build()
    }

    /** Zwraca false, gdy efekt utracił ważność (np. sesja zamknięta) – wtedy go zwalniamy. */
    private fun applyTo(dp: DynamicsProcessing, t: DpMapping.Targets): Boolean = try {
        dp.setInputGainAllChannelsTo(t.inputGainDb)
        t.preEq.forEachIndexed { i, g ->
            dp.setPreEqBandAllChannelsTo(i, DynamicsProcessing.EqBand(true, DpMapping.PRE_EQ_CUTOFFS[i], g))
        }
        t.mbc.forEachIndexed { i, m ->
            dp.setMbcBandAllChannelsTo(
                i,
                DynamicsProcessing.MbcBand(
                    true, DpMapping.MBC_CUTOFFS[i], m.attackMs, m.releaseMs, m.ratio, m.thresholdDb,
                    m.kneeDb, m.noiseGateDb, m.expanderRatio, m.preGainDb, m.postGainDb,
                ),
            )
        }
        t.postEq.forEachIndexed { i, g ->
            dp.setPostEqBandAllChannelsTo(i, DynamicsProcessing.EqBand(true, DpMapping.POST_EQ_CUTOFFS[i], g))
        }
        val l = t.limiter
        dp.setLimiterAllChannelsTo(
            DynamicsProcessing.Limiter(true, l.enabled, 0, l.attackMs, l.releaseMs, l.ratio, l.thresholdDb, l.postGainDb),
        )
        dp.setEnabled(t.enabled) // setEnabled zwraca kod błędu (int), więc bez składni właściwości
        true
    } catch (e: RuntimeException) {
        Log.w(TAG, "Nie można ustawić DynamicsProcessing", e)
        false
    }

    private companion object {
        const val TAG = "GlobalEffectsBridge"
        /** Priorytet efektu na sesji – wysoki, żeby nie oddać kontroli efektom producenta (Xperia) ani innym aplikacjom. */
        const val PRIORITY = Int.MAX_VALUE
        const val GLOBAL_SESSION = 0
        const val CHANNELS = 2
    }
}
