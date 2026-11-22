package pl.szklo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pl.szklo.model.EngineParams
import pl.szklo.model.ModuleState
import pl.szklo.model.ParamsCodec
import pl.szklo.model.Presets

class ParamsCodecTest {

    @Test
    fun roundTripKeepsEveryPreset() {
        for (p in Presets.all) {
            val decoded = ParamsCodec.decodeModules(ParamsCodec.encodeModules(p.modules))
            assertEquals(p.key, p.modules, decoded)
        }
    }

    @Test
    fun damagedDataIsRejected() {
        assertNull(ParamsCodec.decodeModules(null))
        assertNull(ParamsCodec.decodeModules(""))
        assertNull(ParamsCodec.decodeModules("1:0.5;0:0.2")) // za mało modułów
        val ok = ParamsCodec.encodeModules(Presets.default.modules)
        assertNull(ParamsCodec.decodeModules(ok.replaceFirst(":", ":x")))
        assertNull(ParamsCodec.decodeModules(ok.replaceFirst("1:", "7:")))
    }

    @Test
    fun matchesDetectsChanges() {
        val p = Presets.default
        val params = EngineParams.fromPreset(p)
        assertTrue(ParamsCodec.matches(params, p))
        assertTrue(ParamsCodec.matches(params.copy(master = p.master + 0.004f), p))
        assertFalse(ParamsCodec.matches(params.copy(master = p.master + 0.02f), p))
        val changed = params.withModule(14) { ModuleState(!it.on, it.amount) }
        assertFalse(ParamsCodec.matches(changed, p))
    }
}
