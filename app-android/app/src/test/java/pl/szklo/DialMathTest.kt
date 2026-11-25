package pl.szklo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import pl.szklo.audio.DpMapping
import pl.szklo.model.EngineParams
import pl.szklo.model.ModuleId
import pl.szklo.model.Presets
import pl.szklo.ui.components.DialMath
import pl.szklo.ui.components.besselI0
import pl.szklo.ui.components.blurredDiskProfile
import pl.szklo.ui.components.diskIntensity
import kotlin.math.exp

class DialMathTest {

    private val eps = 1e-4f

    @Test
    fun arcEndsMapToZeroAndOne() {
        assertEquals(0f, DialMath.valueAtStart(135f), eps) // lewy dół
        assertEquals(1f, DialMath.valueAtStart(45f), eps) // prawy dół (135 + 270 = 405 = 45)
        assertEquals(0.5f, DialMath.valueAtStart(270f), eps) // góra
    }

    @Test
    fun gapSnapsToNearestEnd() {
        assertEquals(1f, DialMath.valueAtStart(60f), eps) // prawa część luki
        assertEquals(0f, DialMath.valueAtStart(120f), eps) // lewa część luki
    }

    @Test
    fun noJumpAcrossGap() {
        // Z 98% palec wchodzi w lukę i wychodzi po stronie 0% – wartość zostaje na 100%.
        var v = 0.98f
        v = DialMath.nextValue(v, 60f)
        assertEquals(1f, v, eps)
        v = DialMath.nextValue(v, 130f)
        assertEquals(1f, v, eps)
        v = DialMath.nextValue(v, 140f) // tuż za początkiem łuku (~2%)
        assertEquals(1f, v, eps)
        // Powrót po stronie 100%.
        v = DialMath.nextValue(v, 40f)
        assertTrue(v > 0.95f)
    }

    @Test
    fun ticksEveryFivePercent() {
        assertEquals(13, DialMath.tick(0.68f))
        assertEquals(14, DialMath.tick(0.70f))
        assertEquals(20, DialMath.tick(1f))
    }

    @Test
    fun blurredDiskCenterMatchesClosedForm() {
        val r = 165.0
        val s = 90.0
        assertEquals(1 - exp(-r * r / (2 * s * s)), diskIntensity(0.0, r, s * s), 1e-6)
        assertEquals(1.0, besselI0(0.0), 1e-12)
        val p = blurredDiskProfile(r, s, r + s, 24)
        assertEquals(0f, p.last(), 1e-5f)
        for (i in 1 until p.size) assertTrue("profil malejący", p[i] <= p[i - 1] + 1e-6f)
    }

    @Test
    fun presetsMatchRustCatalog() {
        assertEquals(29, ModuleId.entries.size)
        assertEquals("hard_limit", ModuleId.entries[22].key)
        assertEquals(listOf("codzienny", "tramwaj", "stare", "noc"), Presets.all.map { it.key })
        // Porównanie z core/src/presets.rs (źródło prawdy) – wszystkie presety i moduły.
        val rs = java.io.File("../../core/src/presets.rs").takeIf { it.exists() }
            ?: java.io.File("../core/src/presets.rs")
        val text = rs.readText()
        val blok = Regex("""key: "(\w+)",\s*name_pl: "[^"]+",\s*modules: build\(&\[(.*?)\]\)""", RegexOption.DOT_MATCHES_ALL)
        val wpis = Regex("""\(M::(\w+), (true|false), (\d+)\)""")
        val znalezione = blok.findAll(text).toList()
        assertEquals(4, znalezione.size)
        for (b in znalezione) {
            val preset = Presets.byKey(b.groupValues[1]) ?: error("brak presetu ${b.groupValues[1]} w Kotlinie")
            for (w in wpis.findAll(b.groupValues[2])) {
                val nazwa = w.groupValues[1].replace(Regex("(?<=[a-z])(?=[A-Z])"), "_").uppercase()
                val id = ModuleId.valueOf(nazwa)
                val st = preset.modules[id.index]
                assertEquals("${preset.key}/$nazwa on", w.groupValues[2] == "true", st.on)
                assertEquals("${preset.key}/$nazwa moc", w.groupValues[3].toInt() / 100f, st.amount, eps)
            }
        }
    }

    @Test
    fun dpMappingNeutralWhenAllOff() {
        val off = EngineParams(
            modules = List(ModuleId.COUNT) { pl.szklo.model.ModuleState(false, 0.5f) },
            master = 1f,
        )
        val t = DpMapping.compute(off)
        assertTrue(t.preEq.all { it == 0f })
        assertTrue(t.postEq.all { it == 0f })
        assertTrue(t.mbc.all { it.ratio == 1f && it.postGainDb == 0f })
        assertEquals(false, t.limiter.enabled)
        assertEquals(0f, t.inputGainDb, eps)
    }
}
