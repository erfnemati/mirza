package dev.erfnemati.mirza

import android.Manifest
import android.annotation.SuppressLint
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.os.SystemClock
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import dev.erfnemati.mirza.core.Dictation
import dev.erfnemati.mirza.core.DictationListener
import dev.erfnemati.mirza.core.MobileException
import dev.erfnemati.mirza.core.SoundKind
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread
import kotlin.math.abs
import kotlin.math.max

/**
 * One dictation at a time: starts the microphone service, records, streams the audio to the
 * speech service through the Rust core, and types what comes back. Plain dictation types as the
 * words settle; revised dictation waits for the end, has the AI revise the whole text, and types
 * that. Text that can't be typed ends up in the clipboard, so nothing is lost. Everything here
 * runs on the main thread.
 */
object Session {
    enum class State { IDLE, STARTING, LISTENING, FINISHING }

    enum class Mode { DICTATE, REVISE }

    /** Compose state, so the home screen shows it; the bubble listens through [stateListeners]. */
    var state by mutableStateOf(State.IDLE)
        private set
    val stateListeners = CopyOnWriteArrayList<(State) -> Unit>()

    private val main = Handler(Looper.getMainLooper())
    private var dictation: Dictation? = null
    private var recorder: Recorder? = null
    private var startedAt = 0L
    private var lastSpeech = 0L
    private var warnedSilent = false

    var mode = Mode.DICTATE
        private set

    /** What the speech service heard in this session. */
    private val heard = StringBuilder()
    private var typedAny = false

    /** The revision running, if any; a newer one makes older answers stale. */
    private var revising = 0

    /** MicService has reached micReady, so Session may stop it. */
    private var micStarted = false

    /** Text that couldn't be typed, and why. */
    private val untyped = StringBuilder()
    private var untypedWhy = ""

    private val tick = object : Runnable {
        override fun run() {
            if (state != State.LISTENING) return
            val now = SystemClock.elapsedRealtime()
            val s = Core.settings
            when {
                s.maxSessionSec > 0 && now - startedAt > s.maxSessionSec * 1000 -> stop("time limit")
                s.silenceStopSec > 0 && now - lastSpeech > s.silenceStopSec * 1000 -> stop("silence")
                else -> {
                    if (!warnedSilent && now - startedAt > 3000 && recorder?.heardNothing() == true) {
                        warnedSilent = true
                        toast("The microphone gives only silence. Is another app using it?")
                        AppLog.add("microphone silent after 3 s")
                    }
                    main.postDelayed(this, 1000)
                }
            }
        }
    }

    /** A tap: start in `mode` when idle, else stop (or skip the AI revision while it runs). */
    fun toggle(mode: Mode = Mode.DICTATE) {
        when (state) {
            State.IDLE -> start(mode)
            State.STARTING, State.LISTENING -> stop("tap")
            State.FINISHING -> if (this.mode == Mode.REVISE) skipRevision() else cancel("tap while finishing")
        }
    }

    /** The AI is revising the text: a tap types it as it was said. */
    val revisingNow: Boolean get() = state == State.FINISHING && mode == Mode.REVISE

    /**
     * A double tap right after the first tap started a dictation: the same recording, but in
     * `newMode`. Only while nothing has been typed yet. Returns whether it changed.
     */
    fun switchMode(newMode: Mode): Boolean {
        if (state == State.IDLE || state == State.FINISHING || typedAny) return false
        if (mode == newMode) return true
        AppLog.add("switched to ${newMode.name.lowercase()}")
        mode = newMode
        // Anything heard while it waited for the AI is typed now.
        if (newMode == Mode.DICTATE) deliver(heard.toString())
        return true
    }

    fun start(mode: Mode) {
        val app = MirzaApp.app
        val svc = TypingService.instance ?: return toast("Typing is off: open Mirza to turn it on.")
        if (svc.locked()) {
            // Android hides toasts behind the lock screen on many phones: the sound says it.
            AppLog.add("tap on the lock screen: not recording")
            Sounds.play(SoundKind.ERROR)
            return toast("Unlock the phone first. Mirza doesn't type on the lock screen.")
        }
        if (app.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            return toast("Mirza needs the microphone: open Mirza to allow it.")
        }
        if (!svc.hasField()) return toast("Tap into a text field first.")
        val service = Core.settings.activeProvider
        if (!Core.keySet(service)) return toast("Enter your ${serviceName(service)} API key in Mirza first.")
        this.mode = mode
        heard.clear()
        typedAny = false
        untyped.clear()
        warnedSilent = false
        micStarted = false
        changeState(State.STARTING)
        try {
            // MicService calls micReady() once it holds the microphone.
            app.startForegroundService(Intent(app, MicService::class.java))
        } catch (e: Exception) {
            fail("Android didn't let Mirza use the microphone now.", describe(e))
        }
    }

