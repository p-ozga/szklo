package pl.szklo.audio

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.net.Uri
import android.os.Process
import android.os.SystemClock
import android.util.Log
import pl.szklo.core.NativeEngine
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Wątek odtwarzania lokalnego pliku:
 * MediaExtractor → MediaCodec (dekoder) → PCM float → [NativeEngine.process] → AudioTrack (float).
 *
 * Częstotliwość próbkowania AudioTracka = częstotliwość pliku (bez resamplingu w aplikacji; ewentualne
 * przepróbkowanie do natywnej częstotliwości urządzenia robi system). Silnik natywny tworzony jest
 * dla tej samej częstotliwości. Wielokanałowe pliki: bierzemy dwa pierwsze kanały (FL, FR).
 *
 * Callbacki [Listener] przychodzą z wątku odtwarzacza.
 */
class PcmPlayer(
    private val context: Context,
    private val uri: Uri,
    private val listener: Listener,
) : Thread("szklo-odtwarzacz") {

    interface Listener {
        fun onStarted(player: PcmPlayer, sampleRate: Int, channels: Int, durationMs: Long, processed: Boolean)
        fun onProgress(player: PcmPlayer, positionMs: Long)
        fun onCompleted(player: PcmPlayer)
        fun onError(player: PcmPlayer, message: String)
    }

    @Volatile private var stopRequested = false
    @Volatile var isPaused = false
        private set

    private val pauseLock = Object()
    private val trackLock = Any()
    private var track: AudioTrack? = null // dostęp pod trackLock

    fun pausePlayback() {
        isPaused = true
        synchronized(trackLock) { runCatching { track?.pause() } }
    }

    fun resumePlayback() {
        synchronized(trackLock) { runCatching { track?.play() } }
        synchronized(pauseLock) {
            isPaused = false
            pauseLock.notifyAll()
        }
    }

    /** Prośba o zatrzymanie – zwalnianie zasobów odbywa się na wątku odtwarzacza. */
    fun requestStop() {
        stopRequested = true
        synchronized(pauseLock) { pauseLock.notifyAll() }
        // Odblokowuje ewentualny WRITE_BLOCKING na zapauzowanym torze.
        synchronized(trackLock) { runCatching { track?.pause(); track?.flush() } }
    }

    override fun run() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_AUDIO)
        val extractor = MediaExtractor()
        var codec: MediaCodec? = null
        var engine: NativeEngine? = null
        try {
            extractor.setDataSource(context, uri, null)
            val trackIndex = (0 until extractor.trackCount).firstOrNull { i ->
                extractor.getTrackFormat(i).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true
            } ?: throw IOException("Plik nie zawiera ścieżki audio")
            extractor.selectTrack(trackIndex)
            val inFormat = extractor.getTrackFormat(trackIndex)
            val mime = inFormat.getString(MediaFormat.KEY_MIME) ?: throw IOException("Nieznany format")
            val durationMs = if (inFormat.containsKey(MediaFormat.KEY_DURATION)) inFormat.getLong(MediaFormat.KEY_DURATION) / 1000 else -1L

            // Prośba o wyjście float – dekoder może ją zignorować (obsługujemy też 16/24/32-bit).
            inFormat.setInteger(MediaFormat.KEY_PCM_ENCODING, AudioFormat.ENCODING_PCM_FLOAT)
            val dec = MediaCodec.createDecoderByType(mime)
            codec = dec
            dec.configure(inFormat, null, null, 0)
            dec.start()

            val info = MediaCodec.BufferInfo()
            var inputDone = false
            var outputDone = false
            var sink: Sink? = null
            var lastProgressAt = 0L
            var floatBuf = FloatArray(0)

            while (!outputDone && !stopRequested) {
                waitWhilePaused()
                if (stopRequested) break

                if (!inputDone) {
                    val inIdx = dec.dequeueInputBuffer(TIMEOUT_US)
                    if (inIdx >= 0) {
                        val inBuf = dec.getInputBuffer(inIdx) ?: throw IOException("Brak bufora wejściowego dekodera")
                        val size = extractor.readSampleData(inBuf, 0)
                        if (size < 0) {
                            dec.queueInputBuffer(inIdx, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                            inputDone = true
                        } else {
                            dec.queueInputBuffer(inIdx, 0, size, extractor.sampleTime, 0)
                            extractor.advance()
                        }
                    }
                }

                val outIdx = dec.dequeueOutputBuffer(info, TIMEOUT_US)
                when {
                    outIdx == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                        val newSink = Sink.from(dec.outputFormat)
                        val cur = sink
                        if (cur == null || !cur.sameAs(newSink)) {
                            // Nowy format → nowy tor i silnik (zwykle tylko raz, na początku).
                            sink = newSink
                            engine = recreateOutput(engine, newSink, durationMs)
                        }
                    }
                    outIdx >= 0 -> {
                        // Część dekoderów nie zgłasza INFO_OUTPUT_FORMAT_CHANGED przed pierwszym buforem.
                        val known = sink
                        val s: Sink
                        if (known != null) {
                            s = known
                        } else {
                            s = Sink.from(dec.outputFormat)
                            sink = s
                            engine = recreateOutput(engine, s, durationMs)
                        }
                        if (info.size > 0) {
                            val outBuf = dec.getOutputBuffer(outIdx)
                            if (outBuf != null) {
                                outBuf.position(info.offset)
                                outBuf.limit(info.offset + info.size)
                                val frames = info.size / (s.bytesPerSample * s.inChannels)
                                val needed = frames * s.outChannels
                                if (floatBuf.size < needed) floatBuf = FloatArray(needed)
                                s.toFloat(outBuf, frames, floatBuf)
                                engine?.process(floatBuf, frames, s.outChannels)
                                writeFully(floatBuf, frames, s.outChannels)
                            }
                        }
                        dec.releaseOutputBuffer(outIdx, false)
                        if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) outputDone = true

                        val now = SystemClock.uptimeMillis()
                        if (now - lastProgressAt >= PROGRESS_INTERVAL_MS) {
                            lastProgressAt = now
                            listener.onProgress(this, info.presentationTimeUs / 1000)
                        }
                    }
                    else -> Unit // INFO_TRY_AGAIN_LATER
                }
            }

            if (outputDone && !stopRequested) {
                val s = sink
                if (s != null) drainTail(engine, s)
                listener.onCompleted(this)
            }
        } catch (e: Exception) {
            if (!stopRequested) {
                Log.e(TAG, "Błąd odtwarzania", e)
                listener.onError(this, e.message ?: e.javaClass.simpleName)
            }
        } finally {
            runCatching { codec?.stop() }
            runCatching { codec?.release() }
            runCatching { extractor.release() }
            synchronized(trackLock) {
                track?.let { t -> runCatching { t.pause(); t.flush(); t.release() } }
                track = null
            }
            engine?.let {
                AudioHub.detachEngine(it)
                it.close()
            }
        }
    }

    /** Tworzy AudioTrack i NativeEngine dla formatu [s]; zamyka poprzednie. */
    private fun recreateOutput(old: NativeEngine?, s: Sink, durationMs: Long): NativeEngine? {
        old?.let {
            AudioHub.detachEngine(it)
            it.close()
        }
        synchronized(trackLock) {
            track?.let { t -> runCatching { t.pause(); t.flush(); t.release() } }
            track = buildTrack(s).also { if (!isPaused) it.play() }
            framesWritten = 0L
        }
        val engine = NativeEngine.create(s.sampleRate)
        if (engine != null) AudioHub.attachEngine(engine)
        listener.onStarted(this, s.sampleRate, s.inChannels, durationMs, processed = engine != null)
        return engine
    }

    private fun buildTrack(s: Sink): AudioTrack {
        val mask = if (s.outChannels == 1) AudioFormat.CHANNEL_OUT_MONO else AudioFormat.CHANNEL_OUT_STEREO
        val min = AudioTrack.getMinBufferSize(s.sampleRate, mask, AudioFormat.ENCODING_PCM_FLOAT)
        if (min <= 0) throw IOException("Urządzenie nie obsługuje ${s.sampleRate} Hz w formacie float")
        // ~200 ms bufora: odporność na zacięcia GC kosztem minimalnie wolniejszej reakcji na suwaki.
        val target = s.sampleRate / 5 * s.outChannels * 4
        return AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                    .setSampleRate(s.sampleRate)
                    .setChannelMask(mask)
                    .build(),
            )
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setBufferSizeInBytes(maxOf(min * 2, target))
            .build()
    }

    /** Liczba ramek przekazanych do bieżącego toru (do oczekiwania na dogranie końcówki). */
    private var framesWritten = 0L

    private fun writeFully(buf: FloatArray, frames: Int, channels: Int) {
        val samples = frames * channels
        var off = 0
        while (off < samples && !stopRequested) {
            val t = synchronized(trackLock) { track } ?: return
            val n = t.write(buf, off, samples - off, AudioTrack.WRITE_BLOCKING)
            if (n < 0) throw IOException("AudioTrack.write: błąd $n")
            if (n == 0) {
                // Tor zapauzowany lub opróżniony przez requestStop – czekamy na wznowienie albo stop.
                waitWhilePaused()
                if (!isPaused) SystemClock.sleep(2)
            }
            off += n
        }
        framesWritten += off / channels
    }

    /** Wypycha ogon lookaheadu (cisza przez silnik) i czeka na dogranie bufora. */
    private fun drainTail(engine: NativeEngine?, s: Sink) {
        val latency = engine?.latencyFrames() ?: 0
        if (engine != null && latency > 0) {
            val chunk = 1024
            val buf = FloatArray(chunk * s.outChannels)
            var left = latency
            while (left > 0 && !stopRequested) {
                val n = minOf(chunk, left)
                buf.fill(0f, 0, n * s.outChannels)
                engine.process(buf, n, s.outChannels)
                writeFully(buf, n, s.outChannels)
                left -= n
            }
        }
        val t = synchronized(trackLock) { track } ?: return
        runCatching { t.stop() } // MODE_STREAM: dogrywa to, co już zapisano
        val deadline = SystemClock.uptimeMillis() + 3000
        while (!stopRequested && SystemClock.uptimeMillis() < deadline) {
            val head = t.playbackHeadPosition.toLong() and 0xFFFFFFFFL
            if (head >= framesWritten) break
            SystemClock.sleep(20)
        }
    }

    private fun waitWhilePaused() {
        synchronized(pauseLock) {
            while (isPaused && !stopRequested) {
                try {
                    pauseLock.wait()
                } catch (_: InterruptedException) {
                    return
                }
            }
        }
    }

    /** Format wyjścia dekodera i konwersja do float z przeplotem (1 lub 2 kanały). */
    private class Sink(
        val sampleRate: Int,
        val inChannels: Int,
        val encoding: Int,
    ) {
        val outChannels = if (inChannels == 1) 1 else 2
        val bytesPerSample = when (encoding) {
            AudioFormat.ENCODING_PCM_FLOAT, AudioFormat.ENCODING_PCM_32BIT -> 4
            AudioFormat.ENCODING_PCM_24BIT_PACKED -> 3
            AudioFormat.ENCODING_PCM_8BIT -> 1
            else -> 2
        }

        fun sameAs(o: Sink) = sampleRate == o.sampleRate && inChannels == o.inChannels && encoding == o.encoding

        fun toFloat(src: ByteBuffer, frames: Int, dst: FloatArray) {
            val b = src.order(ByteOrder.nativeOrder())
            val inCh = inChannels
            val outCh = outChannels
            when (encoding) {
                AudioFormat.ENCODING_PCM_FLOAT -> {
                    val fb = b.asFloatBuffer()
                    if (inCh == outCh) {
                        fb.get(dst, 0, frames * outCh)
                    } else {
                        for (f in 0 until frames) {
                            val base = f * inCh
                            dst[f * 2] = fb.get(base)
                            dst[f * 2 + 1] = fb.get(base + 1)
                        }
                    }
                }
                AudioFormat.ENCODING_PCM_32BIT -> {
                    val ib = b.asIntBuffer()
                    for (f in 0 until frames) for (c in 0 until outCh) {
                        dst[f * outCh + c] = ib.get(f * inCh + c) / 2147483648f
                    }
                }
                AudioFormat.ENCODING_PCM_24BIT_PACKED -> {
                    val start = b.position()
                    for (f in 0 until frames) for (c in 0 until outCh) {
                        val p = start + (f * inCh + c) * 3
                        val v = (b.get(p).toInt() and 0xFF) or
                            ((b.get(p + 1).toInt() and 0xFF) shl 8) or
                            (b.get(p + 2).toInt() shl 16) // znak z najstarszego bajtu
                        dst[f * outCh + c] = v / 8388608f
                    }
                }
                AudioFormat.ENCODING_PCM_8BIT -> {
                    val start = b.position()
                    for (f in 0 until frames) for (c in 0 until outCh) {
                        dst[f * outCh + c] = ((b.get(start + f * inCh + c).toInt() and 0xFF) - 128) / 128f
                    }
                }
                else -> {
                    val sb = b.asShortBuffer()
                    for (f in 0 until frames) for (c in 0 until outCh) {
                        dst[f * outCh + c] = sb.get(f * inCh + c) / 32768f
                    }
                }
            }
        }

        companion object {
            fun from(f: MediaFormat): Sink {
                val rate = f.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                val ch = f.getInteger(MediaFormat.KEY_CHANNEL_COUNT).coerceAtLeast(1)
                val enc = if (f.containsKey(MediaFormat.KEY_PCM_ENCODING)) {
                    f.getInteger(MediaFormat.KEY_PCM_ENCODING)
                } else {
                    AudioFormat.ENCODING_PCM_16BIT
                }
                return Sink(rate, ch, enc)
            }
        }
    }

    private companion object {
        const val TAG = "PcmPlayer"
        const val TIMEOUT_US = 10_000L
        const val PROGRESS_INTERVAL_MS = 250L
    }
}
