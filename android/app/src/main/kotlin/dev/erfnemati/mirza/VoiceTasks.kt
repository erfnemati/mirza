package dev.erfnemati.mirza

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import dev.erfnemati.mirza.core.MobileException
import dev.erfnemati.mirza.core.SoundKind
import dev.erfnemati.mirza.core.TaskBook
import dev.erfnemati.mirza.core.TaskOutcome
import dev.erfnemati.mirza.core.TaskState
import java.io.File
import java.util.TimeZone
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

/**
 * Voice tasks on the phone: the core's task book (queue, AI reading, Google calls) and what
 * Android adds around it: the notifications with Undo and Edit, the tasks said on the lock screen,
 * and the background job that tries waiting tasks again. Nothing said is lost: it is on disk
 * before anything goes out, and if even that fails it goes to the clipboard.
 */
object VoiceTasks {
    private const val CHANNEL = "tasks"
    private const val HELD_NOTE = 2
    private const val RETRY = "retry-tasks"

    private var book: TaskBook? = null
    private var zone = ""

    /** Compose state, bumped when tasks changed, so the Voice tasks screen reads them again. */
    var changes by mutableIntStateOf(0)
        private set

    fun init(ctx: Context) {
        val nm = ctx.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, "Voice tasks", NotificationManager.IMPORTANCE_DEFAULT).apply {
                description = "Whether a task you said was saved, with Undo and Edit, and the tasks said on the lock screen."
                setSound(null, null)
            },
        )
        // New settings or keys go to the task book; its queue stays.
        Core.listeners.add { reload() }
    }

    /** The task book, made on first use. Null (and logged) when the settings don't allow one. */
    @Synchronized
    fun book(): TaskBook? {
        book?.let { return it }
        zone = TimeZone.getDefault().id
        return try {
            val dir = File(MirzaApp.app.filesDir, "mirza/tasks").path
            Core.api.taskBook(dir, zone, Google.Tokens).also { book = it }
        } catch (e: MobileException) {
            AppLog.add("voice tasks: ${e.message}")
            null
        }
    }

    @Synchronized
    private fun reload() {
        val b = book ?: return
        zone = TimeZone.getDefault().id
        try {
            Core.api.updateTaskBook(b, zone)
        } catch (e: MobileException) {
            AppLog.add("voice tasks: ${e.message}")
        }
        changed()
    }

    /** The phone may have moved to another time zone since the last task. */
    private fun ready(): TaskBook? {
        val b = book() ?: return null
        if (TimeZone.getDefault().id != zone) reload()
        return b
    }

    private fun changed() {
        toMain { changes++ }
    }

    /**
     * What was said with the task gesture (main thread): kept on disk at once, then read and saved
     * in the background. The panel shows the outcome if it is still open, a notification otherwise.
     */
    fun add(text: String) {
        val b = ready() ?: return lost(text, "the settings are broken")
        val w = try {
            b.capture(text)
        } catch (e: MobileException) {
            return lost(text, e.message ?: "unknown error")
        }
        AppLog.add("task kept, saving")
        changed()
        TaskPanel.saving(w.id, text)
        save(w.id)
    }

    /** Reads and saves one task in the background. */
    fun save(id: String) {
        val b = book() ?: return
        thread(name = "mirza-task") {
            val o = b.process(id)
            toMain {
                AppLog.add("task ${o.state.name.lowercase()}")
                changed()
                if (!TaskPanel.outcome(o)) notify(o)
                schedule()
            }
        }
    }

    /**
     * A task said on the lock screen (main thread): kept until the phone is unlocked and the task
     * confirmed; nothing goes to the AI or Google before. Returns whether it was kept.
     */
    fun hold(text: String): Boolean {
        val b = ready() ?: return false.also { AppLog.add("held task lost: no task book") }
        return try {
            b.captureHeld(text)
            AppLog.add("task held for confirmation")
            changed()
            heldNote(null)
            true
        } catch (e: MobileException) {
            AppLog.add("held task refused: ${e.message}")
            heldNote(e.message)
            false
        }
    }

    /** The tasks said on the lock screen, oldest first. */
    fun held() = book()?.waiting()?.filter { it.held }?.sortedBy { it.saidMs }.orEmpty()

    /** After unlocking: the notification that says tasks wait, if any do. */
    fun unlocked() {
        if (held().isNotEmpty()) heldNote(null)
    }

    /** Signed in: waiting tasks are tried again now. */
    fun signedIn() {
        book()?.retryNow()
        changed()
        schedule()
    }

    private fun lost(text: String, why: String) {
        AppLog.add("task not kept: $why")
        val app = MirzaApp.app
        app.getSystemService(ClipboardManager::class.java).setPrimaryClip(sensitiveClip(text))
        Sounds.play(SoundKind.ERROR)
        toast("Couldn't keep the task ($why). What you said is in the clipboard.")
        TaskPanel.close()
    }

    /** The background job, due when the next waiting task is, once there is a connection. */
    fun schedule() {
        val wm = WorkManager.getInstance(MirzaApp.app)
        val next = book()?.nextDueMs()
        if (next == null) {
            wm.cancelUniqueWork(RETRY)
            return
        }
        val delay = (next - System.currentTimeMillis()).coerceAtLeast(0)
        val job = OneTimeWorkRequestBuilder<RetryWorker>()
            .setInitialDelay(delay, TimeUnit.MILLISECONDS)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .build()
        wm.enqueueUniqueWork(RETRY, ExistingWorkPolicy.REPLACE, job)
    }

    /** Called by the job: saves what is due, says what was saved or needs the user. */
    fun retry() {
        val out = book()?.processDue().orEmpty()
        if (out.isNotEmpty()) AppLog.add("retried ${out.size} tasks: " + out.joinToString { it.state.name.lowercase() })
        toMain {
            changed()
            // A task still waiting already has its notification; only news gets one.
            out.filter { it.state == TaskState.SAVED || it.state == TaskState.STUCK }.forEach { notify(it) }
            schedule()
        }
    }

    /** Undo, from a notification or the panel (background thread). Returns why it failed, or null. */
    fun undo(id: String): String? {
        val b = book() ?: return "the settings are broken"
        return try {
            b.undo(id)
            AppLog.add("task undone")
            changed()
            toMain { schedule() }
            null
        } catch (e: MobileException) {
            e.message ?: "unknown error"
        }
    }

    private fun noteId(taskId: String) = 1000 + (taskId.hashCode() and 0xfffffff)

    /** A task's outcome as a notification: Undo and Edit for a saved one, Edit for the others. */
    fun notify(o: TaskOutcome) {
        if (o.state == TaskState.GONE || o.title.isEmpty()) return
        val app = MirzaApp.app
        val n = noteId(o.id)
        val edit = PendingIntent.getActivity(
            app,
            n,
            TaskPanelActivity.edit(app, o.id),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val open = PendingIntent.getActivity(
            app,
            0,
            Intent(app, MainActivity::class.java).putExtra(MainActivity.SCREEN, Screen.TASKS.name),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val b = Notification.Builder(app, CHANNEL)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle(if (o.state == TaskState.SAVED) "Added: ${o.title}" else o.title)
            .setContentText(o.body.lineSequence().first())
            .setStyle(Notification.BigTextStyle().bigText(o.body))
            .setContentIntent(open)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
        if (o.state == TaskState.SAVED) {
            val undo = PendingIntent.getBroadcast(
                app,
                n,
                Intent(app, TaskActionReceiver::class.java).setAction(TaskActionReceiver.UNDO).putExtra("id", o.id),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            b.addAction(Notification.Action.Builder(null, "Undo", undo).build())
        }
        b.addAction(Notification.Action.Builder(null, "Edit", edit).build())
        app.getSystemService(NotificationManager::class.java).notify(n, b.build())
    }

    fun cancelNote(taskId: String) {
        MirzaApp.app.getSystemService(NotificationManager::class.java).cancel(noteId(taskId))
    }

    /** "N tasks wait for you", or why the last one wasn't kept; tapping it goes through them. */
    private fun heldNote(refused: String?) {
        val app = MirzaApp.app
        val nm = app.getSystemService(NotificationManager::class.java)
        val n = held().size
        if (n == 0 && refused == null) return nm.cancel(HELD_NOTE)
        val review = PendingIntent.getActivity(
            app,
            HELD_NOTE,
            TaskPanelActivity.review(app),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val title = if (n == 1) "A task waits for you" else "$n tasks wait for you"
        val text = refused?.let { "The last one wasn't kept: $it" } ?: "Said on the lock screen. Tap to confirm, change or drop each one."
        nm.notify(
            HELD_NOTE,
            Notification.Builder(app, CHANNEL)
                .setSmallIcon(R.drawable.ic_mic)
                .setContentTitle(title)
                .setContentText(text)
                .setStyle(Notification.BigTextStyle().bigText(text))
                .setContentIntent(review)
                // Its own group: in a group with the other task notifications, a tap on the
                // collapsed group would open Mirza's home instead of the tasks to confirm.
                .setGroup("held")
                .setAutoCancel(true)
                .setOnlyAlertOnce(true)
                .setVisibility(Notification.VISIBILITY_PRIVATE)
                .build(),
        )
    }

    /** The held tasks changed (confirmed, edited or dropped): the notification follows. */
    fun heldChanged() {
        val nm = MirzaApp.app.getSystemService(NotificationManager::class.java)
        if (held().isEmpty()) nm.cancel(HELD_NOTE) else heldNote(null)
        changed()
    }
}

/** Undo on a task notification. */
class TaskActionReceiver : BroadcastReceiver() {
    companion object {
        const val UNDO = "dev.erfnemati.mirza.UNDO_TASK"
    }

    override fun onReceive(ctx: Context, i: Intent) {
        val id = i.getStringExtra("id") ?: return
        if (i.action != UNDO) return
        val done = goAsync()
        thread(name = "mirza-undo") {
            val why = VoiceTasks.undo(id)
            toMain {
                if (why == null) {
                    VoiceTasks.cancelNote(id)
                    toast("Task undone.")
                } else {
                    toast("Couldn't undo: $why")
                }
                done.finish()
            }
        }
    }
}

/** Tries the waiting tasks again once they are due and the phone is online. */
class RetryWorker(ctx: Context, params: WorkerParameters) : Worker(ctx, params) {
    override fun doWork(): Result {
        VoiceTasks.retry()
        return Result.success()
    }
}
