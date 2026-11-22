package pl.szklo.audio

import pl.szklo.model.EngineParams
import pl.szklo.model.ModuleId

/**
 * PRZYBLIŻENIE naszych modułów etapami systemowego `android.media.audiofx.DynamicsProcessing`
 * (preEQ → MBC → postEQ → limiter, każdy kanał osobno).
 *
 * Dlaczego tylko przybliżenie: własny DSP w Rust (declipper, delossifier, clipper z kasowaniem
 * zniekształceń, psychoakustyczny bas, M/S, rotacja fazy, true-peak 4×…) NIE może wejść w strumień
 * innej aplikacji bez roota – Android pozwala jedynie dołączyć wbudowane efekty systemowe do sesji audio
 * (szczegóły: spec/android_przetwarzanie_innych_aplikacji.md). Mapowanie poniżej jest wstępne:
 * liczby to punkty startowe do strojenia odsłuchem, nie wynik pomiarów.
 *
 * Moduły bez odpowiednika (brak wpływu w tym trybie): Declipper, Dekwantyzer, Szerokość, Rotacja fazy.
 *
 * Czysta funkcja (bez API Androida) – testowalna na JVM.
 */
object DpMapping {

    /** Górne krawędzie pasm preEQ (Hz). */
    val PRE_EQ_CUTOFFS = floatArrayOf(60f, 150f, 400f, 1000f, 2500f, 6000f, 12000f, 20000f)

    /** Górne krawędzie pasm kompresora wielopasmowego (Hz). */
    val MBC_CUTOFFS = floatArrayOf(150f, 1000f, 5000f, 20000f)

    /** Górne krawędzie pasm postEQ (Hz). */
    val POST_EQ_CUTOFFS = floatArrayOf(100f, 1000f, 8000f, 20000f)

    data class Mbc(
        val attackMs: Float,
        val releaseMs: Float,
        val ratio: Float,
        val thresholdDb: Float,
        val kneeDb: Float,
        val noiseGateDb: Float,
        val expanderRatio: Float,
        val preGainDb: Float,
        val postGainDb: Float,
    )

    data class Limiter(
        val enabled: Boolean,
        val attackMs: Float,
        val releaseMs: Float,
        val ratio: Float,
        val thresholdDb: Float,
        val postGainDb: Float,
    )

    data class Targets(
        val enabled: Boolean,
        val inputGainDb: Float,
        val preEq: FloatArray,
        val mbc: List<Mbc>,
        val postEq: FloatArray,
        val limiter: Limiter,
    ) {
        // FloatArray nie ma równości strukturalnej – porównujemy zawartość (unikamy zbędnych wywołań do audioserwera).
        override fun equals(other: Any?): Boolean = other is Targets &&
            enabled == other.enabled && inputGainDb == other.inputGainDb &&
            preEq.contentEquals(other.preEq) && mbc == other.mbc &&
            postEq.contentEquals(other.postEq) && limiter == other.limiter

        override fun hashCode(): Int {
            var h = enabled.hashCode()
            h = 31 * h + inputGainDb.hashCode()
            h = 31 * h + preEq.contentHashCode()
            h = 31 * h + mbc.hashCode()
            h = 31 * h + postEq.contentHashCode()
            h = 31 * h + limiter.hashCode()
            return h
        }
    }

