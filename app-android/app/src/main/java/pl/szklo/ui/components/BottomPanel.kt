package pl.szklo.ui.components

import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.Shader
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.RenderEffect
import androidx.compose.ui.graphics.asComposeRenderEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import pl.szklo.model.ModuleGroup
import pl.szklo.model.ModuleId
import pl.szklo.model.ModuleState
import pl.szklo.ui.theme.SzkloColors
import pl.szklo.ui.theme.SzkloDims
import pl.szklo.ui.theme.SzkloType
import kotlin.math.abs
import kotlin.math.roundToInt

/**
 * Stan wysuwania panelu dolnego: [extraPx] = ile ponad wysokość zwiniętą (356 dp).
 * Trzymany nad „kartą” (przetrwa obrót okna). Zmiana wysokości czytana jest wyłącznie w fazie układu
 * (Modifier.layout) – przeciąganie nie powoduje rekompozycji.
 */
@Stable
class PanelState {
    var extraPx by mutableFloatStateOf(0f)
        private set
    var maxExtraPx by mutableFloatStateOf(0f)
        internal set
    var isSettling by mutableStateOf(false)
        private set

    val isExpanded: Boolean get() = maxExtraPx > 0f && extraPx >= maxExtraPx - 0.5f

    /** Przesuwa panel o [deltaPx] (dodatnie = w górę). Zwraca faktycznie zużytą część. */
    fun dragBy(deltaPx: Float): Float {
        val old = extraPx
        extraPx = (old + deltaPx).coerceIn(0f, maxExtraPx)
        return extraPx - old
    }

    internal fun clamp() {
        if (extraPx > maxExtraPx) extraPx = maxExtraPx
    }

    /** Dociąga do najbliższego stanu (z uwzględnieniem prędkości; [velocityUp] dodatnia = w górę). */
    suspend fun settle(velocityUp: Float) {
        val target = when {
            velocityUp > FLING_THRESHOLD -> maxExtraPx
            velocityUp < -FLING_THRESHOLD -> 0f
            extraPx > maxExtraPx / 2f -> maxExtraPx
            else -> 0f
        }
        if (abs(target - extraPx) < 0.5f) {
            extraPx = target
            return
        }
        isSettling = true
        try {
            animate(extraPx, target, velocityUp, spring(dampingRatio = 1f, stiffness = 500f)) { v, _ ->
                extraPx = v.coerceIn(0f, maxExtraPx)
            }
        } finally {
            isSettling = false
        }
    }

    suspend fun collapse() = settle(-FLING_THRESHOLD * 2)

    private companion object {
        const val FLING_THRESHOLD = 1200f // px/s
    }
}

/**
 * Panel dolny (12.4): wys. 356 (zwinięty), promień górny 28, padding 12/20/20, gap 14,
 * tło rgba(14,26,28,.50) nad rozmytym (36, nasycenie 150%) tłem, górna krawędź 1 dp biel 18%.
 *
 * Rozmycie „tła pod panelem” (API 31+): [backdrop] to warstwa graficzna z nagraną zawartością ekranu
 * pod panelem (tło + pokrętło + chipy). Rysujemy ją ponownie wewnątrz panelu, przesuniętą o pozycję panelu,
 * w warstwie z RenderEffect (blur + nasycenie). Bez rozmywania pulpitu i bez kopii bitmap.
 * Na API < 31 (albo backdrop == null) – sam półprzezroczysty kolor .62.
 */
