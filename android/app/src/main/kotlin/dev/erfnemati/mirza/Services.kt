package dev.erfnemati.mirza

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.view.WindowManager

object Notes {
    private const val CHANNEL = "mirza"

    fun ensureChannel(ctx: Context) {
        val nm = ctx.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, "Mirza bubble", NotificationManager.IMPORTANCE_LOW))
    }

    fun build(ctx: Context, text: String): Notification {
        val open = PendingIntent.getActivity(
            ctx,
            0,
            Intent(ctx, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle("Mirza trial")
            .setContentText(text)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
    }
}

private fun typeNames(types: Int): String {
    val names = mutableListOf<String>()
    if (Build.VERSION.SDK_INT >= 34 && types and ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE != 0) {
        names += "specialUse"
    }
    if (types and ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE != 0) names += "microphone"
    return names.joinToString("|").ifEmpty { "none" }
}

/**
 * Keeps the app alive while the bubble is shown, and draws the bubble when the
 * bubble host is the overlay window. Started from the activity, while it is in
 * front. In mic mode A it adds the microphone type on each tap; in mode C it
 * has it from the start.
 */
class BubbleService : Service() {
    companion object {
        @Volatile var instance: BubbleService? = null
            private set
        private const val NOTE = 1
    }

    var hasMic = false
        private set
    private var bubble: Bubble? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!foreground(Prefs.micMode == MicMode.ALWAYS, "start")) {
            stopSelf()
            return START_NOT_STICKY
        }
        if (Prefs.host == BubbleHost.OVERLAY && bubble == null) {
            val b = Bubble(this, WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY)
            if (b.show()) {
                bubble = b
                TrialLog.add("bubble shown (overlay window)")
            }
        }
        return START_NOT_STICKY
    }

    private fun foreground(mic: Boolean, why: String): Boolean {
        var types = if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0
        if (mic) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        val text = if (mic) "Bubble on, microphone allowed" else "Bubble on"
        return try {
            startForeground(NOTE, Notes.build(this, text), types)
            hasMic = mic
            TrialLog.add("bubble service: startForeground(${typeNames(types)}) OK ($why)")
            true
        } catch (e: Exception) {
            TrialLog.add("bubble service: startForeground(${typeNames(types)}) REFUSED ($why): ${describe(e)}")
            false
        }
    }

    /** Mode A: add the microphone type now. */
    fun promote(): Boolean = foreground(true, "tap")

    /** Mode A: drop the microphone type again. */
    fun demote() {
        if (hasMic && Prefs.micMode != MicMode.ALWAYS) foreground(false, "dictation ended")
    }

    override fun onDestroy() {
        bubble?.hide()
        bubble = null
        instance = null
        if (Session.state != Session.State.IDLE) Session.cancel("bubble service stopped")
        TrialLog.add("bubble service stopped")
        super.onDestroy()
    }
}

/** Mic mode B: a microphone service of its own, started on each tap. */
class MicService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val ok = try {
            startForeground(2, Notes.build(this, "Listening"), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
            TrialLog.add("mic service: startForeground(microphone) OK")
            true
        } catch (e: Exception) {
            TrialLog.add("mic service: startForeground(microphone) REFUSED: ${describe(e)}")
            // startForegroundService() promised a foreground service; keep that
            // promise with a type that needs nothing, or Android kills the app.
            if (Build.VERSION.SDK_INT >= 34) {
                runCatching {
                    startForeground(2, Notes.build(this, "Stopping"), ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
                }
            }
            false
        }
        Session.micServiceReady(ok)
        if (!ok) stopSelf()
        return START_NOT_STICKY
    }
}