    fun compute(p: EngineParams): Targets {
        fun a(id: ModuleId) = p.effectiveAmount(id)

        // ---------------- preEQ: Bas + Barwa (+ lekka korekta od Naprawy)
        val pre = FloatArray(PRE_EQ_CUTOFFS.size)
        // Głęboki bas: podbicie poniżej ~60 Hz (prawdziwy moduł to dopasowany filtr półkowy z ochroną przed przesterem).
        pre[0] += 6f * a(ModuleId.DEEP_BASS)
        pre[1] += 2f * a(ModuleId.DEEP_BASS)
        // Bas harmoniczny: harmonicznych nie da się wytworzyć EQ – zastępczo podbicie 60–150 Hz.
        pre[1] += 4f * a(ModuleId.HARMONIC_BASS)
        // Bass Boost: szeroka półka basowa.
        pre[0] += 4f * a(ModuleId.BASS_BOOST)
        pre[1] += 4f * a(ModuleId.BASS_BOOST)
        pre[2] += 1.5f * a(ModuleId.BASS_BOOST)
        // Ciepły bas: nasycenia nie da się zrobić EQ – zastępczo wyraźne podbicie dołu i dolnego środka (ciepło),
        // z lekkim ujęciem 2,5–6 kHz (miękkość). Wzmocnione 2026-11-22: w Spotify różnica była niesłyszalna.
        pre[1] += 3f * a(ModuleId.WARM_BASS)
        pre[2] += 2f * a(ModuleId.WARM_BASS)
        pre[4] -= 1f * a(ModuleId.WARM_BASS)
        // Auto EQ: statyczne przybliżenie – mniej „buczenia”, więcej obecności.
        pre[2] -= 1f * a(ModuleId.AUTO_EQ)
        pre[4] += 1f * a(ModuleId.AUTO_EQ)
        pre[5] += 1f * a(ModuleId.AUTO_EQ)
        // Balans widma: ujęcie ostrości 6–12 kHz.
        pre[5] -= 1f * a(ModuleId.SPECTRUM_BALANCE)
        pre[6] -= 0.5f * a(ModuleId.SPECTRUM_BALANCE)
        // Dehummer: zastępczo ujęcie najniższego pasma (filtr grzebieniowy 50/60 Hz niedostępny).
        pre[0] -= 3f * a(ModuleId.DEHUMMER)
        for (i in pre.indices) pre[i] = pre[i].coerceIn(-12f, 12f)

        // ---------------- MBC: Dynamika (+ bramka/ekspander od Naprawy)
        val agc = a(ModuleId.AGC)
        val mb1 = a(ModuleId.MULTIBAND1)
        val mb2 = a(ModuleId.MULTIBAND2)
        val comp = a(ModuleId.COMPRESSOR)
        val punch = a(ModuleId.PUNCH)
        val gate = a(ModuleId.NOISE_GATE)
        val nr = a(ModuleId.NOISE_REDUCTION)

        val ratio = (1f + 3f * mb1 + 2f * mb2 + 2f * comp + 1f * agc).coerceIn(1f, 10f)
        val threshold = (-6f - 14f * mb1 - 8f * mb2 - 8f * comp - 6f * agc).coerceIn(-40f, 0f)
        // Punch: wolniejszy atak przepuszcza transjenty (ekspansja w górę jest niedostępna).
        val attack = 3f + 40f * punch
        // AGC: wolniejsze zwolnienie = wyrównanie poziomu zamiast „pompowania”.
        val release = 80f + 600f * agc
        // Częściowa kompensacja wzmocnienia (połowa redukcji przy progu).
        // Kompensacja wzmocnienia ~80 % redukcji przy progu – głośniej i gęściej (jak rdzeń z AGC i clipperem).
        val makeup = if (ratio > 1.001f) ((-threshold) * (1f - 1f / ratio) * 0.8f).coerceIn(0f, 12f) else 0f
        val mbc = MBC_CUTOFFS.indices.map { band ->
            val isHigh = band == MBC_CUTOFFS.lastIndex
            val gateDb = maxOf(
                if (gate > 0f) -90f + 30f * gate else -90f,
                if (isHigh && nr > 0f) -80f + 25f * nr else -90f,
            )
            val expRatio = 1f + 2f * maxOf(gate, if (isHigh) nr else 0f)
            Mbc(
                attackMs = attack,
                releaseMs = release,
                ratio = ratio,
                thresholdDb = threshold,
                kneeDb = 6f,
                noiseGateDb = gateDb,
                expanderRatio = expRatio,
                preGainDb = 0f,
                postGainDb = makeup,
            )
        }

        // ---------------- postEQ: Naprawa MP3 → „powietrze” w górze pasma
        val post = FloatArray(POST_EQ_CUTOFFS.size)
        post[3] += 2f * a(ModuleId.DELOSSIFIER)

        // ---------------- wejście + limiter: Wyjście
        val clipHd = a(ModuleId.CLIPPER_HD)
        val clipSimple = a(ModuleId.SIMPLE_CLIPPER)
        val preLim = a(ModuleId.PRE_LIMITER)
        // Clippery podnoszą głośność przez dociśnięcie limitera (zamiast kasowania zniekształceń jak w rdzeniu).
        val drive = (3f * clipHd + 1.5f * clipSimple + 1f * preLim).coerceIn(0f, 6f)
        val hard = p[ModuleId.HARD_LIMIT].on
        val limiter = Limiter(
            enabled = hard,
            attackMs = 1f,
            releaseMs = 50f + 100f * preLim,
            ratio = 10f,
            // Limiter DP działa na próbkach (nie true-peak) – zapas 0,3 dB poniżej sufitu.
            thresholdDb = (p.ceilingDb - 0.3f).coerceIn(-12f, 0f),
            postGainDb = 0f,
        )

        return Targets(
            enabled = p.enabled && !p.compare,
            inputGainDb = drive,
            preEq = pre,
            mbc = mbc,
            postEq = post,
            limiter = limiter,
        )
    }
}