@Composable
fun BottomPanel(
    group: ModuleGroup,
    modules: () -> List<ModuleState>,
    onToggle: (Int, Boolean) -> Unit,
    onAmount: (Int, Float) -> Unit,
    panelState: PanelState,
    backdrop: GraphicsLayer?,
    rootHeightPx: Float,
    topInsetPx: Int,
    bottomInsetPx: Int,
    modifier: Modifier = Modifier,
) {
    val density = LocalDensity.current
    val collapsedPx = with(density) { SzkloDims.PanelHeight.toPx() } + bottomInsetPx
    val maxExtra = (rootHeightPx - topInsetPx - collapsedPx).coerceAtLeast(0f)
    SideEffect {
        panelState.maxExtraPx = maxExtra
        panelState.clamp()
    }

    val shape = remember { RoundedCornerShape(topStart = SzkloDims.PanelRadius, topEnd = SzkloDims.PanelRadius) }
    val blurEffect = remember(density) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) panelBackdropEffect(with(density) { SzkloDims.PanelBlur.toPx() }) else null
    }
    val useBlur = backdrop != null && blurEffect != null
    val tint = SzkloColors.PanelTint.copy(
        alpha = if (useBlur) SzkloColors.PANEL_TINT_ALPHA_BLUR else SzkloColors.PANEL_TINT_ALPHA_FALLBACK,
    )
    val edge = remember { SzkloColors.white(0.18f) }
    val edgePath = remember { Path() }

    val nested = remember(panelState) { PanelNestedScroll(panelState) }
    val dragState = rememberDraggableState { delta -> panelState.dragBy(-delta) }

    Box(
        modifier = modifier
            .fillMaxWidth()
            .layout { measurable, constraints ->
                // Wysokość czytana w fazie układu → przeciąganie = tylko relayout, bez rekompozycji.
                val h = (collapsedPx + panelState.extraPx).roundToInt().coerceAtMost(constraints.maxHeight)
                val placeable = measurable.measure(Constraints.fixed(constraints.maxWidth, h))
                layout(placeable.width, h) { placeable.place(0, 0) }
            }
            .nestedScroll(nested)
            .draggable(
                state = dragState,
                orientation = Orientation.Vertical,
                onDragStopped = { velocity -> panelState.settle(-velocity) },
            ),
    ) {
        if (useBlur) {
            Box(
                Modifier
                    .matchParentSize()
                    .graphicsLayer {
                        renderEffect = blurEffect
                        this.shape = shape
                        clip = true
                    }
                    .drawBehind {
                        // Panel jest wyrównany do dołu ekranu: jego górna krawędź = rootHeight − wysokość panelu.
                        translate(top = -(rootHeightPx - size.height)) { drawLayer(backdrop!!) }
                    },
            )
        }
        Box(
            Modifier
                .matchParentSize()
                .clip(shape)
                .background(tint)
                .drawBehind {
                    // border-top 1 px na zaokrąglonym rogu (CSS): linia wzdłuż górnej krawędzi i łuków.
                    val r = SzkloDims.PanelRadius.toPx()
                    val half = 0.5.dp.toPx()
                    edgePath.reset()
                    edgePath.moveTo(half, r)
                    edgePath.arcTo(Rect(half, half, 2 * r, 2 * r), 180f, 90f, false)
                    edgePath.lineTo(size.width - r, half)
                    edgePath.arcTo(Rect(size.width - 2 * r, half, size.width - half, 2 * r), 270f, 90f, false)
                    drawPath(edgePath, edge, style = Stroke(width = 1.dp.toPx()))
                },
        )

        Column(
            Modifier
                .fillMaxSize()
                .padding(top = SzkloDims.PanelPadTop, start = SzkloDims.PanelPadSide, end = SzkloDims.PanelPadSide),
        ) {
            // Uchwyt 40×4, promień 2, biel 30%.
            Box(
                Modifier
                    .align(Alignment.CenterHorizontally)
                    .size(width = 40.dp, height = 4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(SzkloColors.white(0.30f)),
            )
            Spacer(Modifier.height(SzkloDims.PanelGap))
            Crossfade(targetState = group, animationSpec = tween(150), label = "grupa") { g ->
                GroupContent(
                    group = g,
                    modules = modules,
                    onToggle = onToggle,
                    onAmount = onAmount,
                    bottomPadding = SzkloDims.PanelPadBottom + with(density) { bottomInsetPx.toDp() },
                )
            }
        }
    }
}

@Composable
private fun GroupContent(
    group: ModuleGroup,
    modules: () -> List<ModuleState>,
    onToggle: (Int, Boolean) -> Unit,
    onAmount: (Int, Float) -> Unit,
    bottomPadding: androidx.compose.ui.unit.Dp,
) {
    val ids = ModuleId.byGroup[group].orEmpty()
    val states = modules()
    val onCount = ids.count { states[it.index].on }
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(SzkloDims.PanelGap),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                group.label,
                style = SzkloType.PanelTitle,
                modifier = Modifier.alignByBaseline().semantics { heading() },
            )
            Text("$onCount z ${ids.size} włączone", style = SzkloType.PanelCount, modifier = Modifier.alignByBaseline())
        }
        for (id in ids) {
            val idx = id.index
            ModuleRow(
                module = id,
                state = states[idx],
                onToggle = { on -> onToggle(idx, on) },
                onAmount = { a -> onAmount(idx, a) },
            )
        }
        Spacer(Modifier.height(bottomPadding - SzkloDims.PanelGap))
    }
}

/**
 * Przewijanie listy i wysuwanie panelu jako jeden gest: palec w górę najpierw rozwija panel,
 * palec w dół na początku listy go zwija; puszczenie dociąga do stanu.
 */
private class PanelNestedScroll(private val state: PanelState) : NestedScrollConnection {
    override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
        if (available.y < 0f && !state.isExpanded) {
            return Offset(0f, -state.dragBy(-available.y))
        }
        return Offset.Zero
    }

    override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
        if (available.y > 0f && state.extraPx > 0f && !state.isSettling) {
            return Offset(0f, -state.dragBy(-available.y))
        }
        return Offset.Zero
    }

    override suspend fun onPreFling(available: Velocity): Velocity {
        val e = state.extraPx
        if (e > 0f && e < state.maxExtraPx) {
            state.settle(-available.y)
            return available
        }
        return Velocity.Zero
    }
}

/** Rozmycie + nasycenie 150% (CSS: backdrop-filter: blur(36px) saturate(150%)). */
@RequiresApi(Build.VERSION_CODES.S)
private fun panelBackdropEffect(blurSigmaPx: Float): RenderEffect {
    // CSS blur(σ) to odchylenie standardowe; RenderEffect przyjmuje „promień” przeliczany przez
    // Skię/hwui jako σ = 0,57735·r + 0,5 → r = (σ − 0,5) / 0,57735.
    val radius = ((blurSigmaPx - 0.5f) / 0.57735f).coerceAtLeast(1f)
    val blur = android.graphics.RenderEffect.createBlurEffect(radius, radius, Shader.TileMode.CLAMP)
    val saturate = ColorMatrix().apply { setSaturation(1.5f) }
    return android.graphics.RenderEffect
        .createColorFilterEffect(ColorMatrixColorFilter(saturate), blur)
        .asComposeRenderEffect()
}
