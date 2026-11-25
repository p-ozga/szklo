package pl.szklo.ui.backside

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.clickable
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import android.content.Intent
import android.net.Uri
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.platform.LocalContext
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import android.annotation.SuppressLint
import android.util.Base64
import android.webkit.WebView
import kotlinx.coroutines.isActive
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import pl.szklo.Dziennik
import pl.szklo.core.NativeEngine
import pl.szklo.audio.AudioHub
import pl.szklo.audio.SpotifyConnectService
import pl.szklo.audio.PlayerState
import pl.szklo.audio.PlayerStatus
import pl.szklo.ui.Fmt
import pl.szklo.ui.components.GlowBackground
import pl.szklo.ui.theme.SzkloColors
import pl.szklo.ui.theme.SzkloType

/** Akcje lokalnego odtwarzacza (pełny rdzeń Szkło) dostępne ze strony tylnej. */
data class LocalPlayerActions(
    val pickFile: () -> Unit,
    val pause: () -> Unit,
    val resume: () -> Unit,
    val stop: () -> Unit,
)

/**
 * Strona tylna „Odtwarzacz”. Na razie: zaślepka sterowania zewnętrznymi odtwarzaczami
 * (przez [PlayerControlsBackside]) + lokalny odtwarzacz pliku przez pełny rdzeń DSP.
 */
@Composable
fun PlayerBackside(
    controls: PlayerControlsBackside,
    player: PlayerState,
    coreAvailable: Boolean,
    local: LocalPlayerActions,
    onApplyPreset: (String) -> Unit,
    onBack: () -> Unit,
) {
    val density = LocalDensity.current
    var biblioteka by rememberSaveable { mutableStateOf(false) }
    val scroll = rememberScrollState()
    if (biblioteka && coreAvailable) {
        // osobny ekran zamiast strony tylnej (nic pod spodem nie przyjmuje dotyku)
        BibliotekaScreen(onClose = { biblioteka = false })
        return
    }
    Box(Modifier.fillMaxSize()) {
        GlowBackground(
            Modifier.fillMaxSize(),
            topInsetPx = WindowInsets.statusBars.getTop(density),
            bottomInsetPx = WindowInsets.navigationBars.getBottom(density),
        )
        Column(
            Modifier
                .fillMaxSize()
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .verticalScroll(scroll)
                .padding(top = 28.dp, start = 18.dp, end = 18.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("Odtwarzacz", style = SzkloType.BackTitle, modifier = Modifier.weight(1f).semantics { heading() })
                PillButton("Wróć", onClick = onBack, primary = false)
            }

            PresetsCard(onApplyPreset)
            SpotifyConnectCard(coreAvailable)
            BibliotekaCard(coreAvailable) { biblioteka = true }
            ExternalPlayersCard(controls)
            LocalPlayerCard(player, coreAvailable, local)
            WizualizacjaCard(coreAvailable)
            DziennikCard()
            LicensesCard()
        }
    }
}

/**
 * Wizualizacja muzyki: strona z assets/wizualizacja (wspólna z programem na PC; silnik Butterchurn – Jordan Berg, MIT,
 * presety własne) w WebView. Dźwięk podaje rdzeń (fala wyjścia), więc działa dla odtwarzacza Szkła i Spotify Connect;
 * w trybie „inne aplikacje” aplikacja nie ma dostępu do dźwięku.
 */
@Composable
private fun WizualizacjaCard(coreAvailable: Boolean) {
    var on by remember { mutableStateOf(false) }
    GlassCard {
        Text("Wizualizacja", style = SzkloType.CardTitle)
        Text(
            if (coreAvailable) {
                "Obraz poruszany muzyką z odtwarzacza Szkła i ze Spotify Connect. Stuknij obraz, żeby go zmienić. " +
                    "Włączona zużywa więcej baterii."
            } else {
                "Niedostępna: rdzeń Szkła nie jest wczytany."
            },
            style = SzkloType.BodySecondary,
        )
        if (coreAvailable) {
            PillButton(if (on) "Wyłącz wizualizację" else "Włącz wizualizację", onClick = { on = !on }, primary = !on)
            if (on) WizualizacjaWidok()
        }
    }
}

