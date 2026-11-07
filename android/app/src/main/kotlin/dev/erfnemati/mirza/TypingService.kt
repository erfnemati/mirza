package dev.erfnemati.mirza

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.app.KeyguardManager
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.provider.Settings as AndroidSettings
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import android.view.KeyEvent
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import kotlin.math.max
import kotlin.math.min

/**
 * The center of the app: draws the bubble over other apps (an accessibility overlay, so no
 * "draw over other apps" permission), and types into the focused field of the app in front, at
 * the cursor. It reads the focused field only for that.
 */
class TypingService : AccessibilityService() {
    companion object {
        @Volatile var instance: TypingService? = null
            private set

        /** Compose state for the setup checklist, which can be on screen when typing turns on. */
        var running by mutableStateOf(false)
            private set

        /** On in the Accessibility settings. It can be on there and still not running, after a force stop. */
        fun enabledInSettings(ctx: Context, component: ComponentName = ComponentName(ctx, TypingService::class.java)): Boolean {
            val list = AndroidSettings.Secure.getString(ctx.contentResolver, AndroidSettings.Secure.ENABLED_ACCESSIBILITY_SERVICES)
                ?: return false
            return list.split(':').any { ComponentName.unflattenFromString(it) == component }
        }

        /** The trial's typing service, which would draw a second bubble. */
        val TRIAL = ComponentName("dev.erfnemati.mirza.trial", "dev.erfnemati.mirza.TypingService")
    }

    private var lastEditable: AccessibilityNodeInfo? = null
    private var bubble: Bubble? = null
    /** The lock screen (or a dark screen) is in front, as the screen broadcasts tell it. */
    private var lockedNow = false
    private val main = Handler(Looper.getMainLooper())
    private val onSettings: (Settings) -> Unit = {
        updateBubble(it)
        updateTriggers(it.gestures)
    }
    private val volume by lazy { VolumeKey(this) }

