package pl.szklo.ui.components

import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.floor

/**
 * Geometria pokrętła: łuk 270° od kąta 135° (zgodnie z ruchem wskazówek od osi +X, oś Y w dół),
 * luka 90° na dole. Czyste funkcje – testy w src/test.
 */
object DialMath {
    const val START_DEG = 135f
    const val SWEEP_DEG = 270f

    /** Kąt punktu (dx, dy) względem środka, 0..360, zgodnie z ruchem wskazówek od +X (ekranowe Y w dół). */
    fun angleDeg(dx: Float, dy: Float): Float {
        val a = Math.toDegrees(atan2(dy.toDouble(), dx.toDouble())).toFloat()
        return (a + 360f) % 360f
    }

    /** Kąt względem początku łuku: 0..360 (0..270 = łuk, 270..360 = luka). */
    fun relativeDeg(angleDeg: Float): Float = ((angleDeg - START_DEG) % 360f + 360f) % 360f

    /** Wartość przy rozpoczęciu przeciągania (bez historii): w luce – bliższy koniec. */
    fun valueAtStart(angleDeg: Float): Float {
        val rel = relativeDeg(angleDeg)
        return when {
            rel <= SWEEP_DEG -> rel / SWEEP_DEG
            rel < SWEEP_DEG + (360f - SWEEP_DEG) / 2f -> 1f // prawa połowa luki → 100%
            else -> 0f
        }
    }

    /**
     * Następna wartość przy przeciąganiu – bez przeskoku przez lukę:
     * - palec w luce: trzymamy koniec, z którego przyszedł (wg poprzedniej wartości),
     * - palec przeszedł przez lukę na drugą stronę (skok > 50%): trzymamy koniec, aż wróci.
     */
    fun nextValue(previous: Float, angleDeg: Float): Float {
        val rel = relativeDeg(angleDeg)
        if (rel > SWEEP_DEG) return if (previous >= 0.5f) 1f else 0f
        val candidate = rel / SWEEP_DEG
        return if (abs(candidate - previous) > 0.5f) previous else candidate
    }

    /** Numer „ząbka” haptyki (co 5%). */
    fun tick(value: Float): Int = floor(value * 20f + 1e-4f).toInt()
}
