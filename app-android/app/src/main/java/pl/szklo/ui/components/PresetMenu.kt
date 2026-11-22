package pl.szklo.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import pl.szklo.audio.GlobalEffectsState
import pl.szklo.model.Preset
import pl.szklo.model.Presets
import pl.szklo.ui.theme.SzkloColors
import pl.szklo.ui.theme.SzkloType

/**
 * „Codzienny ▾” w nagłówku + menu presetów (własne okienko – bez wyglądu Material).
 * Pod presetami dwa przełączniki spoza makiety: przetwarzanie włączone (globalny bypass)
 * i tryb „inne aplikacje” (przybliżenie przez efekt systemowy).
 */
@Composable
fun PresetSelector(
    presetKey: String,
    userPresets: List<Preset>,
    onPreset: (String) -> Unit,
    enabled: Boolean,
    onEnabled: (Boolean) -> Unit,
    global: GlobalEffectsState,
    onGlobal: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    var open by rememberSaveable { mutableStateOf(false) }
    val name = Presets.byKey(presetKey)?.name ?: userPresets.firstOrNull { it.key == presetKey }?.name ?: Presets.default.name
    val density = LocalDensity.current

    Box(modifier) {
        Text(
            text = "$name ▾",
            style = SzkloType.PresetLabel,
            modifier = Modifier
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClick = { open = true },
                )
                .clearAndSetSemantics {
                    contentDescription = "Preset: $name"
                    stateDescription = "rozwiń listę presetów"
                    role = Role.DropdownList
                    onClick { open = true; true }
                },
        )
        if (open) {
            val dy = with(density) { 28.dp.roundToPx() }
            Popup(
                alignment = Alignment.TopEnd,
                offset = IntOffset(0, dy),
                onDismissRequest = { open = false },
                properties = PopupProperties(focusable = true),
            ) {
                MenuCard {
                    for ((i, p) in (Presets.all + userPresets).withIndex()) {
                        if (i == Presets.all.size) Divider()
                        val selected = p.key == presetKey
                        MenuItem(
                            text = p.name,
                            selected = selected,
                            modifier = Modifier.selectable(selected = selected, role = Role.RadioButton) {
                                onPreset(p.key)
                                open = false
                            },
                        )
                    }
                    Divider()
                    MenuToggle(
                        text = "Przetwarzanie",
                        subtitle = if (enabled) "włączone" else "wyłączone – dźwięk oryginalny",
                        checked = enabled,
                        onChange = onEnabled,
                    )
                    MenuToggle(
                        text = "Inne aplikacje",
                        subtitle = globalSubtitle(global),
                        checked = global.requested,
                        onChange = onGlobal,
                    )
                }
            }
        }
    }
}

private fun globalSubtitle(g: GlobalEffectsState): String = when {
    !g.supported -> "wymaga Androida 9 lub nowszego"
    g.error != null && g.requested -> g.error
    !g.requested -> "przybliżenie efektem systemowym"
    g.attached.isNotEmpty() -> "aktywne sesje: ${g.attached.size}"
    else -> "czeka na odtwarzacz (np. Spotify)"
}

@Composable
private fun MenuCard(content: @Composable () -> Unit) {
    val shape = RoundedCornerShape(16.dp)
    Column(
        Modifier
            .width(248.dp)
            .clip(shape)
            .background(SzkloColors.PanelTint.copy(alpha = 0.96f))
            .border(1.dp, SzkloColors.white(0.14f), shape)
            .padding(vertical = 6.dp),
    ) { content() }
}

@Composable
private fun MenuItem(text: String, selected: Boolean, modifier: Modifier = Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 44.dp)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text,
            style = if (selected) SzkloType.PresetLabel.copy(fontSize = SzkloType.Body.fontSize) else SzkloType.Body,
            modifier = Modifier.weight(1f),
        )
        if (selected) Text("✓", style = SzkloType.PresetLabel)
    }
}

@Composable
private fun MenuToggle(text: String, subtitle: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 52.dp)
            .toggleable(value = checked, role = Role.Switch, onValueChange = onChange)
            .padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(text, style = SzkloType.Body)
            Text(subtitle, style = SzkloType.BodySecondary)
        }
        ModuleCheckbox(checked = checked)
    }
}

@Composable
private fun Divider() {
    Spacer(Modifier.height(6.dp))
    Box(
        Modifier
            .fillMaxWidth()
            .height(1.dp)
            .background(SzkloColors.white(0.10f)),
    )
    Spacer(Modifier.height(6.dp))
}