    /**
     * Called by MicService once it is a foreground service (or was refused). Returns whether it
     * should keep running; the service stops itself otherwise. Session never stops it before this
     * call, since stopping a service that hasn't reached startForeground can crash the app.
     */
    fun micReady(ok: Boolean): Boolean {
        micStarted = true
        if (state != State.STARTING) return false
        if (ok) begin() else fail("Android didn't let Mirza use the microphone now.", "microphone service refused")
        return state == State.LISTENING
    }

    private fun begin() {
        val d = try {
            Core.api.dictate(Listener())
        } catch (e: MobileException) {
            return fail(e.message ?: "Couldn't start.", "settings")
        }
        // The core keeps 10 s of audio while it connects. If that fills up, stop listening but
        // keep what was said: it is still sent once the connection is up.
        val full = AtomicBoolean(false)
        val r = Recorder(Core.api.sampleRate().toInt()) { pcm ->
            if (!d.pushAudio(pcm) && !full.getAndSet(true)) {
                main.post {
                    if (dictation === d && state == State.LISTENING) {
                        toast("The connection is slow, so Mirza stopped listening. What you said so far still comes.")
                        stop("audio queue full")
                    }
                }
            }
        }
        r.start()?.let { err ->
            d.cancel()
            d.close()
            return fail("The microphone couldn't be opened.", err)
        }
        dictation = d
        recorder = r
        startedAt = SystemClock.elapsedRealtime()
        lastSpeech = startedAt
        changeState(State.LISTENING)
        Sounds.play(SoundKind.START)
        main.postDelayed(tick, 1000)
        AppLog.add("listening (${Core.settings.activeProvider}, ${mode.name.lowercase()})")
    }

    fun stop(reason: String) {
        val r = recorder
        if (r == null) {
            if (state == State.STARTING) cancel(reason)
            return
        }
        val ms = SystemClock.elapsedRealtime() - startedAt
        if (ms < 1000) {
            // A second tap right away: nothing worth sending, and the service would answer
            // "no audio" with an error.
            Sounds.play(SoundKind.STOP)
            return cancel("$reason after $ms ms")
        }
        r.stop()
        recorder = null
        Sounds.play(SoundKind.STOP)
        AppLog.add("stopped ($reason) after ${ms / 1000} s")
        val d = dictation ?: return done()
        changeState(State.FINISHING)
        d.finish()
        main.postDelayed({ if (dictation === d) fail("The speech service didn't finish in time.", "no end after 20 s") }, 20_000)
    }

    fun cancel(reason: String) {
        if (state == State.IDLE) return
        AppLog.add("cancelled ($reason)")
        revising = 0
        heard.clear()
        recorder?.stop()
        recorder = null
        dictation?.cancel()
        done()
    }

    private fun fail(message: String, detail: String) {
        AppLog.add("error: $message ($detail)")
        toast(message)
        Sounds.play(SoundKind.ERROR)
        recorder?.stop()
        recorder = null
        dictation?.cancel()
        // Revised dictation types nothing until the end: what was heard still goes out.
        if (mode == Mode.REVISE) deliver(heard.toString().trim())
        done()
    }

    /** The speech service is done: type it all now, or revise it first. */
    private fun heardAll() {
        val d = dictation
        dictation = null
        d?.close()
        if (mode != Mode.REVISE || heard.isBlank()) return done()
        val raw = heard.toString().trim()
        val id = ++revising
        AppLog.add("revising ${raw.split(Regex("\\s+")).size} words")
        thread(name = "mirza-revise") {
            val r = Core.api.revise(raw)
            main.post {
                if (revising != id) return@post
                revising = 0
                r.note?.let {
                    AppLog.add("not revised: $it")
                    toast("Typed without AI revision: $it")
                }
                deliver(r.text)
                done(text = r.text, raw = if (r.revised) r.raw else "")
            }
        }
    }

