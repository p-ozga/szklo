package pl.szklo.ui.components

import android.view.HapticFeedbackConstants
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import pl.szklo.ui.theme.SzkloColors
import pl.szklo.ui.theme.SzkloDims
import pl.szklo.ui.theme.SzkloType
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.roundToInt
import kotlin.math.sin

/**
 * Pokrętło mocy ogólnej (sekcja 12.4, makieta: SVG 260×260).
 * - tor: łuk r 110, grubość 14, zaokrąglone końce, biel 12%, 270° od 135°,
 * - wartość: ten sam łuk w #1EC97B, długość 270° × moc,
 * - tarcza: koło r 86, biel 7%, obrys 1 dp biel 16%,
 * - środek: „68%” (Space Grotesk 52/600, lh 1) + „moc ogólna” (Inter 13 / 70%), odstęp 2.
 *
 * Gest: przeciąganie po okręgu (kąt → wartość, bez przeskoku przez lukę), haptyka co 5%,
 * podwójne stuknięcie = wartość z presetu. TalkBack: suwak 0–100% z akcjami ±5%.
 *
 * [value] przekazywane jako lambda: zmiana mocy przerysowuje tylko pokrętło, nie cały ekran.
 */
@Composable
fun PowerDial(
    value: () -> Float,
    onValueChange: (Float) -> Unit,
    onResetToPreset: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val view = LocalView.current
    val current = value().coerceIn(0f, 1f)
    val percent = (current * 100f).roundToInt()

    val latestValue by rememberUpdatedState(current)
    val latestOnChange by rememberUpdatedState(onValueChange)
    val latestOnReset by rememberUpdatedState(onResetToPreset)

    val trackColor = remember { SzkloColors.white(0.12f) }
    val faceFill = remember { SzkloColors.white(0.07f) }
    val faceStroke = remember { SzkloColors.white(0.16f) }

    fun emit(v: Float, previous: Float) {
        if (DialMath.tick(v) != DialMath.tick(previous)) {
            view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
        }
        latestOnChange(v)
    }

    Box(
        modifier = modifier
            .size(SzkloDims.DialSize)
            .clearAndSetSemantics {
                contentDescription = "Pokrętło mocy ogólnej"
                stateDescription = "$percent%"
                progressBarRangeInfo = ProgressBarRangeInfo(current, 0f..1f, steps = 19)
                setProgress { target ->
                    latestOnChange(target.coerceIn(0f, 1f))
                    true
                }
                customActions = listOf(
                    CustomAccessibilityAction("Zwiększ o 5%") {
                        latestOnChange((latestValue + 0.05f).coerceAtMost(1f)); true
                    },
                    CustomAccessibilityAction("Zmniejsz o 5%") {
                        latestOnChange((latestValue - 0.05f).coerceAtLeast(0f)); true
                    },
                )
            }
            .pointerInput(Unit) {
                detectTapGestures(onDoubleTap = { latestOnReset() })
            }
            .pointerInput(Unit) {
                // Środek pokrętła (promień < 24 dp) ignorujemy – kąt jest tam niestabilny.
                val dead = 24.dp.toPx()
                var v = 0f
                detectDragGestures(
                    onDragStart = { pos ->
                        val dx = pos.x - size.width / 2f
                        val dy = pos.y - size.height / 2f
                        v = latestValue
                        if (hypot(dx, dy) >= dead) {
                            val nv = DialMath.valueAtStart(DialMath.angleDeg(dx, dy))
                            emit(nv, v)
                            v = nv
                        }
                    },
                    onDrag = { change, _ ->
                        change.consume()
                        val dx = change.position.x - size.width / 2f
                        val dy = change.position.y - size.height / 2f
                        if (hypot(dx, dy) >= dead) {
                            val nv = DialMath.nextValue(v, DialMath.angleDeg(dx, dy))
                            if (nv != v) {
                                emit(nv, v)
                                v = nv
                            }
                        }
                    },
                )
            }
            .drawBehind {
                val stroke = SzkloDims.DialStroke.toPx()
                val r = SzkloDims.DialTrackRadius.toPx()
                val c = center
                val topLeft = Offset(c.x - r, c.y - r)
                val arc = Size(2 * r, 2 * r)
                val style = Stroke(width = stroke, cap = StrokeCap.Round)
                drawArc(trackColor, DialMath.START_DEG, DialMath.SWEEP_DEG, false, topLeft, arc, style = style)
                val v = value().coerceIn(0f, 1f)
                if (v > 0f) {
                    drawArc(SzkloColors.Accent, DialMath.START_DEG, DialMath.SWEEP_DEG * v, false, topLeft, arc, style = style)
                } else {
                    // SVG z zerową długością kreski i okrągłym zakończeniem rysuje kropkę – odtwarzamy to.
                    val a = Math.toRadians(DialMath.START_DEG.toDouble())
                    drawCircle(SzkloColors.Accent, stroke / 2f, Offset(c.x + r * cos(a).toFloat(), c.y + r * sin(a).toFloat()))
                }
                val face = SzkloDims.DialFaceRadius.toPx()
                drawCircle(faceFill, face, c)
                drawCircle(faceStroke, face, c, style = Stroke(width = 1.dp.toPx()))
            },
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Text("$percent%", style = SzkloType.DialValue)
            Text("moc ogólna", style = SzkloType.DialCaption)
        }
    }
}
