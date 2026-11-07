package dev.erfnemati.mirza

import android.app.ActivityManager
import android.app.Application
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.Toast
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class MirzaApp : Application() {
    override fun onCreate() {
        super.onCreate()
        app = this
        Core.init(this)
        History.load()
        Core.listeners.add { History.trim() }
        Notes.createChannels(this)
        AppLog.add(
            "started: Mirza ${BuildConfig.VERSION_NAME}, core ${dev.erfnemati.mirza.core.coreVersion()}, " +
                "${Build.MANUFACTURER} ${Build.MODEL}, Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})",
        )
        logLastExit()
    }

    /** Whether the phone force-stopped Mirza last time, which turns typing off (HyperOS does it from recents). */
    private fun logLastExit() {
        if (Build.VERSION.SDK_INT < 30) return
        val exit = runCatching {
            getSystemService(ActivityManager::class.java).getHistoricalProcessExitReasons(packageName, 0, 1).firstOrNull()
        }.getOrNull() ?: return
        val why = when (exit.reason) {
            ApplicationExitInfo.REASON_USER_REQUESTED -> "force stopped"
            ApplicationExitInfo.REASON_CRASH, ApplicationExitInfo.REASON_CRASH_NATIVE -> "crashed"
            ApplicationExitInfo.REASON_ANR -> "not responding"
            ApplicationExitInfo.REASON_LOW_MEMORY -> "low memory"
            ApplicationExitInfo.REASON_OTHER -> "other (often the phone's own cleaner)"
            ApplicationExitInfo.REASON_PACKAGE_UPDATED -> "updated"
            else -> "reason ${exit.reason}"
        }
        AppLog.add("last exit: $why at ${SimpleDateFormat("MM-dd HH:mm", Locale.US).format(Date(exit.timestamp))}")
    }

    companion object {
        lateinit var app: MirzaApp
            private set
    }
}

private val main = Handler(Looper.getMainLooper())

/** A short message from anywhere, shown on the main thread. */
fun toast(msg: String) {
    main.post { Toast.makeText(MirzaApp.app, msg, Toast.LENGTH_LONG).show() }
}

/** Runs `f` on the main thread. */
fun toMain(f: () -> Unit) {
    main.post(f)
}

/**
 * What happened, for "Copy log" in About and for logcat (tag Mirza). Never the recognized text
 * or a key: those stay out of logs.
 */
object AppLog {
    private val lines = ArrayDeque<String>()
    private val time = SimpleDateFormat("MM-dd HH:mm:ss", Locale.US)

    fun add(msg: String) {
        Log.i("Mirza", msg)
        synchronized(this) {
            lines.addLast("${time.format(Date())} $msg")
            while (lines.size > 300) lines.removeFirst()
        }
    }

    fun text(): String = synchronized(this) { lines.joinToString("\n") }
}

/** An exception's class and message, for the log. */
fun describe(e: Throwable): String = "${e.javaClass.simpleName}: ${e.message}"

fun Context.dp(v: Int): Int = (v * resources.displayMetrics.density).toInt()
