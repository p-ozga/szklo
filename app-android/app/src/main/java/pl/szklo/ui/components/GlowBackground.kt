package pl.szklo.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import pl.szklo.ui.theme.SzkloBlobs
import pl.szklo.ui.theme.SzkloColors
import kotlin.math.exp

/**
 * Tło ekranu: kolor #081113 + 3 plamy jako `Brush.radialGradient` – bez rozmywania w czasie rzeczywistym.
 *
 * Makieta rysuje plamy jako koła z CSS `filter: blur(σ)`. Tu profil koła rozmytego gaussem liczymy raz
 * (całka koła z jądrem Gaussa 2D, z funkcją Bessela I0) i zapisujemy jako przystanki gradientu.
 * Zgodnie ze scenariuszem gradient kończy się na promieniu r + blur; żeby nie było „schodka”, ogon
 * profilu jest liniowo ściągany do zera na tym promieniu. (Dokładny profil gaussowski sięga ~r + 3σ –
 * jeśli porównanie SSIM z eksportem makiety wykaże różnicę na brzegach plam, zwiększyć [TAIL_FACTOR].)
 *
 * Pędzle tworzone są tylko przy zmianie rozmiaru/wcięć (drawWithCache) – zero alokacji na klatkę.
 *
 * @param topInsetPx wysokość paska stanu: makieta nie rysuje paska, plama 1 jest liczona od jego dolnej krawędzi.
 * @param bottomInsetPx wysokość paska nawigacji: plamy 2 i 3 są kotwiczone do dołu makiety (nad paskiem).
 */
@Composable
fun GlowBackground(
    modifier: Modifier = Modifier,
    topInsetPx: Int = 0,
    bottomInsetPx: Int = 0,
) {
    val p1 = remember { blobStops(SzkloBlobs.B1) }
    val p2 = remember { blobStops(SzkloBlobs.B2) }
    val p3 = remember { blobStops(SzkloBlobs.B3) }
    Box(
        modifier.drawWithCache {
            val d = density
            val w = size.width
            val bottom = size.height - bottomInsetPx

            val r1 = SzkloBlobs.B1.diameter / 2f
            val c1 = Offset(w / 2f, topInsetPx + (SzkloBlobs.B1_TOP + r1) * d)
            val r2 = SzkloBlobs.B2.diameter / 2f
            val c2 = Offset((SzkloBlobs.B2_LEFT + r2) * d, bottom - (SzkloBlobs.B2_BOTTOM + r2) * d)
            val r3 = SzkloBlobs.B3.diameter / 2f
            val c3 = Offset(w - (SzkloBlobs.B3_RIGHT + r3) * d, bottom - (SzkloBlobs.B3_BOTTOM + r3) * d)

            val e1 = p1.extent * d
            val e2 = p2.extent * d
            val e3 = p3.extent * d
            val b1 = Brush.radialGradient(*p1.stops, center = c1, radius = e1)
            val b2 = Brush.radialGradient(*p2.stops, center = c2, radius = e2)
            val b3 = Brush.radialGradient(*p3.stops, center = c3, radius = e3)
            onDrawBehind {
                drawRect(SzkloColors.Background)
                // Kolejność jak w makiecie (późniejsza plama na wierzchu).
                drawCircle(b1, radius = e1, center = c1)
                drawCircle(b2, radius = e2, center = c2)
                drawCircle(b3, radius = e3, center = c3)
            }
        },
    )
}

/** Gotowe przystanki gradientu + promień końcowy w dp. */
internal class BlobGradient(val stops: Array<Pair<Float, Color>>, val extent: Float)

/** Promień końca gradientu = r + TAIL_FACTOR·blur (scenariusz: 1,0). */
internal const val TAIL_FACTOR = 1.0f
private const val STOP_COUNT = 24

internal fun blobStops(b: SzkloBlobs.Blob): BlobGradient {
    val r = b.diameter / 2.0
    val sigma = b.blur.toDouble()
    val end = r + TAIL_FACTOR * sigma
    val profile = blurredDiskProfile(r, sigma, end, STOP_COUNT)
    val stops = Array(STOP_COUNT) { k ->
        val t = k / (STOP_COUNT - 1f)
        t to b.color.copy(alpha = (b.alpha * profile[k]).coerceIn(0f, 1f))
    }
    return BlobGradient(stops, end.toFloat())
}

/**
 * Jasność koła o promieniu [r] rozmytego gaussem [sigma], próbkowana w [n] punktach na odcinku 0..[end],
 * z liniowym ściągnięciem ogona do 0 w [end]. Wartości 0..1.
 */
internal fun blurredDiskProfile(r: Double, sigma: Double, end: Double, n: Int): FloatArray {
    val s2 = sigma * sigma
    val iEnd = diskIntensity(end, r, s2)
    return FloatArray(n) { k ->
        val dist = end * k / (n - 1)
        (diskIntensity(dist, r, s2) - iEnd * dist / end).toFloat().coerceIn(0f, 1f)
    }
}

/**
 * I(d) = ∫₀ᴿ (ρ/σ²)·exp(−(d²+ρ²)/(2σ²))·I0(dρ/σ²) dρ – splot koła jednostkowego z gaussem 2D
 * w odległości d od środka (całka Simpsona). Dla d = 0: 1 − exp(−R²/2σ²).
 */
internal fun diskIntensity(d: Double, r: Double, s2: Double): Double {
    val steps = 128 // parzysta
    val h = r / steps
    var sum = 0.0
    for (i in 0..steps) {
        val rho = i * h
        val f = rho / s2 * exp(-(d * d + rho * rho) / (2 * s2)) * besselI0(d * rho / s2)
        val w = when {
            i == 0 || i == steps -> 1.0
            i % 2 == 1 -> 4.0
            else -> 2.0
        }
        sum += w * f
    }
    return sum * h / 3.0
}

/** Zmodyfikowana funkcja Bessela I0 (szereg potęgowy – wystarcza dla x ≤ ~20). */
internal fun besselI0(x: Double): Double {
    val q = x * x / 4.0
    var term = 1.0
    var sum = 1.0
    var k = 1
    while (k < 60) {
        term *= q / (k.toDouble() * k)
        sum += term
        if (term < 1e-14 * sum) break
        k++
    }
    return sum
}
