package dev.erfnemati.mirza

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.os.SystemClock
import dev.erfnemati.mirza.core.Dictation
import dev.erfnemati.mirza.core.DictationConfig
import dev.erfnemati.mirza.core.DictationListener
import dev.erfnemati.mirza.core.MobileException
import dev.erfnemati.mirza.core.sampleRate
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.math.abs
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.sqrt

/**
 * One dictation at a time: makes the microphone available the way the chosen
 * mode says, records, streams to Soniox through the Rust core, and types what
 * comes back. Everything here runs on the main thread.
 */
object Session {
    enum class State { IDLE, STARTING, LISTENING, STOPPING }

    var state = State.IDLE
        private set
    val stateListeners = CopyOnWriteArrayList<(State) -> Unit>()

    private val main = Handler(Looper.getMainLooper())
    private lateinit var app: Context
    private var mode = MicMode.NONE
    private var dictation: Dictation? = null
    private var recorder: Recorder? = null
    private var startedAt = 0L
    private var pieces = 0
    private var partials = 0

    /** Longest session; a forgotten bubble must not record for hours. */
    private const val MAX_MS = 10 * 60 * 1000L

    fun toggle(ctx: Context, via: String) {
        when (state) {
            State.IDLE -> start(ctx, via)
            State.STARTING, State.LISTENING -> stop("tap via $via")
            State.STOPPING -> cancel("second tap while finishing")
        }
    }

    private fun start(ctx: Context, via: String) {
        app = ctx.applicationContext
        mode = Prefs.micMode
        pieces = 0
        partials = 0
        setState(State.STARTING)
        TrialLog.add(
            "TAP via $via: mic mode ${mode.name}, bubble ${Prefs.host.name}, " +
                "API ${Build.VERSION.SDK_INT}, app in front: ${TypingService.frontPackage ?: "unknown"}",
        )
        if (app.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            return fail("the microphone permission isn't granted")
        }
        when (mode) {
            MicMode.PROMOTE -> {
                val svc = BubbleService.instance ?: return fail("mode A needs the bubble service; show the bubble again")
                if (!svc.promote()) return fail("the bubble service couldn't add the microphone type")
                begin()
            }
            MicMode.SEPARATE -> try {
                app.startForegroundService(Intent(app, MicService::class.java))
                TrialLog.add("startForegroundService(MicService) accepted")
                // MicService calls micServiceReady() once it is in the foreground.
            } catch (e: Exception) {
                fail("startForegroundService(MicService) refused: ${describe(e)}")
            }
            MicMode.ALWAYS -> {
                if (BubbleService.instance?.hasMic != true) {
                    return fail("mode C needs the bubble service with the microphone type; hide and show the bubble")
                }
                begin()
            }
            MicMode.NONE -> begin()
        }
    }

    fun micServiceReady(ok: Boolean) {
        if (state != State.STARTING) return
        if (ok) begin() else fail("the microphone service didn't start")
    }

    private fun begin() {
        val key = Prefs.apiKey
        val d = if (key.isEmpty()) {
            TrialLog.add("no Soniox key: microphone test only, nothing is sent or typed")
            null
        } else {
            try {
                Dictation(DictationConfig(key, Prefs.model, Prefs.languageList(), Prefs.proxy), Listener())
            } catch (e: MobileException) {
                return fail("couldn't start: ${e.message}")
            }
        }
        val save = if (Prefs.saveAudio) java.io.File(app.filesDir, "last-recording.pcm") else null
        val r = Recorder(sampleRate().toInt(), save) { pcm -> d?.pushAudio(pcm) }
        r.start()?.let { err ->
            d?.cancel()
            d?.close()
            return fail(err)
        }
        dictation = d
        recorder = r
        startedAt = SystemClock.elapsedRealtime()
        setState(State.LISTENING)
        main.postDelayed({ if (recorder === r) TrialLog.add("after 2 s: ${r.report()}") }, 2000)
        main.postDelayed({ if (recorder === r) stop("10 minute limit") }, MAX_MS)
    }

    fun stop(reason: String) {
        val r = recorder
        if (r == null) {
            if (state == State.STARTING) cancel(reason)
            return
        }
        r.stop()
        recorder = null
        val secs = (SystemClock.elapsedRealtime() - startedAt) / 1000.0
        TrialLog.add("stop ($reason) after ${"%.1f".format(secs)} s: ${r.report()}")
        val d = dictation ?: return done()
        setState(State.STOPPING)
        d.finish()
        main.postDelayed({ if (dictation === d) cancel("Soniox didn't finish within 20 s") }, 20_000)
    }

    fun cancel(reason: String) {
        if (state == State.IDLE) return
        TrialLog.add("cancelled: $reason")
        recorder?.stop()
        recorder = null
        dictation?.cancel()
        done()
    }

    private fun done() {
        dictation?.close()
        dictation = null
        releaseMic()
        setState(State.IDLE)
    }

    private fun fail(msg: String) {
        TrialLog.add("ERROR $msg")
        recorder?.stop()
        recorder = null
        dictation?.cancel()
        done()
    }

    private fun releaseMic() {
        when (mode) {
            MicMode.PROMOTE -> BubbleService.instance?.demote()
            MicMode.SEPARATE -> app.stopService(Intent(app, MicService::class.java))
            else -> {}
        }
    }

    private fun setState(s: State) {
        state = s
        stateListeners.forEach { it(s) }
    }

