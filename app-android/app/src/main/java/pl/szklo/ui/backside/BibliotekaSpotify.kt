package pl.szklo.ui.backside

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import pl.szklo.audio.AudioHub
import pl.szklo.audio.SpotifyConnectService
import pl.szklo.core.NativeEngine
import pl.szklo.ui.Fmt
import pl.szklo.ui.components.GlowBackground
import pl.szklo.ui.theme.SzkloColors
import pl.szklo.ui.theme.SzkloType

/*
 * Biblioteka Spotify i wyszukiwanie na telefonie. Dane daje rdzeń (moduł `szklo-spotify`, wspólny z programem na PC):
 * polecenia idą jako JSON, stan wraca jako JSON z numerem wersji.
 */

private data class SpLista(val uri: String, val nazwa: String, val ile: Int, val rodzaj: String, val folder: String)
private data class SpOdn(val nazwa: String, val uri: String)
private data class SpUtwor(val uri: String, val tytul: String, val wykonawcy: List<SpOdn>, val album: SpOdn, val czas: Double)
private data class SpAlbum(val uri: String, val nazwa: String, val wykonawca: String, val rok: Int, val ile: Int)

private data class SpWidok(
    val rodzaj: String = "",
    val uri: String = "",
    val tytul: String = "",
    val q: String = "",
    val zaznaczona: String = "",
    val wczytywanie: Boolean = false,
    val info: String = "",
    val wstecz: Boolean = false,
    val utwory: List<SpUtwor> = emptyList(),
    val albumy: List<SpAlbum> = emptyList(),
    val wykonawcy: List<SpOdn> = emptyList(),
)

private data class SpStan(
    val logowanie: Boolean = true,
    val blad: String = "",
    val spisInfo: String = "",
    val listy: List<SpLista> = emptyList(),
    val widok: SpWidok = SpWidok(),
)

private fun <T> JSONArray?.lista(f: (JSONObject) -> T): List<T> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it)?.let(f) }

private fun odn(o: JSONObject?) = SpOdn(o?.optString("nazwa").orEmpty(), o?.optString("uri").orEmpty())

private fun odczytaj(o: JSONObject, stary: SpStan): SpStan {
    var s = stary.copy(logowanie = o.optBoolean("logowanie", true), blad = o.optString("blad"))
    o.optJSONObject("spis")?.let { sp ->
        s = s.copy(
            spisInfo = sp.optString("info"),
            listy = sp.optJSONArray("listy").lista {
                SpLista(it.optString("uri"), it.optString("nazwa"), it.optInt("ile"), it.optString("rodzaj"), it.optString("folder"))
            },
        )
    }
    o.optJSONObject("widok")?.let { w ->
        s = s.copy(
            widok = SpWidok(
                rodzaj = w.optString("rodzaj"), uri = w.optString("uri"), tytul = w.optString("tytul"), q = w.optString("q"),
                zaznaczona = w.optString("zaznaczona"), wczytywanie = w.optBoolean("wczytywanie"), info = w.optString("info"),
                wstecz = w.optBoolean("wstecz"),
                utwory = w.optJSONArray("utwory").lista {
                    SpUtwor(it.optString("uri"), it.optString("tytul"), it.optJSONArray("wykonawcy").lista(::odn), odn(it.optJSONObject("album")), it.optDouble("czas", 0.0))
                },
                albumy = w.optJSONArray("albumy").lista {
                    SpAlbum(it.optString("uri"), it.optString("nazwa"), it.optString("wykonawca"), it.optInt("rok"), it.optInt("ile"))
                },
                wykonawcy = w.optJSONArray("wykonawcy").lista(::odn),
            ),
        )
    }
    return s
}

private val ZAKRESY = listOf("lista" to "Zaznaczona lista", "biblioteka" to "Biblioteka", "katalog" to "Cały Spotify")
private val RODZAJE = listOf("utwory" to "Utwory", "albumy" to "Albumy", "wykonawcy" to "Wykonawcy")

