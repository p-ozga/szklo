package pl.szklo.ui

import android.os.Build
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.coerceIn
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import pl.szklo.model.ModuleGroup
import pl.szklo.ui.components.BottomPanel
import pl.szklo.ui.components.GlowBackground
import pl.szklo.ui.components.GroupChips
import pl.szklo.ui.components.PanelState
import pl.szklo.ui.components.PowerDial
import pl.szklo.ui.components.PresetSelector
import pl.szklo.ui.theme.SzkloDims
import pl.szklo.ui.theme.SzkloType
import kotlin.math.abs

/**
 * Strona przednia – „Android 2 · Pokrętło mocy” 1:1 (sekcja 12.4):
 * kolumna górna (nagłówek, pokrętło, linia stanu, chipy) nad tłem z plamami + panel dolny.
 *
 * Wydajność: stany czytane są możliwie nisko – moc ogólna tylko w pokrętle, lista modułów tylko
 * w panelu (lambdy zamiast wartości), więc obrót pokrętła nie rekomponuje panelu i odwrotnie.
 */
@Composable
fun FrontScreen(
    vm: SzkloViewModel,
    panelState: PanelState,
    onFlip: () -> Unit,
    onGlobalEffects: (Boolean) -> Unit,
) {
    val modulesState = vm.modules.collectAsStateWithLifecycle()
    val masterState = vm.master.collectAsStateWithLifecycle()
    val group by vm.group.collectAsStateWithLifecycle()
    val presetKey by vm.presetKey.collectAsStateWithLifecycle()
    val userPresets by vm.userPresets.collectAsStateWithLifecycle()
    val enabled by vm.enabled.collectAsStateWithLifecycle()
    val global by vm.global.collectAsStateWithLifecycle()
    val statusState = vm.statusLine.collectAsStateWithLifecycle()

    val density = LocalDensity.current
    val topInsetPx = WindowInsets.statusBars.getTop(density)
    val bottomInsetPx = WindowInsets.navigationBars.getBottom(density)

    // Warstwa „tła pod panelem” do rozmycia (API 31+). Na starszych – brak (kolor zastępczy).
    val backdrop: GraphicsLayer? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) rememberGraphicsLayer() else null

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val rootHeightPx = constraints.maxHeight.toFloat()
        // miejsce na pokrętło: ekran minus paski, panel i reszta kolumny (260 dp, na niskich ekranach mniej)
        val insets = with(density) { (topInsetPx + bottomInsetPx).toDp() }
        val dialSize = (maxHeight - insets - SzkloDims.PanelHeight - SzkloDims.TopColumnWithoutDial)
            .coerceIn(SzkloDims.DialMinSize, SzkloDims.DialSize)

        Box(
            Modifier
                .fillMaxSize()
                .then(
                    if (backdrop != null) {
                        Modifier.drawWithContent {
                            backdrop.record { this@drawWithContent.drawContent() }
                            drawLayer(backdrop)
                        }
                    } else {
                        Modifier
                    },
                ),
        ) {
            GlowBackground(Modifier.fillMaxSize(), topInsetPx = topInsetPx, bottomInsetPx = bottomInsetPx)
            TopColumn(
                dialSize = dialSize,
                master = { masterState.value },
                onMaster = vm::setMaster,
                onMasterReset = vm::resetMasterToPreset,
                status = statusState,
                group = group,
                onGroup = vm::selectGroup,
                header = {
                    Header(
                        onFlip = onFlip,
                        preset = {
                            PresetSelector(
                                presetKey = presetKey,
                                userPresets = userPresets,
                                onPreset = vm::applyPreset,
                                enabled = enabled,
                                onEnabled = vm::setEnabled,
                                global = global,
                                onGlobal = onGlobalEffects,
                            )
                        },
                    )
                },
            )
        }

        BottomPanel(
            group = group,
            modules = { modulesState.value },
            onToggle = vm::setModuleOn,
            onAmount = vm::setModuleAmount,
            panelState = panelState,
            backdrop = backdrop,
            rootHeightPx = rootHeightPx,
            topInsetPx = topInsetPx,
            bottomInsetPx = bottomInsetPx,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
    }
}

/** Kolumna górna: padding top 28 (od dołu paska stanu), boki 18, gap 10, wyśrodkowana. */
@Composable
private fun TopColumn(
    dialSize: Dp,
    master: () -> Float,
    onMaster: (Float) -> Unit,
    onMasterReset: () -> Unit,
    status: State<String>,
    group: ModuleGroup,
    onGroup: (ModuleGroup) -> Unit,
    header: @Composable () -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal))
            .padding(
                top = SzkloDims.ScreenPaddingTop,
                start = SzkloDims.ScreenPaddingSide,
                end = SzkloDims.ScreenPaddingSide,
            ),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(SzkloDims.ColumnGap),
    ) {
        header()
        PowerDial(
            value = master,
            onValueChange = onMaster,
            onResetToPreset = onMasterReset,
            modifier = Modifier.padding(top = SzkloDims.DialTopMargin),
            dialSize = dialSize,
        )
        StatusLine(status)
        GroupChips(selected = group, onSelect = onGroup)
    }
}

@Composable
private fun StatusLine(status: State<String>) {
    Text(status.value, style = SzkloType.Status, maxLines = 1)
}

/**
 * Nagłówek: „Szkło” SG 24/600 · spacer · preset. Stuknięcie w „Szkło” albo przesunięcie palcem
 * w poziomie po nagłówku obraca okno na stronę odtwarzacza (wygląd 1:1 z makietą – bez dodatkowej ikony).
 */
@Composable
private fun Header(onFlip: () -> Unit, preset: @Composable () -> Unit) {
    val latestFlip by rememberUpdatedState(onFlip)
    Row(
        Modifier
            .fillMaxWidth()
            .pointerInput(Unit) {
                val threshold = 56.dp.toPx()
                var total = 0f
                detectHorizontalDragGestures(
                    onDragStart = { total = 0f },
                    onDragEnd = { if (abs(total) > threshold) latestFlip() },
                    onHorizontalDrag = { change, dx ->
                        change.consume()
                        total += dx
                    },
                )
            },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            "Szkło",
            style = SzkloType.AppTitle,
            modifier = Modifier
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClickLabel = "Odwróć na stronę odtwarzacza",
                    onClick = onFlip,
                )
                .semantics {
                    heading()
                    role = Role.Button
                },
        )
        Spacer(Modifier.weight(1f))
        preset()
    }
}
