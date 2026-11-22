package pl.szklo.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
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

/** Chipy grup (12.4): gap 8, przewijane poziomo, margines górny 4 (ponad odstęp kolumny 10). */
@Composable
fun GroupChips(
    selected: ModuleGroup,
    onSelect: (ModuleGroup) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(top = SzkloDims.ChipsTopMargin)
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(SzkloDims.ChipGap),
    ) {
        for (g in ModuleGroup.entries) {
            GroupChip(label = g.label, selected = g == selected, onClick = { onSelect(g) })
        }
    }
}

/**
 * Chip: min. wys. 40, padding 9/14, pigułka, Inter 13. Wybrany: #1EC97B, tekst #081113/600.
 * Niewybrany: biel 8%, obrys 1 dp biel 16%. Obrys CSS powiększa przycisk o 1 px z każdej strony,
 * stąd poziomy padding 15 dp dla niewybranego (szerokości zgodne z makietą).
 */
@Composable
private fun GroupChip(label: String, selected: Boolean, onClick: () -> Unit) {
    val base = Modifier
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
            .padding(
                horizontal = if (selected) SzkloDims.ChipPadH else SzkloDims.ChipPadH + 1.dp,
                vertical = SzkloDims.ChipPadV,
            ),
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