    /**
     * Hides the bubble on the lock screen when the setting says so. "User present" comes while the
     * keyguard still says locked, so it counts as unlocked by itself.
     */
    private val screen = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, i: Intent) {
            lockedNow = when (i.action) {
                Intent.ACTION_SCREEN_OFF -> true
                Intent.ACTION_USER_PRESENT -> false
                else -> locked()
            }
            updateBubble(Core.settings)
            // Tasks said on the lock screen wait for the user now.
            if (i.action == Intent.ACTION_USER_PRESENT) VoiceTasks.unlocked()
        }
    }

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
        running = true
        AppLog.add("typing service connected")
        Sounds.warmUp()
        Core.listeners.add(onSettings)
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(Intent.ACTION_SCREEN_ON)
            addAction(Intent.ACTION_USER_PRESENT)
        }
        // Exported: "user present" comes from the system UI app, which a not-exported receiver
        // never hears. Only the system can send these three, so nothing else can reach it.
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(screen, filter, RECEIVER_EXPORTED) else registerReceiver(screen, filter)
        lockedNow = locked()
        updateBubble(Core.settings)
        updateTriggers(Core.settings.gestures)
    }

    /** Asks Android for the keys only while a volume gesture is set: the key filter sees every key. */
    private fun updateTriggers(g: Gestures) {
        val info = serviceInfo ?: return
        val on = volume.wanted(g)
        val flag = AccessibilityServiceInfo.FLAG_REQUEST_FILTER_KEY_EVENTS
        if (on == (info.flags and flag != 0)) return
        if (!on) volume.reset()
        info.flags = if (on) info.flags or flag else info.flags and flag.inv()
        serviceInfo = info
        AppLog.add("volume down: held ${g.volumeLongPress.id}, twice ${g.volumeDoublePress.id}")
    }

    override fun onKeyEvent(e: KeyEvent): Boolean = volume.onKey(e)

    override fun onAccessibilityEvent(e: AccessibilityEvent) {
        if (e.packageName?.toString() == packageName) return
        when (e.eventType) {
            AccessibilityEvent.TYPE_VIEW_FOCUSED, AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED -> {
                val node = e.source
                if (node != null && node.isEditable) lastEditable = node
            }
        }
    }

    override fun onInterrupt() {}

    override fun onUnbind(intent: Intent?): Boolean {
        Core.listeners.remove(onSettings)
        runCatching { unregisterReceiver(screen) }
        volume.reset()
        bubble?.hide()
        bubble = null
        instance = null
        running = false
        Session.cancel("typing service stopped")
        AppLog.add("typing service disconnected")
        return super.onUnbind(intent)
    }

    private var look: BubbleSettings? = null

    /** Shows, hides or redraws the bubble after a setting changed or the phone locked or unlocked. */
    private fun updateBubble(s: Settings) {
        val b = s.bubble
        val want = b.show && (b.lockScreen || !lockedNow)
        val same = look?.let { it.size == b.size && it.logo == b.logo } == true
        if (bubble != null && (!want || !same)) {
            bubble?.hide()
            bubble = null
        }
        if (want && bubble == null) {
            bubble = Bubble(this, b).takeIf { it.show() }
            look = b
        }
        bubble?.setOpacity(b.opacity)
    }

    /** Draws the bubble again, e.g. at its first place after "Move it back". */
    fun redrawBubble() {
        bubble?.hide()
        bubble = null
        updateBubble(Core.settings)
    }

    /** The phone is locked: Mirza never types or acts there. */
    fun locked(): Boolean = getSystemService(KeyguardManager::class.java).isKeyguardLocked

    fun hasField(): Boolean = target() != null

    /**
     * Types `text` at the cursor. Returns null when typed, or why not. Never into a password or
     * PIN field: paste would get around the check in SET_TEXT, and passwords must not go through a
     * speech service.
     */
    fun type(text: String): String? {
        val node = target() ?: return "no text field"
        if (node.isPassword) return "a password field"
        val app = node.packageName
        val problem = setText(node, text) ?: return null
        AppLog.add("set text refused in $app ($problem), pasting")
        return if (paste(node, text)) null else "$app refused the text"
    }

    /** The focused editable field in another app, in any window. */
    private fun target(): AccessibilityNodeInfo? {
        val focused = findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
        if (focused != null && focused.isEditable && focused.packageName != packageName) return focused
        val last = lastEditable ?: return null
        return if (last.refresh() && last.isEditable && last.isFocused) last else null
    }

    /** Whether the field's text is really its hint, as a plain EditText says or as text equal to the hint. */
    private fun showsHint(node: AccessibilityNodeInfo): Boolean {
        if (node.isShowingHintText) return true
        val text = node.text?.toString() ?: return false
        return text == node.hintText?.toString()
    }

    /** Replaces the field's text with itself plus `text` at the cursor. Null on success. */
    private fun setText(node: AccessibilityNodeInfo, text: String): String? {
        val old = if (showsHint(node)) "" else node.text?.toString() ?: ""
        var s = node.textSelectionStart
        var e = node.textSelectionEnd
        if (old.isNotEmpty() && (s < 0 || e < 0)) {
            // A field reports a cursor whenever it holds text. Text without a cursor is most likely
            // the hint shown as text, as Telegram and WhatsApp do with "Message": setting the text
            // would keep the hint, or could drop real text. Paste goes in at the real cursor.
            return "text but no cursor, maybe the hint"
        }
        if (s < 0 || e < 0 || s > old.length || e > old.length) {
            s = old.length
            e = old.length
        }
        val a = min(s, e)
        val b = max(s, e)
        val args = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, old.substring(0, a) + text + old.substring(b))
        }
        if (!node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) return "the app said no"
        val pos = a + text.length
        node.performAction(
            AccessibilityNodeInfo.ACTION_SET_SELECTION,
            Bundle().apply {
                putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, pos)
                putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, pos)
            },
        )
        return null
    }

    /** Puts `text` on the clipboard, pastes it, and clears the clipboard. */
    private fun paste(node: AccessibilityNodeInfo, text: String): Boolean {
        val cm = getSystemService(ClipboardManager::class.java)
        cm.setPrimaryClip(sensitiveClip(text))
        val ok = node.performAction(AccessibilityNodeInfo.ACTION_PASTE)
        main.postDelayed({ runCatching { cm.clearPrimaryClip() } }, 500)
        return ok
    }
}

/** A clip kept out of the clipboard preview (and history, where respected). */
fun sensitiveClip(text: String): ClipData {
    val clip = ClipData.newPlainText("Mirza", text)
    if (Build.VERSION.SDK_INT >= 33) {
        clip.description.extras = PersistableBundle().apply { putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true) }
    }
    return clip
}
