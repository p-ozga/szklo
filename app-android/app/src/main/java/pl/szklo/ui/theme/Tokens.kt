package pl.szklo.ui.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/**
 * Tokeny projektu „Android 2 · Pokrętło mocy (Compound)” – sekcja 12.3 scenariusza i makieta
 * `design/and2-pokretlo-mocy.dc.html` (makieta wygrywa). 1 px makiety = 1 dp, tekst w sp.
 */
object SzkloColors {
    val Background = Color(0xFF081113)
    val Text = Color(0xFFF1F7F5)
    val Accent = Color(0xFF1EC97B)
    val PresetAccent = Color(0xFFC9921E)
    val OnAccent = Color(0xFF081113)

    /** Panel dolny: rgba(14,26,28,.50) nad rozmytym tłem; bez rozmycia (API < 31) – krycie .62. */
    val PanelTint = Color(0xFF0E1A1C)
    const val PANEL_TINT_ALPHA_BLUR = 0.50f
    const val PANEL_TINT_ALPHA_FALLBACK = 0.62f

    val ThumbFill = Color(0xFFF4FFF9)
    val ThumbFillDisabled = Color(0xFF56656A)
    val ThumbBorderDisabled = Color(0xFF3B494D)

    /** Biel z kryciem – wszystkie „szklane” obrysy i tory. */
    fun white(alpha: Float) = Color.White.copy(alpha = alpha)
    fun text(alpha: Float) = Text.copy(alpha = alpha)
}

/** Plamy tła (pozycje w dp względem ekranu referencyjnego 390×844, blur = odchylenie standardowe CSS). */
object SzkloBlobs {
    data class Blob(val color: Color, val alpha: Float, val diameter: Float, val blur: Float)

    /** left 30, top 90 (od dołu paska stanu), Ø 330, blur 90 – poziomo wyśrodkowana jak na 390 dp. */
    val B1 = Blob(Color(0xFF1FB8C9), 0.45f, 330f, 90f)
    const val B1_TOP = 90f

    /** left −100, bottom −60, Ø 360, blur 80. */
    val B2 = Blob(Color(0xFF386E74), 0.90f, 360f, 80f)
    const val B2_LEFT = -100f
    const val B2_BOTTOM = -60f

    /** right −120, bottom 120, Ø 300, blur 110. */
    val B3 = Blob(Color(0xFFC9921E), 0.40f, 300f, 110f)
    const val B3_RIGHT = -120f
    const val B3_BOTTOM = 120f
}

object SzkloDims {
    val ScreenPaddingTop = 28.dp
    val ScreenPaddingSide = 18.dp
    val ColumnGap = 10.dp

    val DialSize = 260.dp
    /** Najmniejsze pokrętło na niskich ekranach (dwa rzędy chipów muszą się zmieścić nad panelem). */
    val DialMinSize = 120.dp
    val DialTopMargin = 8.dp
    val DialTrackRadius = 110.dp
    val DialStroke = 14.dp
    val DialFaceRadius = 86.dp

    val ChipMinHeight = 40.dp
    val ChipPadV = 9.dp
    const val CHIPS_PER_ROW = 3
    val ChipGap = 8.dp
    val ChipsTopMargin = 4.dp

    /** Kolumna górna bez pokrętła: marginesy, nagłówek, linia stanu, dwa rzędy chipów, odstęp nad panelem. */
    val TopColumnWithoutDial = 226.dp

    val PanelHeight = 356.dp
    val PanelRadius = 28.dp
    val PanelPadTop = 12.dp
    val PanelPadSide = 20.dp
    val PanelPadBottom = 20.dp
    val PanelGap = 14.dp
    val PanelBlur = 36.dp

    val RowMinHeight = 40.dp
    val RowGap = 12.dp
    val CheckSize = 24.dp
    val CheckRadius = 7.dp
    val NameWidth = 96.dp
    val ValueWidth = 34.dp
    val TrackHeight = 4.dp
    val ThumbDiameter = 20.dp
    val ThumbBorder = 3.dp
}
