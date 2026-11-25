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
 * Przestrzeń, Szerokość, Kaptur i Binauralny: DynamicsProcessing nie ma M/S ani splotu – zastępczo systemowy `Virtualizer` ([Targets.virtualizer]).
 * Moduły bez odpowiednika (brak wpływu w tym trybie): Declipper, Dekwantyzer, Rotacja fazy, Mniej słów, Szum tła.
 *
 * Czysta funkcja (bez API Androida) – testowalna na JVM.
 */
object DpMapping {

    /** Górne krawędzie pasm preEQ (Hz). */
    val PRE_EQ_CUTOFFS = floatArrayOf(60f, 150f, 400f, 1000f, 2500f, 6000f, 12000f, 20000f)

    /** Górne krawędzie pasm kompresora wielopasmowego (Hz). */
    val MBC_CUTOFFS = floatArrayOf(150f, 1000f, 5000f, 20000f)

    /** Poziom odniesienia kompensacji kompresora i jej górna granica (dB). */
    const val MAKEUP_REF_DB = -12f
    const val MAKEUP_MAX_DB = 6f

    /** Największe podbicie jednego pasma korektora (dB) – suma filtrów basu nie może wejść wyżej. */
    const val PRE_EQ_MAX_DB = 9f

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
        /** Siła systemowego wirtualizera 0..1000 (0 = wyłączony). */
        val virtualizer: Int,
    ) {
        // FloatArray nie ma równości strukturalnej – porównujemy zawartość (unikamy zbędnych wywołań do audioserwera).
        override fun equals(other: Any?): Boolean = other is Targets &&
            enabled == other.enabled && inputGainDb == other.inputGainDb &&
            preEq.contentEquals(other.preEq) && mbc == other.mbc &&
            postEq.contentEquals(other.postEq) && limiter == other.limiter && virtualizer == other.virtualizer

        override fun hashCode(): Int {
            var h = enabled.hashCode()
            h = 31 * h + inputGainDb.hashCode()
            h = 31 * h + preEq.contentHashCode()
            h = 31 * h + mbc.hashCode()
            h = 31 * h + postEq.contentHashCode()
            h = 31 * h + limiter.hashCode()
            h = 31 * h + virtualizer
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
        // 2026-11-24: jeszcze mocniej – jak rdzeń v3 (≈ +3 / +6 / +5 dB przy pełnym suwaku).
        pre[0] += 3f * a(ModuleId.WARM_BASS)
        pre[1] += 5.5f * a(ModuleId.WARM_BASS)
        pre[2] += 3.5f * a(ModuleId.WARM_BASS)
        pre[4] -= 1f * a(ModuleId.WARM_BASS)
        // Radiowy bas: głęboki dół w górę, dudnienie (150–400 Hz) lekko w dół; gęstość – w kompresorze pasma basu (MBC).
        pre[0] += 5f * a(ModuleId.RADIO_BASS)
        pre[1] += 2f * a(ModuleId.RADIO_BASS)
        pre[2] -= 1.5f * a(ModuleId.RADIO_BASS)
        // Dynamiczna góra: bez toru bocznego – umiarkowane podbicie 6–20 kHz (rdzeń daje średnio +4…+6 dB).
        pre[6] += 3.5f * a(ModuleId.DYNAMIC_TREBLE)
        pre[7] += 5f * a(ModuleId.DYNAMIC_TREBLE)
        // Kaptur: przyciemnienie góry i odrobina ciepła; odbić nie da się zrobić – przestrzeń z wirtualizera.
        pre[2] += 1f * a(ModuleId.HOOD)
        pre[5] -= 2f * a(ModuleId.HOOD)
        pre[6] -= 4f * a(ModuleId.HOOD)
        pre[7] -= 4.5f * a(ModuleId.HOOD)
        // Auto EQ: statyczne przybliżenie – mniej „buczenia”, więcej obecności.
        pre[2] -= 1f * a(ModuleId.AUTO_EQ)
        pre[4] += 1f * a(ModuleId.AUTO_EQ)
        pre[5] += 1f * a(ModuleId.AUTO_EQ)
        // Balans widma: ujęcie ostrości 6–12 kHz.
        pre[5] -= 1f * a(ModuleId.SPECTRUM_BALANCE)
        pre[6] -= 0.5f * a(ModuleId.SPECTRUM_BALANCE)
        // Dehummer: zastępczo ujęcie najniższego pasma (filtr grzebieniowy 50/60 Hz niedostępny).
        pre[0] -= 3f * a(ModuleId.DEHUMMER)
        for (i in pre.indices) pre[i] = pre[i].coerceIn(-12f, PRE_EQ_MAX_DB)

        // ---------------- MBC: Dynamika (+ bramka/ekspander od Naprawy)
        val agc = a(ModuleId.AGC)
        val mb1 = a(ModuleId.MULTIBAND1)
        val mb2 = a(ModuleId.MULTIBAND2)
        // Równy strumień: w tym trybie zastępczo mocniejsza kompresja (wygładzania wahań w pasmach tu nie ma).
        // Mniej słów i Szum tła nie mają odpowiednika: efekty systemowe nie odejmują kanałów i nie dodają dźwięku.
        val comp = maxOf(a(ModuleId.COMPRESSOR), 0.8f * a(ModuleId.EVEN_STREAM))
        // Kryształ: jak Punch – wolniejszy atak przepuszcza uderzenia (wyostrzania w pasmach tu nie ma)
        val punch = maxOf(a(ModuleId.PUNCH), 0.7f * a(ModuleId.CRYSTAL))
        val gate = a(ModuleId.NOISE_GATE)
        val nr = a(ModuleId.NOISE_REDUCTION)

        val ratio = (1f + 3f * mb1 + 2f * mb2 + 2f * comp + 1f * agc).coerceIn(1f, 10f)
        val threshold = (-6f - 14f * mb1 - 8f * mb2 - 8f * comp - 6f * agc).coerceIn(-40f, 0f)
        // Punch: wolniejszy atak przepuszcza transjenty (ekspansja w górę jest niedostępna).
        val attack = 3f + 40f * punch
        // AGC: wolniejsze zwolnienie = wyrównanie poziomu zamiast „pompowania”.
        val release = 80f + 600f * agc
        // Kompensacja: 60 % redukcji, jaką dostaje typowa głośna muzyka (obwiednia pasma ok. −12 dBFS), najwyżej 6 dB.
        // Do 0.5.0 liczona od pełnej skali (do +12 dB): przy mocnych filtrach Multiband muzyka wychodziła o 6–8 dB za głośno,
        // limiter zgniatał ją bez przerwy, a bez limitera dźwięk się obcinał („mocno przesterowany”, uwaga z 2026-11-25).
        val makeup = if (ratio > 1.001f) {
            (0.6f * (MAKEUP_REF_DB - threshold).coerceAtLeast(0f) * (1f - 1f / ratio)).coerceIn(0f, MAKEUP_MAX_DB)
        } else 0f
        val radio = a(ModuleId.RADIO_BASS)
        val mbc = MBC_CUTOFFS.indices.map { band ->
            val isHigh = band == MBC_CUTOFFS.lastIndex
            // Radiowy bas: pasmo < 150 Hz gęściej (niższy próg, wyższe ratio) z wyrównaniem głośności
            val dense = if (band == 0) radio else 0f
            val gateDb = maxOf(
                if (gate > 0f) -90f + 30f * gate else -90f,
                if (isHigh && nr > 0f) -80f + 25f * nr else -90f,
            )
            val expRatio = 1f + 2f * maxOf(gate, if (isHigh) nr else 0f)
            Mbc(
                attackMs = attack,
                releaseMs = release,
                ratio = (ratio + 2f * dense).coerceIn(1f, 10f),
                thresholdDb = (threshold - 10f * dense).coerceIn(-40f, 0f),
                kneeDb = 6f + 4f * dense,
                noiseGateDb = gateDb,
                expanderRatio = expRatio,
                preGainDb = 0f,
                postGainDb = (makeup + 1.5f * dense).coerceAtMost(MAKEUP_MAX_DB),
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
        val drive = (2f * clipHd + 1f * clipSimple + 0.5f * preLim).coerceIn(0f, 3f)
        val hard = p[ModuleId.HARD_LIMIT].on
        // Limiter także wtedy, gdy Hard Limit jest wyłączony, a ustawienia mogą podnieść poziom ponad wejście –
        // efekty systemowe nie mają innej ochrony przed obcięciem.
        val canBoost = drive + (pre.maxOrNull() ?: 0f).coerceAtLeast(0f) + mbc.maxOf { it.postGainDb } + post.max() > 0.5f
        val limiter = Limiter(
            enabled = hard || canBoost,
            attackMs = 1f,
            releaseMs = 50f + 100f * preLim,
            ratio = 50f,
            // Limiter DP działa na próbkach (nie true-peak) i ma atak 1 ms – zapas 1,5 dB poniżej sufitu, ratio 50.
            thresholdDb = (p.ceilingDb - 1.5f).coerceIn(-12f, 0f),
            postGainDb = 0f,
        )

        return Targets(
            enabled = p.enabled && !p.compare,
            inputGainDb = drive,
            preEq = pre,
            mbc = mbc,
            postEq = post,
            limiter = limiter,
            virtualizer = (1000f * maxOf(a(ModuleId.SPACE), a(ModuleId.BINAURAL), a(ModuleId.HOOD), 0.5f * a(ModuleId.WIDTH))).toInt().coerceIn(0, 1000),
        )
    }
}
