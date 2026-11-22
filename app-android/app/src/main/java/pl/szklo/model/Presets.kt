package pl.szklo.model

import pl.szklo.model.ModuleId as M

/**
 * Własny preset. [modules] – odwzorowanie 1:1 `core/src/presets.rs` (te same klucze i wartości).
 * [master] – wartość mocy ogólnej dla pokrętła (podwójne stuknięcie = powrót do niej). W Rust presety
 * nie mają mocy ogólnej (rdzeń dostaje ją osobno przez `setMaster`), więc ta liczba żyje tylko w Kotlinie;
 * wartości wstępne do strojenia odsłuchem (faza 5).
 */
data class Preset(
    val key: String,
    val name: String,
    val master: Float,
    val modules: List<ModuleState>,
)

object Presets {

    private fun build(vararg entries: Triple<M, Boolean, Int>): List<ModuleState> {
        // Domyślnie jak `build` w presets.rs: (false, 0.5) dla modułów spoza listy.
        val out = MutableList(M.COUNT) { ModuleState(on = false, amount = 0.5f) }
        for ((id, on, pct) in entries) out[id.index] = ModuleState(on, pct / 100f)
        return out
    }

    private fun m(id: M, on: Boolean, pct: Int) = Triple(id, on, pct)

    val CODZIENNY = Preset(
        key = "codzienny", name = "Codzienny", master = 0.68f,
        modules = build(
            m(M.DECLIPPER, true, 40), m(M.DELOSSIFIER, true, 40), m(M.NOISE_REDUCTION, false, 0),
            m(M.DEHUMMER, false, 0), m(M.DEQUANTIZER, true, 50), m(M.NOISE_GATE, false, 0),
            m(M.AGC, true, 50), m(M.MULTIBAND1, true, 30), m(M.MULTIBAND2, true, 35),
            m(M.COMPRESSOR, false, 0), m(M.PUNCH, true, 25), m(M.DEEP_BASS, true, 35),
            m(M.HARMONIC_BASS, true, 30), m(M.BASS_BOOST, false, 0), m(M.WARM_BASS, true, 50),
            m(M.AUTO_EQ, true, 40), m(M.SPECTRUM_BALANCE, true, 25), m(M.WIDTH, true, 20),
            m(M.PHASE_ROTATION, false, 0), m(M.CLIPPER_HD, true, 25), m(M.PRE_LIMITER, true, 30),
            m(M.SIMPLE_CLIPPER, false, 0), m(M.HARD_LIMIT, true, 0),
        ),
    )

    val TRAMWAJ = Preset(
        key = "tramwaj", name = "Głośno w tramwaju", master = 0.85f,
        modules = build(
            m(M.DECLIPPER, true, 30), m(M.DELOSSIFIER, true, 40), m(M.NOISE_REDUCTION, false, 0),
            m(M.DEHUMMER, false, 0), m(M.DEQUANTIZER, false, 0), m(M.NOISE_GATE, false, 0),
            m(M.AGC, true, 90), m(M.MULTIBAND1, true, 75), m(M.MULTIBAND2, true, 70),
            m(M.COMPRESSOR, true, 50), m(M.PUNCH, true, 20), m(M.DEEP_BASS, false, 0),
            m(M.HARMONIC_BASS, true, 70), m(M.BASS_BOOST, true, 30), m(M.WARM_BASS, true, 40),
            m(M.AUTO_EQ, true, 50), m(M.SPECTRUM_BALANCE, true, 40), m(M.WIDTH, true, 10),
            m(M.PHASE_ROTATION, true, 60), m(M.CLIPPER_HD, true, 75), m(M.PRE_LIMITER, true, 60),
            m(M.SIMPLE_CLIPPER, false, 0), m(M.HARD_LIMIT, true, 30),
        ),
    )

    val STARE = Preset(
        key = "stare", name = "Stare nagrania", master = 0.60f,
        modules = build(
            m(M.DECLIPPER, true, 70), m(M.DELOSSIFIER, true, 60), m(M.NOISE_REDUCTION, true, 55),
            m(M.DEHUMMER, true, 70), m(M.DEQUANTIZER, true, 70), m(M.NOISE_GATE, true, 35),
            m(M.AGC, true, 60), m(M.MULTIBAND1, true, 25), m(M.MULTIBAND2, true, 45),
            m(M.COMPRESSOR, false, 0), m(M.PUNCH, true, 40), m(M.DEEP_BASS, true, 30),
            m(M.HARMONIC_BASS, true, 45), m(M.BASS_BOOST, true, 20), m(M.WARM_BASS, true, 55),
            m(M.AUTO_EQ, true, 80), m(M.SPECTRUM_BALANCE, true, 45), m(M.WIDTH, true, 15),
            m(M.PHASE_ROTATION, false, 0), m(M.CLIPPER_HD, true, 20), m(M.PRE_LIMITER, true, 30),
            m(M.SIMPLE_CLIPPER, false, 0), m(M.HARD_LIMIT, true, 0),
        ),
    )

    val NOC = Preset(
        key = "noc", name = "Noc", master = 0.50f,
        modules = build(
            m(M.DECLIPPER, true, 40), m(M.DELOSSIFIER, true, 40), m(M.NOISE_REDUCTION, false, 0),
            m(M.DEHUMMER, false, 0), m(M.DEQUANTIZER, true, 60), m(M.NOISE_GATE, false, 0),
            m(M.AGC, true, 80), m(M.MULTIBAND1, true, 40), m(M.MULTIBAND2, true, 60),
            m(M.COMPRESSOR, true, 60), m(M.PUNCH, false, 0), m(M.DEEP_BASS, false, 0),
            m(M.HARMONIC_BASS, true, 30), m(M.BASS_BOOST, true, 25), m(M.WARM_BASS, true, 45),
            m(M.AUTO_EQ, true, 40), m(M.SPECTRUM_BALANCE, true, 50), m(M.WIDTH, true, 10),
            m(M.PHASE_ROTATION, false, 0), m(M.CLIPPER_HD, false, 0), m(M.PRE_LIMITER, true, 50),
            m(M.SIMPLE_CLIPPER, true, 20), m(M.HARD_LIMIT, true, 0),
        ),
    )

    /** Kolejność w menu. */
    val all: List<Preset> = listOf(CODZIENNY, TRAMWAJ, STARE, NOC)

    val default: Preset get() = CODZIENNY

    fun byKey(key: String?): Preset? = all.firstOrNull { it.key == key }
}