@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun WizualizacjaWidok() {
    var web by remember { mutableStateOf<WebView?>(null) }
    AndroidView(
        modifier = Modifier.fillMaxWidth().height(230.dp).clip(RoundedCornerShape(14.dp)),
        factory = { ctx ->
            WebView(ctx).apply {
                setBackgroundColor(android.graphics.Color.BLACK)
                // tylko własna strona z zasobów aplikacji; bez dostępu do plików, treści i sieci
                settings.javaScriptEnabled = true
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.blockNetworkLoads = true
                loadUrl("file:///android_asset/wizualizacja/index.html?fps=30&skala=1.25")
                web = this
            }
        },
        onRelease = { it.destroy() },
    )
    LaunchedEffect(web) {
        val w = web ?: return@LaunchedEffect
        val bufor = ByteArray(NativeEngine.SCOPE_BYTES)
        try {
            while (isActive) {
                // ok. 30 razy na sekundę; gdy nic nie gra, nic nie wysyłamy i obraz sam się uspokaja
                if (AudioHub.readScope(bufor)) {
                    w.evaluateJavascript("window.szkloFala&&szkloFala('${Base64.encodeToString(bufor, Base64.NO_WRAP)}')", null)
                }
                delay(33)
            }
        } finally {
            AudioHub.scopeOff()
        }
    }
}

/** Zapis dziennika aplikacji do folderu Dokumenty (dostępny w menedżerze plików i przez USB). */
@Composable
private fun DziennikCard() {
    var auto by remember { mutableStateOf(Dziennik.automatyczny) }
    var komunikat by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    GlassCard {
        Text("Dziennik aplikacji", style = SzkloType.CardTitle)
        Text(
            if (Dziennik.dostepny) {
                "Zapisuje zdarzenia i błędy do folderu ${Dziennik.FOLDER} (pliki szklo-data.txt). " +
                    "Nazwa konta Spotify jest zamazana. Nic nie jest wysyłane."
            } else {
                "Zapis do Dokumentów wymaga Androida 10."
            },
            style = SzkloType.BodySecondary,
        )
        if (Dziennik.dostepny) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PillButton(
                    if (auto) "Zapis automatyczny: wł." else "Zapis automatyczny: wył.",
                    onClick = { auto = !auto; Dziennik.automatyczny = auto },
                    primary = auto,
                )
                PillButton("Zapisz teraz", onClick = {
                    scope.launch { komunikat = withContext(Dispatchers.IO) { Dziennik.zapiszTeraz() } }
                }, primary = false)
            }
            if (komunikat.isNotEmpty()) Text(komunikat, style = SzkloType.BodySecondary)
        }
    }
}

/** Cudze elementy (biblioteki, czcionki, znaki towarowe) – wyraźnie podpisane; pełna lista z assets/licencje.txt. */
@Composable
private fun LicensesCard() {
    val ctx = LocalContext.current
    var open by remember { mutableStateOf(false) }
    val full = remember(open) {
        if (!open) "" else runCatching { ctx.assets.open("licencje.txt").bufferedReader().use { it.readText() } }.getOrDefault("Brak pliku licencji")
    }
    GlassCard {
        Text("O programie i licencje", style = SzkloType.CardTitle)
        Text(
            "Szkło korzysta z cudzych elementów: czcionek Inter i Space Grotesk (SIL OFL 1.1), Android Jetpack (Google), " +
                "Kotlin (JetBrains), librespot (MIT) i Symphonia (MPL-2.0) oraz wielu bibliotek open source. " +
                "Spotify to znak towarowy Spotify AB – Szkło nie jest powiązane ze Spotify ani przez nie wspierane.",
            style = SzkloType.BodySecondary,
        )
        PillButton(if (open) "Ukryj pełną listę" else "Pełna lista i licencje", onClick = { open = !open }, primary = false)
        if (open) {
            Text(
                full,
                style = SzkloType.BodySecondary.copy(fontFamily = FontFamily.Monospace, fontSize = 10.sp, lineHeight = 13.sp),
                modifier = Modifier.fillMaxWidth().heightIn(max = 420.dp).verticalScroll(rememberScrollState()),
            )
        }
    }
}

