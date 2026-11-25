package pl.szklo

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.HandlerThread
import android.provider.MediaStore
import android.util.Log
import androidx.annotation.RequiresApi
import java.io.File
import java.io.FileOutputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Dziennik aplikacji.
 *
 * Wszystko, co aplikacja zapisuje do logcat (Kotlin, rdzeń Rust, librespot, błędy Androida dotyczące tego procesu),
 * trafia do pliku wewnętrznego `files/logi/szklo-RRRR-MM-DD.txt`. Własny logcat procesu można czytać bez uprawnień.
 * Awarie są dopisywane ze śladem stosu. Trzymamy 7 ostatnich dni, najwyżej [MAX_BAJTOW] na dzień.
 *
 * Zapis do folderu dostępnego dla użytkownika: `Dokumenty/Szklo` przez MediaStore (Android 10+, bez uprawnień).
 * Nazwa konta Spotify jest zamazywana przed zapisem.
 */
object Dziennik {

    private const val TAG = "SzkloDziennik"
    private const val MAX_BAJTOW = 4_000_000L
    private const val DNI = 7
    private const val CO_ILE_MS = 30_000L
    const val FOLDER = "Dokumenty/Szklo"
    private val SCIEZKA = "${Environment.DIRECTORY_DOCUMENTS}/Szklo/"

    private lateinit var app: Context
    private lateinit var watek: Handler
    private val blokada = Any()
    private var zapisanoPrzyDlugosci = -1L
    private var zapisanoPlik = ""

    private val prefs get() = app.getSharedPreferences("szklo_dziennik", Context.MODE_PRIVATE)

    /** Zapis do Dokumentów działa od Androida 10 (MediaStore bez uprawnień). */
    val dostepny: Boolean get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q

    /** Automatyczny zapis do `Dokumenty/Szklo` (co 30 s i przy zamykaniu aplikacji). */
    var automatyczny: Boolean
        get() = prefs.getBoolean("auto", false)
        set(v) {
            prefs.edit().putBoolean("auto", v).apply()
            Log.i(TAG, "Zapis dziennika do $FOLDER: ${if (v) "włączony" else "wyłączony"}")
            if (v) zapiszWTle()
        }

    fun start(context: Context) {
        app = context.applicationContext
        val t = HandlerThread("szklo-dziennik").apply { start() }
        watek = Handler(t.looper)
        przechwycAwarie()
        Thread({ czytajLogcat() }, "szklo-logcat").apply { isDaemon = true }.start()
        val wersja = runCatching { app.packageManager.getPackageInfo(app.packageName, 0).versionName }.getOrNull()
        Log.i(TAG, "Szkło $wersja start · ${Build.MANUFACTURER} ${Build.MODEL} · Android ${Build.VERSION.RELEASE}")
        watek.postDelayed(object : Runnable {
            override fun run() {
                if (automatyczny) runCatching { zapiszTeraz(tylkoZmiany = true) }
                watek.postDelayed(this, CO_ILE_MS)
            }
        }, CO_ILE_MS)
    }

    /** Zapis w tle, jeśli włączony automatyczny (np. przy chowaniu aplikacji). */
    fun zapiszJesliAutomatyczny() {
        if (automatyczny) zapiszWTle()
    }

    private fun zapiszWTle() {
        watek.postDelayed({ runCatching { zapiszTeraz(tylkoZmiany = true) } }, 300)
    }

    /**
     * Kopiuje dzisiejszy dziennik (przy zapisie ręcznym – wszystkie dni) do `Dokumenty/Szklo`.
     * Zwraca komunikat dla użytkownika. Nie wołać z wątku głównego.
     */
    fun zapiszTeraz(tylkoZmiany: Boolean = false): String {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return "Zapis do Dokumentów wymaga Androida 10"
        return try {
            synchronized(blokada) {
                val pliki = if (tylkoZmiany) listOf(dzisiejszy()) else katalog().listFiles()?.sortedBy { it.name }.orEmpty()
                val niepuste = pliki.filter { it.exists() && it.length() > 0 }
                if (niepuste.isEmpty()) return "Dziennik jest jeszcze pusty"
                for (p in niepuste) {
                    if (tylkoZmiany && p.name == zapisanoPlik && p.length() == zapisanoPrzyDlugosci) continue
                    wyslij(p)
                    zapisanoPlik = p.name
                    zapisanoPrzyDlugosci = p.length()
                }
                "Zapisano: $FOLDER (${niepuste.joinToString { it.name }})"
            }
        } catch (e: Exception) {
            Log.w(TAG, "Nie udało się zapisać dziennika do Dokumentów", e)
            "Nie udało się zapisać: ${e.message ?: e.javaClass.simpleName}"
        }
    }

