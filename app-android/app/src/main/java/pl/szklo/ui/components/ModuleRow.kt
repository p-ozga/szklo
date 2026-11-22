package pl.szklo.ui.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import pl.szklo.model.ModuleId
import pl.szklo.model.ModuleState
import pl.szklo.ui.Fmt
import pl.szklo.ui.theme.SzkloColors
import pl.szklo.ui.theme.SzkloDims
import pl.szklo.ui.theme.SzkloType
import kotlin.math.roundToInt

/**
 * Wiersz modułu (12.4): min. wys. 40, gap 12 – [pole wyboru 24] [nazwa 96] [suwak ↔] [wartość 34].
 * Pole wyboru i nazwa tworzą jeden cel dotyku (większy niż 24 dp, bez zmiany wyglądu).
 */
@Composable
fun ModuleRow(
    module: ModuleId,
    state: ModuleState,
    onToggle: (Boolean) -> Unit,
    onAmount: (Float) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = SzkloDims.RowMinHeight),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(SzkloDims.RowGap),
    ) {
        Row(
            modifier = Modifier
                .heightIn(min = SzkloDims.RowMinHeight)
                .clip(RoundedCornerShape(8.dp))
                .toggleable(value = state.on, role = Role.Checkbox, onValueChange = onToggle)
                // Etykieta jak aria-label makiety; rola i stan zaznaczenia pochodzą z toggleable.
                .clearAndSetSemantics { contentDescription = "${module.label} włącz/wyłącz" },
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(SzkloDims.RowGap),
        ) {
            ModuleCheckbox(checked = state.on)
            // Bez wielokropka: jak w makiecie dłuższa nazwa łamie się na dwie linie w 96 dp.
            Text(module.label, style = SzkloType.ModuleName, modifier = Modifier.width(SzkloDims.NameWidth))
        }
        ModuleSlider(
            value = state.amount,
            enabled = state.on,
            onValueChange = onAmount,
            label = "${module.label} moc",
            modifier = Modifier.weight(1f),
        )
        Text(
            text = if (state.on) Fmt.percent(state.amount) else "–",
            style = SzkloType.ModuleValue,
            textAlign = TextAlign.End,
            maxLines = 1,
            modifier = Modifier.width(SzkloDims.ValueWidth),
        )
    }
}

/**
 * Pole wyboru 24×24, promień 7. Włączone: #1EC97B + ptaszek (M5 12l5 5L19 7, obrys #081113 3, 14×14).
 * Wyłączone: obrys 1,5 biel 35%, przezroczyste.
 */
@Composable
fun ModuleCheckbox(checked: Boolean, modifier: Modifier = Modifier) {
    val shape = RoundedCornerShape(SzkloDims.CheckRadius)
    val bg by animateColorAsState(
        targetValue = if (checked) SzkloColors.Accent else Color.Transparent,
        animationSpec = tween(120),
        label = "pole",
    )
    val offBorder = remember { SzkloColors.white(0.35f) }
    val checkPath = remember { Path() }
    Box(
        modifier
            .size(SzkloDims.CheckSize)
            .clip(shape)
            .background(bg)
            .then(if (checked) Modifier else Modifier.border(1.5.dp, offBorder, shape))
            .drawBehind {
                if (!checked) return@drawBehind
                // Ikona 14 dp wyśrodkowana; współrzędne viewBox 24 → skala 14/24.
                val s = 14.dp.toPx() / 24f
                val o = (size.width - 14.dp.toPx()) / 2f
                checkPath.reset()
                checkPath.moveTo(o + 5f * s, o + 12f * s)
                checkPath.lineTo(o + 10f * s, o + 17f * s)
                checkPath.lineTo(o + 19f * s, o + 7f * s)
                drawPath(
                    checkPath, SzkloColors.OnAccent,
                    style = Stroke(width = 3f * s, cap = StrokeCap.Round, join = StrokeJoin.Round),
                )
            },
    )
}

/**
 * Suwak własny (jak `input[type=range]` z makiety): tor 4 dp biel 16% na całej szerokości, bez wypełnienia;
 * gałka Ø20 #F4FFF9 z obrysem 3 #1EC97B (wewnątrz średnicy, jak `box-sizing: border-box` w Chrome);
 * środek gałki przesuwa się od 10 dp do szer.−10 dp. Nieaktywny: gałka #56656A / #3B494D.
 * Wysokość 40 dp = cel dotyku (wiersz i tak ma min. 40). Krok 1%.
 */
@Composable
fun ModuleSlider(
    value: Float,
    enabled: Boolean,
    onValueChange: (Float) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
) {
    val latestOnChange by rememberUpdatedState(onValueChange)
    val trackColor = remember { SzkloColors.white(0.16f) }
    val v = value.coerceIn(0f, 1f)
    val percent = (v * 100f).roundToInt()

    fun fractionAt(x: Float, width: Float, thumbR: Float): Float {
        val usable = (width - 2 * thumbR).coerceAtLeast(1f)
        return (((x - thumbR) / usable).coerceIn(0f, 1f) * 100f).roundToInt() / 100f
    }

    Box(
        modifier
            .height(SzkloDims.RowMinHeight)
            .clearAndSetSemantics {
                contentDescription = label
                stateDescription = if (enabled) "$percent%" else "wyłączony"
                progressBarRangeInfo = ProgressBarRangeInfo(v, 0f..1f, steps = 99)
                if (enabled) {
                    setProgress { t ->
                        latestOnChange(((t.coerceIn(0f, 1f)) * 100f).roundToInt() / 100f)
                        true
                    }
                } else {
                    disabled()
                }
            }
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                val r = SzkloDims.ThumbDiameter.toPx() / 2f
                detectTapGestures(onTap = { pos -> latestOnChange(fractionAt(pos.x, size.width.toFloat(), r)) })
            }
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                val r = SzkloDims.ThumbDiameter.toPx() / 2f
                // Poziomy próg dotyku: pionowe ruchy zostają dla przewijania panelu.
                detectHorizontalDragGestures(
                    onDragStart = { pos -> latestOnChange(fractionAt(pos.x, size.width.toFloat(), r)) },
                    onHorizontalDrag = { change, _ ->
                        change.consume()
                        latestOnChange(fractionAt(change.position.x, size.width.toFloat(), r))
                    },
                )
            }
            .drawBehind {
                val trackH = SzkloDims.TrackHeight.toPx()
                val cy = size.height / 2f
                drawRoundRect(
                    color = trackColor,
                    topLeft = Offset(0f, cy - trackH / 2f),
                    size = Size(size.width, trackH),
                    cornerRadius = CornerRadius(trackH / 2f),
                )
                val r = SzkloDims.ThumbDiameter.toPx() / 2f
                val cx = r + v * (size.width - 2 * r)
                val border = SzkloDims.ThumbBorder.toPx()
                val c = Offset(cx, cy)
                drawCircle(if (enabled) SzkloColors.Accent else SzkloColors.ThumbBorderDisabled, r, c)
                drawCircle(if (enabled) SzkloColors.ThumbFill else SzkloColors.ThumbFillDisabled, r - border, c)
            },
    )
}