/**
 * Spotify Connect „Szkło”: pełny silnik dla Spotify (Premium). Logowanie otwiera przeglądarkę automatycznie,
 * gdy rdzeń poda adres; potem w Spotify wybiera się urządzenie „Szkło”.
 */
@Composable
private fun SpotifyConnectCard(coreAvailable: Boolean) {
    val ctx = LocalContext.current
    val sp by AudioHub.spotify.collectAsStateWithLifecycle()
    // adres logowania → przeglądarka (raz na adres)
    LaunchedEffect(sp.loginUrl) {
        sp.loginUrl?.let { url ->
            runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
        }
    }
    GlassCard {
        Text("Spotify Connect", style = SzkloType.CardTitle)
        val opis = when {
            !coreAvailable -> "Brak rdzenia DSP"
            !sp.running -> "Pełne Szkło dla Spotify (Premium): włącz, a potem w Spotify wybierz urządzenie „${SpotifyConnectService.DEVICE_NAME}”."
            sp.status == "login" -> "Zaloguj się do Spotify w otwartej przeglądarce, potem wróć tutaj."
            sp.status == "ready" -> "Gotowe. W Spotify stuknij ikonę urządzeń i wybierz „${SpotifyConnectService.DEVICE_NAME}”."
            sp.status == "playing" -> "Gra przez pełny silnik Szkła ✓"
            sp.status == "error" && sp.error?.contains("audio key") == true ->
                "Spotify połączył się z „Szkłem”, ale odmówił klucza do odszyfrowania utworu. To blokada po stronie Spotify " +
                    "dla nieoficjalnych odbiorników (dotyczy części kont) – aplikacja tego nie obejdzie."
            sp.status == "error" -> "Błąd: ${sp.error ?: "nieznany"} – ponawiam…"
            else -> "Łączenie ze Spotify…"
        }
        Text(opis, style = SzkloType.BodySecondary)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (!sp.running) {
                PillButton("Włącz", onClick = { SpotifyConnectService.start(ctx) }, primary = true)
            } else {
                PillButton("Wyłącz", onClick = { SpotifyConnectService.stop(ctx) }, primary = false)
                if (sp.status == "login" && sp.loginUrl != null) {
                    PillButton("Otwórz logowanie", onClick = {
                        runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(sp.loginUrl)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
                    }, primary = true)
                }
                if (sp.status == "ready" || sp.status == "playing" || sp.status == "error") {
                    PillButton("Wyloguj", onClick = { SpotifyConnectService.logout(ctx) }, primary = false)
                }
            }
        }
        if (coreAvailable) SpotifyLog()
    }
}

/** Dziennik Spotify (ostatnie komunikaty rdzenia) – do diagnozy bez kabla USB. */
@Composable
private fun SpotifyLog() {
    val ctx = LocalContext.current
    var open by remember { mutableStateOf(false) }
    var text by remember { mutableStateOf("") }
    var copied by remember { mutableStateOf(false) }
    val version = remember {
        runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull() ?: "?"
    }
    LaunchedEffect(open) {
        while (open) {
            text = runCatching { NativeEngine.nativeSpotifyLog() }.getOrDefault("")
            delay(1000)
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        PillButton(if (open) "Ukryj dziennik" else "Dziennik", onClick = { open = !open; copied = false }, primary = false)
        if (open) {
            PillButton(if (copied) "Skopiowano ✓" else "Kopiuj", onClick = {
                val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                cm.setPrimaryClip(android.content.ClipData.newPlainText("Szkło – dziennik Spotify", "Szkło $version\n$text"))
                copied = true
            }, primary = false)
        }
        Text("v$version", style = SzkloType.BodySecondary)
    }
    if (open) {
        val lines = text.lines().takeLast(40).joinToString("\n").ifBlank { "(pusto – włącz Spotify Connect i spróbuj odtworzyć utwór)" }
        Text(
            lines,
            style = SzkloType.BodySecondary.copy(fontFamily = FontFamily.Monospace, fontSize = 10.sp, lineHeight = 13.sp),
            modifier = Modifier.fillMaxWidth().heightIn(max = 360.dp).verticalScroll(rememberScrollState()),
        )
    }
}

@Composable
private fun ExternalPlayersCard(controls: PlayerControlsBackside) {
    GlassCard {
        Text("Inne aplikacje", style = SzkloType.CardTitle)
        if (!controls.isAvailable) {
            Text("Sterowanie aplikacjami odtwarzającymi muzykę – wkrótce", style = SzkloType.BodySecondary)
        } else {
            val st by controls.state.collectAsStateWithLifecycle()
            Text(listOfNotNull(st.appLabel, st.artist, st.title).joinToString(" · ").ifEmpty { "Nic nie gra" }, style = SzkloType.Body)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PillButton("Poprzedni", onClick = controls::skipToPrevious, primary = false)
                PillButton(if (st.isPlaying) "Pauza" else "Graj", onClick = controls::playPause, primary = true)
                PillButton("Następny", onClick = controls::skipToNext, primary = false)
            }
        }
    }
}