/** Karta na odwrocie: wejście do biblioteki. */
@Composable
internal fun BibliotekaCard(coreAvailable: Boolean, onOpen: () -> Unit) {
    GlassCard {
        Text("Biblioteka Spotify", style = SzkloType.CardTitle)
        Text(
            if (coreAvailable) "Polubione utwory, playlisty, albumy i wykonawcy oraz szukanie – w zaznaczonej liście, w bibliotece albo w całym Spotify."
            else "Niedostępna: rdzeń Szkła nie jest wczytany.",
            style = SzkloType.BodySecondary,
        )
        if (coreAvailable) PillButton("Otwórz bibliotekę", onClick = onOpen, primary = true)
    }
}

@Composable
internal fun BibliotekaScreen(onClose: () -> Unit) {
    val ctx = LocalContext.current
    val katalog = remember { ctx.filesDir.absolutePath }
    var stan by remember { mutableStateOf(SpStan()) }
    var q by rememberSaveable { mutableStateOf("") }
    var zakres by rememberSaveable { mutableStateOf("biblioteka") }
    var rodzaj by rememberSaveable { mutableStateOf("utwory") }
    val sp by AudioHub.spotify.collectAsStateWithLifecycle()
    val w = stan.widok
    val focus = LocalFocusManager.current
    val spisScroll = rememberLazyListState()

    fun rozkaz(c: String, vararg pola: Pair<String, Any>) {
        val o = JSONObject().put("c", c)
        for ((k, v) in pola) o.put(k, v)
        runCatching { NativeEngine.nativeSpotifyLib(katalog, o.toString()) }
    }
    fun otworz(uri: String) {
        focus.clearFocus()
        q = ""
        rozkaz("otworz", "uri" to uri)
    }
    fun szukaj() = rozkaz("szukaj", "q" to q, "zakres" to zakres)

    // stan z rdzenia – tylko po zmianie wersji
    LaunchedEffect(Unit) {
        rozkaz("odswiez", "max_s" to 600)
        var ws = -1L
        var ww = -1L
        while (isActive) {
            val t = withContext(Dispatchers.Default) { runCatching { NativeEngine.nativeSpotifyLibState(katalog, ws, ww) }.getOrDefault("") }
            if (t.isNotEmpty()) {
                val stary = stan
                val (o, nowy) = withContext(Dispatchers.Default) { JSONObject(t).let { it to odczytaj(it, stary) } }
                ws = o.optLong("ws", ws)
                ww = o.optLong("ww", ww)
                if (nowy != stary) stan = nowy
            }
            delay(150)
        }
    }
    // po zalogowaniu (Spotify Connect) wczytaj bibliotekę
    LaunchedEffect(sp.status) { if (sp.status == "ready" || sp.status == "playing") rozkaz("odswiez", "max_s" to 600) }
    // szukanie w trakcie pisania
    var pierwsze by remember { mutableStateOf(true) }
    LaunchedEffect(q, zakres) {
        if (pierwsze) {
            pierwsze = false
            return@LaunchedEffect
        }
        if (q.isBlank()) {
            if (stan.widok.rodzaj == "szukaj") szukaj()
        } else {
            delay(if (zakres == "katalog") 700 else 400)
            szukaj()
        }
    }
    LaunchedEffect(w.zaznaczona) { if (w.zaznaczona.isEmpty() && zakres == "lista") zakres = "biblioteka" }

    fun wstecz() {
        if (w.rodzaj.isEmpty()) onClose() else {
            focus.clearFocus()
            q = ""
            rozkaz("wstecz")
        }
    }
    BackHandler { wstecz() }

    // granie: gdy odbiornik „Szkło” jest gotowy – przez pełny silnik; inaczej w aplikacji Spotify
    val wSzkle = sp.running && (sp.status == "ready" || sp.status == "playing")
    fun graj(i: Int) {
        val u = w.utwory.getOrNull(i) ?: return
        val poszlo = wSzkle && runCatching {
            val lista = JSONArray(w.utwory.drop(i).take(200).map { it.uri })
            NativeEngine.nativeSpotifyPlay(lista.toString())
        }.getOrDefault(false)
        if (!poszlo) otworzWSpotify(ctx, u.uri)
    }

    val density = LocalDensity.current
    Box(Modifier.fillMaxSize().background(SzkloColors.Background)) {
        GlowBackground(
            Modifier.fillMaxSize(),
            topInsetPx = WindowInsets.statusBars.getTop(density),
            bottomInsetPx = WindowInsets.navigationBars.getBottom(density),
        )
        Column(
            Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).padding(start = 18.dp, end = 18.dp, top = 16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("Biblioteka Spotify", style = SzkloType.BackTitle.copy(fontSize = SzkloType.PanelTitle.fontSize), modifier = Modifier.weight(1f).semantics { heading() })
                PillButton(if (w.rodzaj.isEmpty()) "Zamknij" else "Wstecz", onClick = ::wstecz, primary = false)
            }
            if (stan.logowanie) {
                PoleSzukania(q, { q = it }, { focus.clearFocus(); szukaj() })
                Chipy(
                    ZAKRESY.map { (k, n) -> k to if (k == "lista" && w.zaznaczona.isNotEmpty()) w.zaznaczona else n },
                    zakres,
                    aktywny = { it != "lista" || w.zaznaczona.isNotEmpty() },
                ) { zakres = it }
                if (stan.blad.isNotEmpty()) Text(stan.blad, style = SzkloType.BodySecondary)
                if (w.rodzaj.isEmpty()) {
                    if (stan.listy.isEmpty()) Text(stan.spisInfo.ifEmpty { "Łączenie ze Spotify…" }, style = SzkloType.BodySecondary)
                    Spis(stan.listy, spisScroll, ::otworz, Modifier.weight(1f))
                } else {
                    Wyniki(w, rodzaj, { rodzaj = it }, wSzkle, ::graj, ::otworz, { otworzWSpotify(ctx, it) }, Modifier.weight(1f))
                }
            } else {
                GlassCard {
                    Text("Najpierw zaloguj się do Spotify: włącz Spotify Connect i dokończ logowanie w przeglądarce.", style = SzkloType.Body)
                    if (!sp.running) PillButton("Włącz Spotify Connect", onClick = { SpotifyConnectService.start(ctx) }, primary = true)
                    else if (sp.loginUrl != null) {
                        PillButton("Otwórz logowanie", onClick = { otworzAdres(ctx, sp.loginUrl.orEmpty()) }, primary = true)
                    } else Text("Łączenie ze Spotify…", style = SzkloType.BodySecondary)
                }
                LaunchedEffect(sp.loginUrl) { sp.loginUrl?.let { otworzAdres(ctx, it) } }
            }
        }
    }
}