    private fun typeText(text: String) {
        pieces++
        val shown = if (Prefs.logText) " \"$text\"" else " (${text.length} chars)"
        val svc = TypingService.instance
        if (svc == null) {
            TrialLog.add("NOT TYPED$shown: the accessibility service is off")
            return
        }
        TrialLog.add(svc.type(text) + shown)
    }

    /** Called by the Rust core on its own threads; hands everything to the main thread. */
    private class Listener : DictationListener {
        override fun onText(text: String) {
            main.post { if (dictation != null) typeText(text) }
        }

        override fun onPartial(text: String) {
            main.post {
                // Only counted, so the log shows whether Soniox heard anything at all.
                if (dictation != null && partials++ == 0) {
                    TrialLog.add("first partial text after ${(SystemClock.elapsedRealtime() - startedAt) / 100 / 10.0} s")
                }
            }
        }

        override fun onError(message: String) {
            main.post { if (dictation != null) fail("Soniox: $message") }
        }

        override fun onFinished() {
            main.post {
                if (dictation != null) {
                    TrialLog.add("transcript complete: $pieces pieces typed, $partials partial updates")
                    done()
                }
            }
        }
    }
}

/**
 * Records 16-bit mono audio in 50 ms chunks on its own thread, and keeps
 * track of the level, so a microphone that Android silences shows up.
 */
class Recorder(private val rate: Int, private val save: java.io.File?, private val onChunk: (ByteArray) -> Unit) {
    @Volatile private var running = false
    private var thread: Thread? = null
    private var rec: AudioRecord? = null

    @Volatile private var chunks = 0

    /** Chunks with at least one non-zero sample. */
    @Volatile private var soundChunks = 0

    /** What Android says about this recording: silenced at some point, and not silenced at some point. */
    @Volatile private var silencedSeen = false

    @Volatile private var openSeen = false

    @Volatile private var peak = 0

    @Volatile private var loudestDb = -100.0

    /** Null when recording, or why it couldn't start. */
    @SuppressLint("MissingPermission")
    fun start(): String? {
        val chunkBytes = rate * 2 / 20
        val min = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        if (min <= 0) return "this phone can't record at $rate Hz"
        val r = try {
            AudioRecord(
                MediaRecorder.AudioSource.VOICE_RECOGNITION,
                rate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                max(min, chunkBytes * 8),
            )
        } catch (e: Exception) {
            return "AudioRecord refused: ${describe(e)}"
        }
        if (r.state != AudioRecord.STATE_INITIALIZED) {
            r.release()
            return "the microphone couldn't be opened (AudioRecord not initialized)"
        }
        try {
            r.startRecording()
        } catch (e: Exception) {
            r.release()
            return "startRecording failed: ${describe(e)}"
        }
        if (r.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
            r.release()
            return "the microphone is busy or blocked (not recording after startRecording)"
        }
        rec = r
        running = true
        thread = Thread({ loop(r, chunkBytes) }, "mirza-mic").also { it.start() }
        return null
    }

    private fun loop(r: AudioRecord, n: Int) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        val buf = ByteArray(n)
        val out = save?.outputStream()?.buffered()
        while (running) {
            var off = 0
            while (off < n && running) {
                val got = r.read(buf, off, n - off)
                if (got < 0) {
                    TrialLog.add("AudioRecord.read error $got")
                    running = false
                    break
                }
                off += got
            }
            if (off < n) break
            if (measure(buf)) soundChunks++
            if (chunks % 10 == 0) checkSilenced(r)
            chunks++
            out?.write(buf)
            onChunk(buf.copyOf())
        }
        out?.close()
    }

    /** True when the chunk has any sound in it. */
    private fun measure(buf: ByteArray): Boolean {
        var sum = 0.0
        var chunkPeak = 0
        for (i in 0 until buf.size / 2) {
            val s = (buf[2 * i].toInt() and 0xff) or (buf[2 * i + 1].toInt() shl 8)
            chunkPeak = max(chunkPeak, abs(s))
            sum += s.toDouble() * s
        }
        peak = max(peak, chunkPeak)
        val rms = sqrt(sum / (buf.size / 2)) / 32768.0
        if (rms > 0) loudestDb = max(loudestDb, 20 * log10(rms))
        return chunkPeak > 0
    }

    /** Android's own answer to "is this app's microphone muted by the system?" (API 29+). */
    private fun checkSilenced(r: AudioRecord) {
        val silenced = r.activeRecordingConfiguration?.isClientSilenced ?: return
        if (silenced) silencedSeen = true else openSeen = true
    }

    private fun androidSays(): String = when {
        silencedSeen && openSeen -> "Android silenced it part of the time"
        silencedSeen -> "Android says it silenced this app"
        openSeen -> "Android says it did not silence this app"
        else -> "Android didn't say whether it silenced this app"
    }

    fun report(): String = when {
        chunks == 0 -> "no audio arrived"
        peak == 0 -> "SILENT: $chunks chunks, every sample is zero; ${androidSays()}"
        else -> "audio OK: $chunks chunks, $soundChunks with sound, peak $peak, " +
            "loudest ${"%.0f".format(loudestDb)} dBFS; ${androidSays()}"
    }

    fun stop() {
        running = false
        try {
            rec?.stop()
        } catch (_: IllegalStateException) {
        }
        thread?.join(1000)
        rec?.release()
        rec = null
    }
}
