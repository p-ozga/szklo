package pl.szklo.audio

import android.content.Context
import pl.szklo.model.EngineParams
import pl.szklo.model.ParamsCodec

/**
 * Trwały zapis ustawień w SharedPreferences (format tekstowy, odporny na uszkodzenia:
 * przy błędzie wracamy do presetu domyślnego). Porównanie z oryginałem nie jest zapisywane.
 */
internal class SettingsStore(context: Context) {

    private val prefs = context.getSharedPreferences("szklo", Context.MODE_PRIVATE)

    fun loadParams(): EngineParams? {
        val modules = ParamsCodec.decodeModules(prefs.getString(KEY_MODULES, null)) ?: return null
        return EngineParams(
            modules = modules,
            master = prefs.getFloat(KEY_MASTER, 0.68f).coerceIn(0f, 1f),
            enabled = prefs.getBoolean(KEY_ENABLED, true),
            ceilingDb = prefs.getFloat(KEY_CEILING, -1f).coerceIn(-12f, 0f),
        )
    }

    fun loadPresetKey(): String? = prefs.getString(KEY_PRESET, null)

    fun loadGlobalRequested(): Boolean = prefs.getBoolean(KEY_GLOBAL, true) // domyślnie włączone (Spotify)

    /** [now] = zapis synchroniczny (wyjście z aplikacji), inaczej w tle. */
    fun save(p: EngineParams, presetKey: String, now: Boolean = false) {
        val e = prefs.edit()
            .putString(KEY_MODULES, ParamsCodec.encodeModules(p.modules))
            .putFloat(KEY_MASTER, p.master)
            .putBoolean(KEY_ENABLED, p.enabled)
            .putFloat(KEY_CEILING, p.ceilingDb)
            .putString(KEY_PRESET, presetKey)
        if (now) e.commit() else e.apply()
    }

    /** Czy przy starcie aplikacji włączyć odbiornik Spotify Connect (był włączony przy zamknięciu). */
    fun loadSpotifyWanted(): Boolean = prefs.getBoolean(KEY_SPOTIFY, false)

    fun saveSpotifyWanted(on: Boolean) {
        prefs.edit().putBoolean(KEY_SPOTIFY, on).apply()
    }

    fun saveGlobalRequested(on: Boolean) {
        prefs.edit().putBoolean(KEY_GLOBAL, on).apply()
    }

    private companion object {
        const val KEY_MODULES = "modules_v1"
        const val KEY_MASTER = "master"
        const val KEY_ENABLED = "enabled"
        const val KEY_CEILING = "ceiling_dbtp"
        const val KEY_PRESET = "preset"
        const val KEY_GLOBAL = "global_effects"
        const val KEY_SPOTIFY = "spotify_connect"
    }
}
