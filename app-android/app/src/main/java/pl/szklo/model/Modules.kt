package pl.szklo.model

import androidx.compose.runtime.Immutable

/** Grupy modułów w kolejności chipów (sekcja 12.4) – odpowiednik `catalog::Group` w Rust. */
enum class ModuleGroup(val label: String) {
    NAPRAWA("Naprawa"),
    DYNAMIKA("Dynamika"),
    BAS("Bas"),
    BARWA("Barwa"),
    STEREO("Stereo"),
    WYJSCIE("Wyjście"),
}

/**
 * Katalog 25 modułów – DOKŁADNIE w kolejności `core/src/catalog.rs` (`ModuleId` / `MODULES`).
 * `ordinal` = indeks przekazywany do `NativeEngine.setModule`. Nie zmieniać kolejności bez zmiany w Rust.
 * Nazwy w UI – sekcja 11.4 scenariusza (własne nazwy, bez nazw własnych wzorca).
 */
enum class ModuleId(val key: String, val label: String, val group: ModuleGroup) {
    DECLIPPER("declipper", "Declipper", ModuleGroup.NAPRAWA),
    DELOSSIFIER("delossifier", "Naprawa MP3", ModuleGroup.NAPRAWA),
    NOISE_REDUCTION("noise_reduction", "Szum i brum", ModuleGroup.NAPRAWA),
    DEHUMMER("dehummer", "Dehummer", ModuleGroup.NAPRAWA),
    DEQUANTIZER("dequantizer", "Dekwantyzer", ModuleGroup.NAPRAWA),
    NOISE_GATE("noise_gate", "Bramka szumów", ModuleGroup.NAPRAWA),
    AGC("agc", "AGC", ModuleGroup.DYNAMIKA),
    MULTIBAND1("multiband1", "Multiband 1", ModuleGroup.DYNAMIKA),
    MULTIBAND2("multiband2", "Multiband 2", ModuleGroup.DYNAMIKA),
    COMPRESSOR("compressor", "Kompresor", ModuleGroup.DYNAMIKA),
    PUNCH("punch", "Punch", ModuleGroup.DYNAMIKA),
    DEEP_BASS("deep_bass", "Głęboki bas", ModuleGroup.BAS),
    HARMONIC_BASS("harmonic_bass", "Bas harmoniczny", ModuleGroup.BAS),
    BASS_BOOST("bass_boost", "Bass Boost", ModuleGroup.BAS),
    WARM_BASS("warm_bass", "Ciepły bas", ModuleGroup.BAS),
    AUTO_EQ("auto_eq", "Auto EQ", ModuleGroup.BARWA),
    SPECTRUM_BALANCE("spectrum_balance", "Balans widma", ModuleGroup.BARWA),
    WIDTH("width", "Szerokość", ModuleGroup.STEREO),
    PHASE_ROTATION("phase_rotation", "Rotacja fazy", ModuleGroup.STEREO),
    CLIPPER_HD("clipper_hd", "Clipper HD", ModuleGroup.WYJSCIE),
    PRE_LIMITER("pre_limiter", "Pre-limiter", ModuleGroup.WYJSCIE),
    SIMPLE_CLIPPER("simple_clipper", "Prosty clipper", ModuleGroup.WYJSCIE),
    HARD_LIMIT("hard_limit", "Hard Limit", ModuleGroup.WYJSCIE),
    SPACE("space", "Przestrzeń", ModuleGroup.STEREO),
    RADIO_BASS("radio_bass", "Radiowy bas", ModuleGroup.BAS);

    val index: Int get() = ordinal

    companion object {
        const val COUNT = 25

        /** Moduły pogrupowane, w kolejności katalogu. */
        val byGroup: Map<ModuleGroup, List<ModuleId>> = entries.groupBy { it.group }

        fun fromKey(key: String): ModuleId? = entries.firstOrNull { it.key == key }

        init {
            check(entries.size == COUNT) { "Katalog modułów musi mieć $COUNT pozycje (jak core/src/catalog.rs)" }
        }
    }
}

/** Stan jednego modułu: włączony + moc 0..1 (UI pokazuje w %, krok 1). */
@Immutable
data class ModuleState(val on: Boolean, val amount: Float)

/**
 * Pełny stan parametrów przetwarzania (jedno źródło prawdy dla UI, odtwarzacza i mostu efektów).
 * [modules] zawsze ma [ModuleId.COUNT] elementów w kolejności katalogu.
 */
@Immutable
data class EngineParams(
    val modules: List<ModuleState>,
    /** Moc ogólna z pokrętła – mnożnik mocy modułów (poza Hard Limit), 0..1. */
    val master: Float,
    /** Przetwarzanie włączone (false = obejście). */
    val enabled: Boolean = true,
    /** Chwilowe porównanie z oryginałem (nie zapisywane). */
    val compare: Boolean = false,
    /** Sufit true-peak, dBTP (domyślnie −1,0 – Bluetooth, sekcja 4). */
    val ceilingDb: Float = -1.0f,
) {
    init {
        require(modules.size == ModuleId.COUNT) { "Oczekiwano ${ModuleId.COUNT} modułów, jest ${modules.size}" }
    }

    operator fun get(id: ModuleId): ModuleState = modules[id.index]

    /** Moc efektywna modułu po uwzględnieniu on/off i mocy ogólnej (jak `Engine::sync_params` w Rust). */
    fun effectiveAmount(id: ModuleId): Float {
        val m = modules[id.index]
        if (!m.on) return 0f
        return if (id == ModuleId.HARD_LIMIT) m.amount else m.amount * master
    }

    fun withModule(index: Int, transform: (ModuleState) -> ModuleState): EngineParams {
        if (index !in modules.indices) return this
        val updated = transform(modules[index])
        if (updated == modules[index]) return this
        return copy(modules = modules.toMutableList().also { it[index] = updated })
    }

    companion object {
        fun fromPreset(p: Preset) = EngineParams(modules = p.modules, master = p.master)
    }
}
