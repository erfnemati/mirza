package dev.erfnemati.mirza

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.SoundPool
import android.os.Build
import android.os.IBinder
import dev.erfnemati.mirza.core.SoundKind
import dev.erfnemati.mirza.core.soundWav
import java.io.File

/** Mirza's notifications. Only while recording, for now. */
object Notes {
    private const val LISTENING = "listening"

    fun createChannels(ctx: Context) {
        val nm = ctx.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(LISTENING, "While listening", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Shown while Mirza records, with a Stop button. Android needs it to let Mirza use the microphone."
                setShowBadge(false)
            },
        )
    }

    fun listening(ctx: Context): Notification {
        val open = PendingIntent.getActivity(ctx, 0, Intent(ctx, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val stop = PendingIntent.getService(
            ctx,
            1,
            Intent(ctx, MicService::class.java).setAction(MicService.STOP),
            PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(ctx, LISTENING)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle("Mirza is listening")
            .setContentIntent(open)
            .addAction(Notification.Action.Builder(null, "Stop", stop).build())
            .setOngoing(true)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setCategory(Notification.CATEGORY_SERVICE)
            .build()
    }
}

/**
 * Holds the microphone while one dictation runs. Started on each tap and stopped right after, so
 * no notification stays between dictations.
 */
class MicService : Service() {
    companion object {
        const val STOP = "dev.erfnemati.mirza.STOP"
        private const val NOTE = 1
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == STOP) {
            Session.stop("notification")
            return START_NOT_STICKY
        }
        val ok = try {
            startForeground(NOTE, Notes.listening(this), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
            true
        } catch (e: Exception) {
            AppLog.add("microphone service refused: ${describe(e)}")
            // startForegroundService() promised a foreground service; keep that promise with a type
            // that needs nothing, or Android stops the whole app.
            if (Build.VERSION.SDK_INT >= 34) {
                runCatching { startForeground(NOTE, Notes.listening(this), ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE) }
            }
            false
        }
        if (!Session.micReady(ok)) stopSelf()
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        // Android stopped it (or Session did, after the end): never keep recording without it.
        if (Session.state == Session.State.LISTENING) Session.stop("microphone service stopped")
        super.onDestroy()
    }
}

/**
 * The desktop's start, stop and error sounds, made by the core at full volume and played through
 * SoundPool at the volume setting, squared like the desktop's, since loudness is heard on a log scale.
 */
object Sounds {
    private var pool: SoundPool? = null
    private val ids = mutableMapOf<SoundKind, Int>()

    private fun load(): SoundPool {
        pool?.let { return it }
        val p = SoundPool.Builder()
            .setMaxStreams(2)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                    .build(),
            )
            .build()
        val dir = File(MirzaApp.app.cacheDir, "sounds").apply { mkdirs() }
        for (kind in SoundKind.entries) {
            val f = File(dir, "${kind.name.lowercase()}.wav")
            f.writeBytes(soundWav(kind, 100u))
            ids[kind] = p.load(f.path, 1)
        }
        pool = p
        return p
    }

    /** Plays `kind` if sounds are on, or always with `test` (the volume setting). */
    fun play(kind: SoundKind, test: Boolean = false) {
        val s = Core.settings
        if (!s.sounds && !test) return
        val p = load()
        val v = s.soundVolume / 100f
        ids[kind]?.let { p.play(it, v * v, v * v, 1, 0, 1f) }
    }

    /** Loads the sounds ahead, so the first one isn't late. */
    fun warmUp() {
        if (Core.settings.sounds) load()
    }
}
