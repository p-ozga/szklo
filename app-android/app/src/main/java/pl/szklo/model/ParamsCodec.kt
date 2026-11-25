package pl.szklo.model

import java.util.Locale
import kotlin.math.abs

/**
 * Tekstowy zapis stanu modułów: `on:moc;on:moc;…` (29 pozycji w kolejności katalogu), np. `1:0.5000;0:0.0000;…`.
 * Wspólny dla bieżących ustawień i presetów użytkownika. Odczyt jest odporny na uszkodzenia (null zamiast wyjątku).
 */
object ParamsCodec {

    fun encodeModules(modules: List<ModuleState>): String =
        modules.joinToString(";") { m -> (if (m.on) "1" else "0") + ":" + String.format(Locale.ROOT, "%.4f", m.amount) }

    fun decodeModules(raw: String?): List<ModuleState>? {
        if (raw.isNullOrEmpty()) return null
        val parts = raw.split(';')
        // zapis sprzed dodania modułów (od 23 pozycji): brakujące = wyłączone
        if (parts.size < MIN_COUNT || parts.size > ModuleId.COUNT) return null
        val read = parts.map { p ->
            val kv = p.split(':')
            if (kv.size != 2 || (kv[0] != "0" && kv[0] != "1")) return null
            val a = kv[1].toFloatOrNull()?.takeIf { it.isFinite() }?.coerceIn(0f, 1f) ?: return null
            ModuleState(kv[0] == "1", a)
        }
        return read + List(ModuleId.COUNT - read.size) { ModuleState(on = false, amount = 0.5f) }
    }

    /** Czy bieżące ustawienia odpowiadają presetowi (tolerancja pół kroku suwaka). */
    fun matches(p: EngineParams, preset: Preset): Boolean {
        if (abs(p.master - preset.master) > TOL) return false
        return p.modules.indices.all { i ->
            val a = p.modules[i]
            val b = preset.modules[i]
            a.on == b.on && abs(a.amount - b.amount) <= TOL
        }
    }

    private const val MIN_COUNT = 23
    private const val TOL = 0.005f
}