@Composable
private fun LocalPlayerCard(player: PlayerState, coreAvailable: Boolean, local: LocalPlayerActions) {
    GlassCard {
        Text("Plik z telefonu", style = SzkloType.CardTitle)
        Text(
            if (coreAvailable) {
                "Pełny rdzeń Szkło – wszystkie moduły dokładnie jak na PC."
            } else {
                "Brak biblioteki rdzenia (libszklo_jni.so) – plik zagra bez obróbki."
            },
            style = SzkloType.BodySecondary,
        )
        val line = when (player.status) {
            PlayerStatus.IDLE -> player.title?.let { "Zakończono: $it" } ?: "Nic nie gra"
            PlayerStatus.PREPARING -> "Przygotowanie: ${player.title.orEmpty()}"
            PlayerStatus.PLAYING, PlayerStatus.PAUSED -> buildString {
                append(player.title.orEmpty())
                append(" · ")
                append(Fmt.time(player.positionMs))
                if (player.durationMs > 0) append(" / ").append(Fmt.time(player.durationMs))
                if (player.sampleRate > 0) append(" · ").append(Fmt.khz(player.sampleRate))
                if (!player.processed) append(" · bez obróbki")
            }
            PlayerStatus.ERROR -> "Błąd: ${player.error.orEmpty()}"
        }
        Text(line, style = SzkloType.Body)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PillButton("Wybierz plik", onClick = local.pickFile, primary = true)
            when (player.status) {
                PlayerStatus.PLAYING -> PillButton("Pauza", onClick = local.pause, primary = false)
                PlayerStatus.PAUSED -> PillButton("Wznów", onClick = local.resume, primary = false)
                else -> Unit
            }
            if (player.status == PlayerStatus.PLAYING || player.status == PlayerStatus.PAUSED || player.status == PlayerStatus.PREPARING) {
                PillButton("Stop", onClick = local.stop, primary = false)
            }
        }
    }
}

/** Karta szklana (tokeny „karta” z PC: biel 7%, obrys biel 12%, promień 16). */
@Composable
internal fun GlassCard(content: @Composable () -> Unit) {
    val shape = RoundedCornerShape(16.dp)
    Column(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(SzkloColors.white(0.07f))
            .border(1.dp, SzkloColors.white(0.12f), shape)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) { content() }
}

/** Przycisk-pigułka w stylu chipów (wys. 40, padding 9/14). */
@Composable
internal fun PillButton(text: String, onClick: () -> Unit, primary: Boolean) {
    val shape = RoundedCornerShape(percent = 50)
    val base = Modifier.heightIn(min = 40.dp).clip(shape)
    val styled = if (primary) {
        base.background(SzkloColors.Accent)
    } else {
        base.background(SzkloColors.white(0.08f)).border(1.dp, SzkloColors.white(0.16f), shape)
    }
    Box(
        styled
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 9.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, style = SzkloType.Button.copy(color = if (primary) SzkloColors.OnAccent else SzkloColors.Text), maxLines = 1)
    }
}
