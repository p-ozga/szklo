package pl.szklo.ui.backside

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.LaunchedEffect
import pl.szklo.audio.AudioHub
import pl.szklo.model.Presets
import pl.szklo.ui.theme.SzkloColors
import pl.szklo.ui.theme.SzkloType

private enum class Tryb { NORMALNY, NAZWA, USUWANIE }

/**
 * Presety użytkownika: „Zapisz jako…”, „Zapisz” (nadpisuje bieżący preset użytkownika), „Usuń”
 * oraz lista własnych presetów (stuknięcie = wczytanie z płynnym przejściem). Presety wbudowane są tylko do odczytu.
 */
@Composable
internal fun PresetsCard(onApplyPreset: (String) -> Unit) {
    val key by AudioHub.presetKey.collectAsStateWithLifecycle()
    val params by AudioHub.params.collectAsStateWithLifecycle()
    val user by AudioHub.userPresets.collectAsStateWithLifecycle()

    var tryb by rememberSaveable { mutableStateOf(Tryb.NORMALNY) }
    var nazwa by rememberSaveable { mutableStateOf("") }
    var komunikat by rememberSaveable { mutableStateOf<String?>(null) }

    val isUser = AudioHub.isUserPreset(key) && user.any { it.key == key }
    val current = AudioHub.presetByKey(key) ?: Presets.default
    val modified = AudioHub.isModified(params, key)

    GlassCard {
        Text("Presety", style = SzkloType.CardTitle)
        Text(
            buildString {
                append("Bieżący: ").append(current.name)
                if (!isUser) append(" (wbudowany)")
                if (modified) append(" · zmieniony")
            },
            style = SzkloType.Body,
        )

        when (tryb) {
            Tryb.NORMALNY -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (isUser) {
                    PillButton("Zapisz", onClick = { komunikat = AudioHub.saveCurrentPreset() }, primary = modified)
                }
                PillButton("Zapisz jako…", onClick = {
                    nazwa = if (isUser) current.name else ""
                    komunikat = null
                    tryb = Tryb.NAZWA
                }, primary = !isUser)
                if (isUser) {
                    PillButton("Usuń", onClick = { komunikat = null; tryb = Tryb.USUWANIE }, primary = false)
                }
            }

            Tryb.NAZWA -> {
                val focus = remember { FocusRequester() }
                LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
                val zatwierdz = {
                    val msg = AudioHub.savePresetAs(nazwa)
                    komunikat = msg
                    if (msg.startsWith("Zapisano") || msg.startsWith("Nadpisano")) tryb = Tryb.NORMALNY
                }
                NameField(nazwa, { nazwa = it.take(40) }, Modifier.focusRequester(focus), onDone = zatwierdz)
                Text("Ta sama nazwa co Twój preset = nadpisanie go.", style = SzkloType.BodySecondary)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    PillButton("Zapisz", onClick = zatwierdz, primary = true)
                    PillButton("Anuluj", onClick = { tryb = Tryb.NORMALNY; komunikat = null }, primary = false)
                }
            }

            Tryb.USUWANIE -> {
                Text("Usunąć preset „${current.name}”? Bieżące ustawienia zostaną.", style = SzkloType.Body)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    PillButton("Usuń", onClick = { komunikat = AudioHub.deletePreset(key); tryb = Tryb.NORMALNY }, primary = true)
                    PillButton("Anuluj", onClick = { tryb = Tryb.NORMALNY }, primary = false)
                }
            }
        }

        komunikat?.let { Text(it, style = SzkloType.BodySecondary) }

        if (user.isNotEmpty()) {
            Text("Twoje presety", style = SzkloType.BodySecondary)
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                for (p in user) {
                    val sel = p.key == key
                    val shape = RoundedCornerShape(12.dp)
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 44.dp)
                            .clip(shape)
                            .background(SzkloColors.white(if (sel) 0.12f else 0.05f))
                            .border(1.dp, if (sel) SzkloColors.Accent else SzkloColors.white(0.10f), shape)
                            .clickable(role = Role.Button) {
                                komunikat = null
                                tryb = Tryb.NORMALNY
                                onApplyPreset(p.key)
                            }
                            .padding(horizontal = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(p.name, style = SzkloType.Body, modifier = Modifier.weight(1f), maxLines = 1)
                        if (sel) Text("✓", style = SzkloType.PresetLabel)
                    }
                }
            }
        } else {
            Text("Nie masz jeszcze własnych presetów – ustaw moduły i stuknij „Zapisz jako…”.", style = SzkloType.BodySecondary)
        }
    }
}

@Composable
private fun NameField(value: String, onChange: (String) -> Unit, modifier: Modifier, onDone: () -> Unit) {
    val shape = RoundedCornerShape(12.dp)
    Box(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 44.dp)
            .clip(shape)
            .background(SzkloColors.white(0.06f))
            .border(1.dp, SzkloColors.Accent, shape)
            .padding(horizontal = 14.dp, vertical = 11.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        if (value.isEmpty()) Text("Nazwa presetu, np. Mój bas", style = SzkloType.BodySecondary)
        BasicTextField(
            value = value,
            onValueChange = onChange,
            singleLine = true,
            textStyle = SzkloType.Body.copy(color = SzkloColors.Text),
            cursorBrush = SolidColor(SzkloColors.Accent),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { onDone() }),
            modifier = modifier.fillMaxWidth(),
        )
    }
}
