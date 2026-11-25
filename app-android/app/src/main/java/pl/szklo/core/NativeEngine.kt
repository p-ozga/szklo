package pl.szklo.core

import android.util.Log
import java.util.concurrent.locks.ReentrantReadWriteLock
import kotlin.concurrent.read
import kotlin.concurrent.write

/**
 * Most JNI do rdzenia DSP Szkła (crate `szklo-jni`, biblioteka `libszklo_jni.so`).
 *
 * Nazwy funkcji natywnych muszą odpowiadać eksportom `Java_pl_szklo_core_NativeEngine_*`
 * z `app-android/rust/src/lib.rs` (metody statyczne tej klasy – drugi parametr JNI to `jclass`).
 *
 * Bezpieczeństwo wątków:
 * - setery i [readMeters] mogą być wołane z dowolnego wątku (rdzeń trzyma parametry i mierniki w atomikach),
 * - [process] woła wyłącznie wątek audio,
 * - [close] zwalnia uchwyt pod blokadą zapisu, więc żadne wywołanie natywne nie trafi w zwolnioną pamięć.
 *
 * Blokada odczytu jest praktycznie bezkosztowa (brak rywalizacji poza chwilą zamykania).
 */
class NativeEngine private constructor(
    private var handle: Long,
    /** Częstotliwość próbkowania, z którą utworzono silnik (Hz). */
    val sampleRate: Int,
) : AutoCloseable {

    private val lock = ReentrantReadWriteLock()

    val isClosed: Boolean get() = lock.read { handle == 0L }

    /** Moduł [index] (0..22, kolejność jak `ModuleId`): włączony + moc 0..1. */
    fun setModule(index: Int, on: Boolean, amount: Float) = lock.read {
        if (handle != 0L) nativeSetModule(handle, index, on, amount)
    }

    /** Moc ogólna (pokrętło) – mnożnik 0..1 mocy wszystkich modułów poza Hard Limit. */
    fun setMaster(master: Float) = lock.read {
        if (handle != 0L) nativeSetMaster(handle, master)
    }

    /** Globalne włączenie przetwarzania (false = obejście całego toru). */
    fun setEnabled(on: Boolean) = lock.read {
        if (handle != 0L) nativeSetEnabled(handle, on)
    }

    /** „Porównaj z oryginałem” – true = chwilowe obejście. */
    fun setCompare(hold: Boolean) = lock.read {
        if (handle != 0L) nativeSetCompare(handle, hold)
    }

    /** Sufit wyjścia w dBTP (rdzeń ogranicza do −12…0). */
    fun setCeiling(dbtp: Float) = lock.read {
        if (handle != 0L) nativeSetCeiling(handle, dbtp)
    }

    /** Wczytuje preset po kluczu (`codzienny`, `tramwaj`, `stare`, `noc`). Zwraca false dla nieznanego klucza. */
    fun applyPreset(key: String): Boolean = lock.read {
        handle != 0L && nativeApplyPreset(handle, key)
    }

    /**
     * Przetwarza w miejscu bufor z przeplotem (L R L R …). [frames] × [channels] ≤ `buf.size`.
     * Mono/wielokanał: rdzeń przetwarza dwa pierwsze kanały.
     */
    fun process(buf: FloatArray, frames: Int, channels: Int) = lock.read {
        if (handle != 0L) nativeProcess(handle, buf, frames, channels)
    }

    /**
     * Fala wyjścia dla wizualizacji: [on] włącza podgląd w rdzeniu (false – wyłącza), [out] (min. [SCOPE_BYTES]) dostaje
     * 1024 próbki lewego i 1024 prawego kanału (8 bitów, środek 128). Zwraca true tylko wtedy, gdy przyszedł nowy dźwięk.
     */
    fun scope(on: Boolean, out: ByteArray): Boolean = lock.read {
        handle != 0L && out.size >= SCOPE_BYTES && nativeScope(handle, on, out)
    }

    /**
     * Odczyt mierników do [out] (min. [METER_COUNT] elementów):
     * [METER_IN_DB], [METER_OUT_DB], [METER_GR_DB], [METER_LUFS_3S], [METER_TRUE_PEAK_DB].
     * Zwraca false, gdy silnik jest już zamknięty.
     */
    fun readMeters(out: FloatArray): Boolean {
        require(out.size >= METER_COUNT) { "Bufor mierników za krótki" }
        return lock.read {
            if (handle == 0L) false else {
                nativeMeters(handle, out)
                true
            }
        }
    }

    /** Opóźnienie toru (lookahead) w ramkach. */
    fun latencyFrames(): Int = lock.read {
        if (handle == 0L) 0 else nativeLatency(handle)
    }

    /**
     * Start odbiornika Spotify Connect „Szkło” na parametrach i miernikach tego silnika.
     * Stan: [spotifyStatus]. [cacheDir] – katalog prywatny aplikacji (poświadczenia Spotify).
     */
    fun spotifyStart(cacheDir: String, name: String) = lock.read {
        if (handle != 0L) nativeSpotifyStart(handle, cacheDir, name)
    }

    override fun close() {
        lock.write {
            if (handle != 0L) {
                nativeDestroy(handle)
                handle = 0L
            }
        }
    }

    companion object {
        private const val TAG = "NativeEngine"

        const val METER_COUNT = 5
        const val METER_IN_DB = 0
        const val METER_OUT_DB = 1
        const val METER_GR_DB = 2
        const val METER_LUFS_3S = 3
        const val METER_TRUE_PEAK_DB = 4

        /**
         * true, gdy udało się załadować `libszklo_jni.so`. Brak biblioteki (np. emulator x86_64 bez
         * zbudowanego .so) nie wywraca aplikacji – UI działa, odtwarzacz gra bez obróbki.
         */
        val isAvailable: Boolean by lazy {
            try {
                System.loadLibrary("szklo_jni")
                true
            } catch (e: UnsatisfiedLinkError) {
                Log.w(TAG, "Brak biblioteki libszklo_jni.so – rdzeń DSP niedostępny", e)
                false
            } catch (e: SecurityException) {
                Log.w(TAG, "Nie można załadować libszklo_jni.so", e)
                false
            }
        }

        /** Tworzy silnik dla [sampleRate] Hz albo zwraca null, gdy rdzeń jest niedostępny. */
        fun create(sampleRate: Int): NativeEngine? {
            if (!isAvailable) return null
            val h = nativeCreate(sampleRate)
            return if (h == 0L) null else NativeEngine(h, sampleRate)
        }

        // Deklaracje natywne – publiczne celowo: `private`/`internal` zmieniają nazwę lub miejsce metody
        // statycznej na JVM (internal = nazwa z sufiksem modułu), a JNI szuka dokładnie
        // `pl.szklo.core.NativeEngine.nativeXxx`. Poza tą klasą używać wyłącznie API wyżej.
        @JvmStatic external fun nativeCreate(sampleRate: Int): Long
        @JvmStatic external fun nativeDestroy(h: Long)
        @JvmStatic external fun nativeSetModule(h: Long, idx: Int, on: Boolean, amount: Float)
        @JvmStatic external fun nativeSetMaster(h: Long, master: Float)
        @JvmStatic external fun nativeSetEnabled(h: Long, on: Boolean)
        @JvmStatic external fun nativeSetCompare(h: Long, hold: Boolean)
        @JvmStatic external fun nativeSetCeiling(h: Long, db: Float)
        @JvmStatic external fun nativeApplyPreset(h: Long, key: String): Boolean
        @JvmStatic external fun nativeProcess(h: Long, buf: FloatArray, frames: Int, channels: Int)
        @JvmStatic external fun nativeMeters(h: Long, out: FloatArray)
        @JvmStatic external fun nativeScope(h: Long, on: Boolean, out: ByteArray): Boolean
        const val SCOPE_BYTES = 2048
        @JvmStatic external fun nativeLatency(h: Long): Int
        @JvmStatic external fun nativeInitAndroid(context: android.content.Context)
        @JvmStatic external fun nativeSpotifyStart(h: Long, cacheDir: String, name: String)
        @JvmStatic external fun nativeSpotifyStop()
        @JvmStatic external fun nativeSpotifyLogout(cacheDir: String)
        /** `idle | connecting | login <url> | ready | playing | error <opis>` */
        @JvmStatic external fun nativeSpotifyStatus(): String
        /** Polecenie biblioteki Spotify (JSON). */
        @JvmStatic external fun nativeSpotifyLib(cacheDir: String, cmd: String)
        /** Stan biblioteki (JSON) albo "", gdy od wersji [ws]/[ww] nic się nie zmieniło. */
        @JvmStatic external fun nativeSpotifyLibState(cacheDir: String, ws: Long, ww: Long): String
        /** Gra utwory (JSON: lista adresów) przez odbiornik „Szkło”. */
        @JvmStatic external fun nativeSpotifyPlay(uris: String): Boolean
        /** Ostatnie linie dziennika Spotify (rdzeń). */
        @JvmStatic external fun nativeSpotifyLog(): String
    }
}