@Composable
private fun PoleSzukania(q: String, onChange: (String) -> Unit, onSearch: () -> Unit) {
    val shape = RoundedCornerShape(percent = 50)
    BasicTextField(
        value = q,
        onValueChange = { onChange(it.take(120)) },
        singleLine = true,
        textStyle = SzkloType.Body.copy(color = SzkloColors.Text),
        cursorBrush = SolidColor(SzkloColors.Accent),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = { onSearch() }),
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Szukaj w Spotify" },
        decorationBox = { pole ->
            Row(
                Modifier.fillMaxWidth().heightIn(min = 46.dp).clip(shape).background(SzkloColors.white(0.08f))
                    .border(1.dp, SzkloColors.white(0.16f), shape).padding(horizontal = 18.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.weight(1f)) {
                    if (q.isEmpty()) Text("Szukaj utworów, albumów, wykonawców", style = SzkloType.Body.copy(color = SzkloColors.text(0.6f)), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    pole()
                }
                if (q.isNotEmpty()) {
                    Box(
                        Modifier.size(40.dp).clip(shape).clickable(role = Role.Button) { onChange("") }.semantics { contentDescription = "Wyczyść" },
                        contentAlignment = Alignment.Center,
                    ) { Text("✕", style = SzkloType.Body) }
                }
            }
        },
    )
}

