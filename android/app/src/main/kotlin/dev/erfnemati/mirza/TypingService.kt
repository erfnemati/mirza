package dev.erfnemati.mirza

import android.accessibilityservice.AccessibilityService
import android.app.KeyguardManager
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.provider.Settings as AndroidSettings
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
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
    private val main = Handler(Looper.getMainLooper())
    private val onSettings: (Settings) -> Unit = { updateBubble(it) }

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
        running = true
        AppLog.add("typing service connected")
        Sounds.warmUp()
        Core.listeners.add(onSettings)
        updateBubble(Core.settings)
    }

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
        bubble?.hide()
        bubble = null
        instance = null
        running = false
        Session.cancel("typing service stopped")
        AppLog.add("typing service disconnected")
        return super.onUnbind(intent)
    }

    private fun updateBubble(s: Settings) {
        if (s.bubbleShow && bubble == null) {
            bubble = Bubble(this).takeIf { it.show() }
        } else if (!s.bubbleShow) {
            bubble?.hide()
            bubble = null
        }
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
