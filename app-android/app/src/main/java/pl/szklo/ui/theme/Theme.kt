package pl.szklo.ui.theme

import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider

/**
 * Material 3 tylko jako baza (ripple, Text). Wygląd w 100% własny; aplikacja zawsze ciemna,
 * niezależnie od trybu systemu (12.5).
 */
@Composable
fun SzkloTheme(content: @Composable () -> Unit) {
    val colors = darkColorScheme(
        primary = SzkloColors.Accent,
        onPrimary = SzkloColors.OnAccent,
        secondary = SzkloColors.PresetAccent,
        background = SzkloColors.Background,
        onBackground = SzkloColors.Text,
        surface = SzkloColors.Background,
        onSurface = SzkloColors.Text,
    )
    // Domyślna typografia M3 z Interem (używamy własnych stylów SzkloType; to tylko zabezpieczenie).
    val d = Typography()
    val typography = d.copy(
        bodyLarge = d.bodyLarge.copy(fontFamily = Inter),
        bodyMedium = d.bodyMedium.copy(fontFamily = Inter),
        bodySmall = d.bodySmall.copy(fontFamily = Inter),
        labelLarge = d.labelLarge.copy(fontFamily = Inter),
        labelMedium = d.labelMedium.copy(fontFamily = Inter),
        labelSmall = d.labelSmall.copy(fontFamily = Inter),
    )
    MaterialTheme(colorScheme = colors, typography = typography) {
        CompositionLocalProvider(LocalContentColor provides SzkloColors.Text, content = content)
    }
}