@Composable
private fun Chipy(pozycje: List<Pair<String, String>>, wybrany: String, aktywny: (String) -> Boolean = { true }, onSelect: (String) -> Unit) {
    val shape = RoundedCornerShape(percent = 50)
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        for ((k, n) in pozycje) {
            val on = k == wybrany
            val akt = aktywny(k)
            val b = Modifier.weight(1f).heightIn(min = 40.dp).clip(shape)
            Box(
                (if (on) b.background(SzkloColors.Accent) else b.background(SzkloColors.white(0.08f)).border(1.dp, SzkloColors.white(0.16f), shape))
                    .selectable(selected = on, enabled = akt, role = Role.Tab) { onSelect(k) }
                    .padding(horizontal = 8.dp, vertical = 9.dp),
                contentAlignment = Alignment.Center,
            ) {
                val st = if (on) SzkloType.ChipSelected else SzkloType.Chip
                Text(n, style = if (akt) st else st.copy(color = SzkloColors.text(0.4f)), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun Spis(listy: List<SpLista>, scroll: LazyListState, onOpen: (String) -> Unit, modifier: Modifier) {
    // [grupa albo lista]: nagłówki „Biblioteka”, „Podcasty”, „Folder: …”, „Playlisty”
    val wiersze = remember(listy) {
        buildList<Any> {
            var g = ""
            for (l in listy) {
                val n = when {
                    l.rodzaj == "podcast" -> "Podcasty"
                    l.rodzaj != "playlista" -> "Biblioteka"
                    l.folder.isNotEmpty() -> "Folder: ${l.folder}"
                    else -> "Playlisty"
                }
                if (n != g) {
                    g = n
                    add(n)
                }
                add(l)
            }
        }
    }
    LazyColumn(modifier.fillMaxWidth(), state = scroll) {
        items(wiersze) { x ->
            when (x) {
                is String -> Text(x, style = SzkloType.PresetLabel, modifier = Modifier.padding(top = 12.dp, bottom = 4.dp).semantics { heading() })
                is SpLista -> Wiersz(x.nazwa.ifEmpty { "(bez nazwy)" }, null, if (x.ile > 0) x.ile.toString() else "", { onOpen(x.uri) })
            }
        }
    }
}

@Composable
private fun Wiersz(tytul: String, pod: String?, prawo: String, onClick: () -> Unit, koniec: @Composable () -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp).clip(RoundedCornerShape(10.dp)).clickable(onClick = onClick).padding(horizontal = 4.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(tytul, style = SzkloType.Body, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (!pod.isNullOrEmpty()) Text(pod, style = SzkloType.BodySecondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (prawo.isNotEmpty()) Text(prawo, style = SzkloType.ModuleValue)
        koniec()
    }
}

@Composable
private fun Wyniki(
    w: SpWidok,
    rodzaj: String,
    onRodzaj: (String) -> Unit,
    wSzkle: Boolean,
    onGraj: (Int) -> Unit,
    onOpen: (String) -> Unit,
    onSpotify: (String) -> Unit,
    modifier: Modifier,
) {
    val ile = mapOf("utwory" to w.utwory.size, "albumy" to w.albumy.size, "wykonawcy" to w.wykonawcy.size)
    val pokazany = if ((ile[rodzaj] ?: 0) > 0) rodzaj else RODZAJE.firstOrNull { (ile[it.first] ?: 0) > 0 }?.first ?: rodzaj
    val dostepne = RODZAJE.filter { w.rodzaj == "szukaj" || (ile[it.first] ?: 0) > 0 }
    val adres = if (w.rodzaj == "szukaj") "spotify:search:" + Uri.encode(w.q) else w.uri.takeIf { it.startsWith("spotify:") }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(w.tytul, style = SzkloType.CardTitle, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            if (adres != null) PillButton("W aplikacji", onClick = { onSpotify(adres) }, primary = false)
        }
        if (dostepne.size > 1) Chipy(dostepne.map { (k, n) -> k to if ((ile[k] ?: 0) > 0) "$n ${ile[k]}" else n }, pokazany) { onRodzaj(it) }
        val info = when {
            w.info.isNotEmpty() -> w.info
            w.wczytywanie -> if (w.rodzaj == "szukaj") "Szukam…" else "Wczytywanie…"
            (ile[pokazany] ?: 0) == 0 -> "Pusto."
            pokazany == "utwory" -> if (wSzkle) "Stuknięcie gra przez pełny silnik Szkła." else "Stuknięcie gra w aplikacji Spotify."
            else -> ""
        }
        if (info.isNotEmpty()) Text(info, style = SzkloType.BodySecondary)
        val scroll = remember(w.uri, w.q, pokazany) { LazyListState() }
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = scroll) {
            when (pokazany) {
                "albumy" -> items(w.albumy) { a ->
                    Wiersz(a.nazwa, a.wykonawca, if (a.rok > 0) a.rok.toString() else "", { onOpen(a.uri) })
                }
                "wykonawcy" -> items(w.wykonawcy) { x -> Wiersz(x.nazwa, null, "", { onOpen(x.uri) }) }
                else -> itemsIndexed(w.utwory) { i, u ->
                    val kto = u.wykonawcy.joinToString(", ") { it.nazwa }
                    Wiersz(
                        u.tytul,
                        listOf(kto, u.album.nazwa).filter { it.isNotEmpty() }.distinct().joinToString(" · "),
                        if (u.czas > 0) Fmt.time((u.czas * 1000).toLong()) else "",
                        { onGraj(i) },
                    ) { MenuUtworu(u, { onSpotify(u.uri) }, onOpen) }
                }
            }
        }
    }
}

@Composable
private fun MenuUtworu(u: SpUtwor, onSpotify: () -> Unit, onOpen: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        Box(
            Modifier.size(40.dp).clip(RoundedCornerShape(percent = 50)).clickable(role = Role.Button) { open = true }.semantics { contentDescription = "Więcej: ${u.tytul}" },
            contentAlignment = Alignment.Center,
        ) { Text("⋮", style = SzkloType.CardTitle) }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(text = { Text("Zagraj w aplikacji Spotify") }, onClick = { open = false; onSpotify() })
            for (x in u.wykonawcy.filter { it.uri.startsWith("spotify:artist:") }.take(3)) {
                DropdownMenuItem(text = { Text("Wykonawca: ${x.nazwa}", maxLines = 1) }, onClick = { open = false; onOpen(x.uri) })
            }
            if (u.album.uri.startsWith("spotify:album:") || u.album.uri.startsWith("spotify:show:")) {
                DropdownMenuItem(
                    text = { Text((if (u.album.uri.startsWith("spotify:show:")) "Podcast: " else "Album: ") + u.album.nazwa, maxLines = 1) },
                    onClick = { open = false; onOpen(u.album.uri) },
                )
            }
        }
    }
}

private fun otworzAdres(ctx: Context, url: String): Boolean =
    runCatching { ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }.isSuccess

/** Aplikacja Spotify, a gdy jej nie ma – strona open.spotify.com. */
private fun otworzWSpotify(ctx: Context, uri: String) {
    val app = runCatching {
        ctx.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)).setPackage("com.spotify.music").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }.isSuccess
    if (!app) {
        val sciezka = if (uri == "spotify:collection") "collection/tracks" else uri.removePrefix("spotify:").replaceFirst(':', '/')
        otworzAdres(ctx, "https://open.spotify.com/$sciezka")
    }
}
