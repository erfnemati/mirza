package dev.erfnemati.mirza

import android.app.Application
import android.app.ForegroundServiceStartNotAllowedException
import android.content.Context
import android.content.SharedPreferences
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.CopyOnWriteArrayList

/** Where the bubble window comes from. */
enum class BubbleHost(val label: String) {
    OVERLAY("1: overlay window (SYSTEM_ALERT_WINDOW) in a foreground service"),
    ACCESSIBILITY("2: accessibility overlay drawn by the typing service"),
}

/** How the microphone is made available when the bubble is tapped. */
enum class MicMode(val label: String) {
    PROMOTE("A: bubble service adds the microphone type on tap"),
    SEPARATE("B: separate microphone service started on tap"),
    ALWAYS("C: bubble service has the microphone type all the time"),
    NONE("D: no microphone service, just record"),
}

/** How text gets into the other app's field. */
enum class TypingMethod(val label: String) {
    AUTO("Set text, paste if that fails"),
    SET_TEXT("Set text only"),
    PASTE("Paste only"),
}

class TrialApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Prefs.init(this)
        Notes.ensureChannel(this)
        TrialLog.add(
            "started: Mirza trial ${BuildConfig.VERSION_NAME}, core ${dev.erfnemati.mirza.core.coreVersion()}, " +
                "${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})",
        )
    }
}

/** The trial's settings, in app-private storage. The key is never logged. */
object Prefs {
    private lateinit var sp: SharedPreferences

    fun init(ctx: Context) {
        sp = ctx.getSharedPreferences("trial", Context.MODE_PRIVATE)
    }

    private fun str(key: String, def: String = "") = sp.getString(key, def) ?: def

    private fun put(key: String, value: String) = sp.edit().putString(key, value).apply()

    var apiKey: String
        get() = str("soniox_key")
        set(v) = put("soniox_key", v.trim())
    var proxy: String
        get() = str("proxy")
        set(v) = put("proxy", v.trim())
    var model: String
        get() = str("model")
        set(v) = put("model", v.trim())

    /** Comma-separated language hints, e.g. "fa, en". */
    var languages: String
        get() = str("languages", "fa, en")
        set(v) = put("languages", v)

    fun languageList() = languages.split(',', ' ').map { it.trim() }.filter { it.isNotEmpty() }

    var host: BubbleHost
        get() = runCatching { BubbleHost.valueOf(str("host", BubbleHost.OVERLAY.name)) }.getOrDefault(BubbleHost.OVERLAY)
        set(v) = put("host", v.name)
    var micMode: MicMode
        get() = runCatching { MicMode.valueOf(str("mic", MicMode.PROMOTE.name)) }.getOrDefault(MicMode.PROMOTE)
        set(v) = put("mic", v.name)
    var typing: TypingMethod
        get() = runCatching { TypingMethod.valueOf(str("typing", TypingMethod.AUTO.name)) }.getOrDefault(TypingMethod.AUTO)
        set(v) = put("typing", v.name)

    /** Recognized text in the log; off by default. */
    var logText: Boolean
        get() = sp.getBoolean("log_text", false)
        set(v) = sp.edit().putBoolean("log_text", v).apply()

    /**
     * Keep the last recording in the app's files (last-recording.pcm, 16-bit mono). No switch on
     * screen: set "save_audio" in the prefs with adb, to check what the microphone really gave.
     */
    val saveAudio: Boolean
        get() = sp.getBoolean("save_audio", false)

    /** The user wants the bubble shown. */
    var bubbleOn: Boolean
        get() = sp.getBoolean("bubble_on", false)
        set(v) = sp.edit().putBoolean("bubble_on", v).apply()
}

/** What happened, for the screen and for logcat (tag MirzaTrial). */
object TrialLog {
    private val lines = ArrayDeque<String>()
    private val time = SimpleDateFormat("HH:mm:ss.SSS", Locale.US)
    val listeners = CopyOnWriteArrayList<Runnable>()
    private val main = Handler(Looper.getMainLooper())

    fun add(msg: String) {
        Log.i("MirzaTrial", msg)
        synchronized(this) {
            lines.addLast("${time.format(Date())} $msg")
            while (lines.size > 400) lines.removeFirst()
        }
        main.post { listeners.forEach { it.run() } }
    }

    fun text(): String = synchronized(this) { lines.joinToString("\n") }

    fun clear() {
        synchronized(this) { lines.clear() }
        main.post { listeners.forEach { it.run() } }
    }
}

/** An exception's class and message, for the log. */
fun describe(e: Throwable): String {
    val kind = when {
        Build.VERSION.SDK_INT >= 31 && e is ForegroundServiceStartNotAllowedException ->
            "ForegroundServiceStartNotAllowedException"
        else -> e.javaClass.simpleName
    }
    return "$kind: ${e.message}"
}

fun Context.dp(v: Int): Int = (v * resources.displayMetrics.density).toInt()
