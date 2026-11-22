package pl.szklo.ui.theme

import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.unit.sp
import pl.szklo.R

/**
 * Czcionki z res/font: statyczne instancje zmiennych fontów Google Fonts (OFL, licencje w assets/licencje):
 * Inter 400/500/600 (opsz 14) i Space Grotesk 500/600.
 */
val Inter = FontFamily(
    Font(R.font.inter_regular, FontWeight.W400),
    Font(R.font.inter_medium, FontWeight.W500),
    Font(R.font.inter_semibold, FontWeight.W600),
)

val SpaceGrotesk = FontFamily(
    Font(R.font.space_grotesk_medium, FontWeight.W500),
    Font(R.font.space_grotesk_semibold, FontWeight.W600),
)

/**
 * Style tekstu z makiety. Wysokość linii nieokreślona = metryki fontu (odpowiednik CSS `line-height: normal`).
 * Kolor domyślny – LocalContentColor (#F1F7F5); drugorzędne kolory ustawione jawnie.
 */
@OptIn(ExperimentalTextApi::class)
object SzkloType {
    val AppTitle = TextStyle(fontFamily = SpaceGrotesk, fontWeight = FontWeight.W600, fontSize = 24.sp)
    val PresetLabel = TextStyle(fontFamily = Inter, fontWeight = FontWeight.W600, fontSize = 13.sp, color = SzkloColors.PresetAccent)

    /** „68%”: Space Grotesk 52/600, line-height 1 (półodstęp rozłożony równo – jak CSS). */
    val DialValue = TextStyle(
        fontFamily = SpaceGrotesk, fontWeight = FontWeight.W600, fontSize = 52.sp, lineHeight = 52.sp,
        lineHeightStyle = LineHeightStyle(alignment = LineHeightStyle.Alignment.Center, trim = LineHeightStyle.Trim.None),
    )
    val DialCaption = TextStyle(fontFamily = Inter, fontSize = 13.sp, color = SzkloColors.text(0.70f))
    val Status = TextStyle(fontFamily = Inter, fontSize = 12.sp, color = SzkloColors.text(0.66f))

    val Chip = TextStyle(fontFamily = Inter, fontSize = 13.sp, color = SzkloColors.Text)
    val ChipSelected = TextStyle(fontFamily = Inter, fontWeight = FontWeight.W600, fontSize = 13.sp, color = SzkloColors.OnAccent)

    val PanelTitle = TextStyle(fontFamily = SpaceGrotesk, fontWeight = FontWeight.W600, fontSize = 18.sp)
    val PanelCount = TextStyle(fontFamily = Inter, fontSize = 12.sp, color = SzkloColors.text(0.66f))

    val ModuleName = TextStyle(fontFamily = Inter, fontSize = 14.sp)
    val ModuleValue = TextStyle(
        fontFamily = Inter, fontSize = 12.5.sp, color = SzkloColors.text(0.75f),
        fontFeatureSettings = "tnum",
    )

    // Strona tylna i menu (poza makietą – w tym samym języku wizualnym).
    val BackTitle = TextStyle(fontFamily = SpaceGrotesk, fontWeight = FontWeight.W600, fontSize = 24.sp)
    val CardTitle = TextStyle(fontFamily = SpaceGrotesk, fontWeight = FontWeight.W600, fontSize = 16.sp)
    val Body = TextStyle(fontFamily = Inter, fontSize = 14.sp)
    val BodySecondary = TextStyle(fontFamily = Inter, fontSize = 13.sp, color = SzkloColors.text(0.70f))
    val Button = TextStyle(fontFamily = Inter, fontWeight = FontWeight.W600, fontSize = 13.sp)
}
