package pl.szklo.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import pl.szklo.model.ModuleGroup
import pl.szklo.ui.theme.SzkloColors
import pl.szklo.ui.theme.SzkloDims
import pl.szklo.ui.theme.SzkloType

private val Pill = RoundedCornerShape(percent = 50)
private val ChipUnselectedBg = SzkloColors.white(0.08f)
private val ChipUnselectedBorder = SzkloColors.white(0.16f)

/**
 * Chipy grup: dwa rzędy po trzy, równe szerokości, gap 8, margines górny 4 – WSZYSTKIE grupy zawsze widoczne.
 * Wcześniej jeden rząd przewijany poziomo: na telefonie 360–411 dp „Stereo” i „Wyjście” były poza ekranem i nic nie
 * wskazywało, że pasek się przewija (uwaga użytkownika 2026-11-24: „nie widzę nigdzie stereo”).
 */
@Composable
fun GroupChips(
    selected: ModuleGroup,
    onSelect: (ModuleGroup) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(top = SzkloDims.ChipsTopMargin),
        verticalArrangement = Arrangement.spacedBy(SzkloDims.ChipGap),
    ) {
        for (rowGroups in ModuleGroup.entries.chunked(SzkloDims.CHIPS_PER_ROW)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(SzkloDims.ChipGap)) {
                for (g in rowGroups) {
                    GroupChip(label = g.label, selected = g == selected, onClick = { onSelect(g) }, modifier = Modifier.weight(1f))
                }
            }
        }
    }
}

/**
 * Chip: min. wys. 40, pigułka, Inter 13, tekst na środku. Wybrany: #1EC97B, tekst #081113/600.
 * Niewybrany: biel 8%, obrys 1 dp biel 16%.
 */
@Composable
private fun GroupChip(label: String, selected: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val base = modifier
        .heightIn(min = SzkloDims.ChipMinHeight)
        .clip(Pill)
    val styled = if (selected) {
        base.background(SzkloColors.Accent)
    } else {
        base.background(ChipUnselectedBg).border(1.dp, ChipUnselectedBorder, Pill)
    }
    Box(
        modifier = styled
            .selectable(selected = selected, role = Role.Tab, onClick = onClick)
            .padding(horizontal = 4.dp, vertical = SzkloDims.ChipPadV),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label,
            style = if (selected) SzkloType.ChipSelected else SzkloType.Chip,
            maxLines = 1,
            softWrap = false,
        )
    }
}
