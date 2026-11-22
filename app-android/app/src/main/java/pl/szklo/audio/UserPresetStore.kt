package pl.szklo.audio

import android.content.Context
import pl.szklo.model.ParamsCodec
import pl.szklo.model.Preset

/**
 * Presety użytkownika w osobnym pliku SharedPreferences („szklo_presety”).
 * Lista kluczy w `lista` (po jednym w linii), dla każdego: `n.<klucz>` nazwa, `m.<klucz>` moc ogólna, `s.<klucz>` moduły.
 * Uszkodzony wpis jest pomijany, reszta wczytuje się normalnie.
 */
internal class UserPresetStore(context: Context) {

    private val prefs = context.getSharedPreferences("szklo_presety", Context.MODE_PRIVATE)

    fun load(): List<Preset> {
        val keys = prefs.getString(KEY_LIST, null)?.split('\n')?.filter { it.isNotBlank() } ?: return emptyList()
        return keys.mapNotNull { k ->
            val name = prefs.getString("n.$k", null)?.takeIf { it.isNotBlank() } ?: return@mapNotNull null
            val modules = ParamsCodec.decodeModules(prefs.getString("s.$k", null)) ?: return@mapNotNull null
            val master = prefs.getFloat("m.$k", 0.68f).takeIf { it.isFinite() }?.coerceIn(0f, 1f) ?: 0.68f
            Preset(key = k, name = name, master = master, modules = modules)
        }
    }

    /** Zapisuje całą listę (kolejność = kolejność w menu). Zapis synchroniczny – preset nie może zginąć. */
    fun saveAll(presets: List<Preset>) {
        val e = prefs.edit().clear()
        e.putString(KEY_LIST, presets.joinToString("\n") { it.key })
        for (p in presets) {
            e.putString("n.${p.key}", p.name)
            e.putFloat("m.${p.key}", p.master)
            e.putString("s.${p.key}", ParamsCodec.encodeModules(p.modules))
        }
        e.commit()
    }

    private companion object {
        const val KEY_LIST = "lista"
    }
}
