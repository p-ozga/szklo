package pl.szklo.ui

import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToInt

/** Formatowanie liczb po polsku: przecinek dziesiętny, znak minus U+2212, „–” dla braku sygnału. */
object Fmt {
    private val PL: Locale = Locale.forLanguageTag("pl-PL")

    /** Poniżej tego poziomu traktujemy odczyt jako ciszę (rdzeń zwraca −120 dla braku sygnału). */
    private const val FLOOR_DB = -70f

    fun db(v: Float): String {
        if (!v.isFinite() || v <= FLOOR_DB) return "–"
        val x = if (abs(v) < 0.05f) 0f else v
        return String.format(PL, "%.1f", x).replace('-', '−')
    }

    fun percent(amount: Float): String = "${(amount * 100f).roundToInt()}%"

    /** „44,1 kHz”. */
    fun khz(hz: Int): String = String.format(PL, "%.1f kHz", hz / 1000f)

    fun time(ms: Long): String {
        if (ms < 0) return "–:––"
        val s = ms / 1000
        return String.format(Locale.ROOT, "%d:%02d", s / 60, s % 60)
    }
}