    /** A tap while the AI works: type what was said as it is. */
    private fun skipRevision() {
        revising = 0
        dictation?.cancel()
        AppLog.add("revision skipped")
        val raw = heard.toString().trim()
        deliver(raw)
        done(text = raw, raw = "")
    }

    /** Ends the session; `text` (or what was heard) goes into the recent texts. */
    private fun done(text: String = heard.toString().trim(), raw: String = "") {
        main.removeCallbacks(tick)
        dictation?.close()
        dictation = null
        revising = 0
        val app = MirzaApp.app
        if (micStarted) app.stopService(Intent(app, MicService::class.java))
        val clipboard = app.getSystemService(ClipboardManager::class.java)
        if (untyped.isNotEmpty()) {
            clipboard.setPrimaryClip(sensitiveClip(untyped.toString().trim()))
            toast("Couldn't type ($untypedWhy). The text is in the clipboard.")
            untyped.clear()
        } else if (text.isNotEmpty() && Core.settings.keepOnClipboard) {
            clipboard.setPrimaryClip(sensitiveClip(text))
        }
        if (text.isNotEmpty()) History.add(text, raw)
        heard.clear()
        changeState(State.IDLE)
    }

    private fun changeState(s: State) {
        state = s
        stateListeners.forEach { it(s) }
    }

    /** Text from the speech service: typed now, or kept for the AI. */
    private fun heard(text: String) {
        lastSpeech = SystemClock.elapsedRealtime()
        heard.append(text)
        if (mode == Mode.DICTATE) deliver(text)
    }

    private fun deliver(text: String) {
        if (text.isEmpty()) return
        typedAny = true
        // After the first failure the rest goes to the clipboard too, so the text stays in order.
        val svc = TypingService.instance
        val why = when {
            untyped.isNotEmpty() -> untypedWhy
            svc == null -> "typing is off"
            else -> svc.type(text)
        }
        if (why != null) {
            if (untyped.isEmpty()) AppLog.add("not typed: $why")
            untypedWhy = why
            untyped.append(text)
        }
    }

    /** Called by the Rust core on its own threads; hands everything to the main thread. */
    private class Listener : DictationListener {
        override fun onText(text: String) {
            main.post { if (dictation != null) heard(text) }
        }

        override fun onPartial(text: String) {
            if (text.isNotBlank()) main.post { lastSpeech = SystemClock.elapsedRealtime() }
        }

        override fun onError(message: String) {
            main.post { if (dictation != null) fail(message, "speech service") }
        }

        override fun onFinished() {
            main.post { if (dictation != null) heardAll() }
        }
    }
}

/** Records 16-bit mono audio in 50 ms chunks on its own thread. */
class Recorder(private val rate: Int, private val onChunk: (ByteArray) -> Unit) {
    @Volatile private var running = false
    private var thread: Thread? = null
    private var rec: AudioRecord? = null

    @Volatile private var peak = 0

    /** Null when recording, or why it couldn't start. */
    @SuppressLint("MissingPermission")
    fun start(): String? {
        val chunkBytes = rate * 2 / 20
        val min = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        if (min <= 0) return "this phone can't record at $rate Hz"
        val r = try {
            AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, max(min, chunkBytes * 8))
        } catch (e: Exception) {
            return describe(e)
        }
        if (r.state != AudioRecord.STATE_INITIALIZED) {
            r.release()
            return "AudioRecord not initialized"
        }
        try {
            r.startRecording()
        } catch (e: Exception) {
            r.release()
            return describe(e)
        }
        if (r.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
            r.release()
            return "the microphone is busy or blocked"
        }
        rec = r
        running = true
        thread = Thread({ loop(r, chunkBytes) }, "mirza-mic").also { it.start() }
        return null
    }

    private fun loop(r: AudioRecord, n: Int) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        val buf = ByteArray(n)
        while (running) {
            var off = 0
            while (off < n && running) {
                val got = r.read(buf, off, n - off)
                if (got < 0) {
                    AppLog.add("microphone read error $got")
                    running = false
                    break
                }
                off += got
            }
            if (off < n) break
            if (peak == 0) {
                for (i in 0 until n / 2) peak = max(peak, abs((buf[2 * i].toInt() and 0xff) or (buf[2 * i + 1].toInt() shl 8)))
            }
            onChunk(buf.copyOf())
        }
    }

    /** Every sample so far was zero: Android or another app is holding the microphone. */
    fun heardNothing() = peak == 0

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