    @RequiresApi(Build.VERSION_CODES.Q)
    private fun wyslij(plik: File) {
        val cr = app.contentResolver
        val zbior = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val istniejacy = cr.query(
            zbior,
            arrayOf(MediaStore.MediaColumns._ID),
            "${MediaStore.MediaColumns.RELATIVE_PATH}=? AND ${MediaStore.MediaColumns.DISPLAY_NAME}=?",
            arrayOf(SCIEZKA, plik.name),
            null,
        )?.use { c -> if (c.moveToFirst()) ContentUris.withAppendedId(zbior, c.getLong(0)) else null }
        val uri = istniejacy ?: cr.insert(
            zbior,
            ContentValues().apply {
                put(MediaStore.MediaColumns.DISPLAY_NAME, plik.name)
                put(MediaStore.MediaColumns.MIME_TYPE, "text/plain")
                put(MediaStore.MediaColumns.RELATIVE_PATH, SCIEZKA)
            },
        ) ?: error("system nie utworzył pliku")
        val wyjscie = cr.openOutputStream(uri, "wt") ?: error("system nie otworzył pliku do zapisu")
        wyjscie.use { o -> plik.inputStream().use { it.copyTo(o) } }
    }

    // ───────────────────────── plik wewnętrzny ─────────────────────────

    private fun katalog() = File(app.filesDir, "logi").apply { mkdirs() }

    private fun dzisiejszy() = File(katalog(), "szklo-${SimpleDateFormat("yyyy-MM-dd", Locale.ROOT).format(Date())}.txt")

    private fun usunStare() {
        katalog().listFiles()?.sortedByDescending { it.name }?.drop(DNI)?.forEach { it.delete() }
    }

    /** Czyta logcat własnego procesu i dopisuje do pliku; po przerwaniu próbuje ponownie. */
    private fun czytajLogcat() {
        var pierwszy = true
        while (true) {
            try {
                usunStare()
                // po przerwaniu czytamy tylko nowe wpisy (-T 1), inaczej cały bufor trafiał do pliku drugi raz
                val arg = mutableListOf("logcat", "-v", "threadtime", "--pid=${android.os.Process.myPid()}")
                if (!pierwszy) arg += listOf("-T", "1")
                arg += "*:I"
                pierwszy = false
                val p = ProcessBuilder(arg).redirectErrorStream(true).start()
                p.inputStream.bufferedReader().useLines { linie -> linie.forEach { dopisz(it) } }
            } catch (e: Exception) {
                dopisz("(dziennik: logcat niedostępny: ${e.message})")
            }
            Thread.sleep(10_000)
        }
    }

    private fun dopisz(linia: String) {
        runCatching {
            synchronized(blokada) {
                val f = dzisiejszy()
                if (f.length() < MAX_BAJTOW) FileOutputStream(f, true).use { it.write((zamaz(linia) + "\n").toByteArray()) }
            }
        }
    }

    /** Nazwa konta Spotify nie trafia do pliku w Dokumentach. */
    internal fun zamaz(linia: String): String = linia
        .replace(KONTO, "$1$2***$3")
        .replace(ADRES_LOGOWANIA, "$1***")

    private val KONTO = Regex("""(Authenticated as |zalogowano[^'"(]*|user_?name[ =:]+)(['"(])[^'")]+(['")])""")
    private val ADRES_LOGOWANIA = Regex("""(login https?://[^?\s]+\?)\S+""")

    private fun przechwycAwarie() {
        val poprzedni = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            runCatching {
                dopisz("AWARIA w wątku ${t.name}: ${Log.getStackTraceString(e)}")
                if (automatyczny) zapiszTeraz(tylkoZmiany = true)
            }
            poprzedni?.uncaughtException(t, e)
        }
    }
}
